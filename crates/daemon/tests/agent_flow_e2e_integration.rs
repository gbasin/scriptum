use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::process::Command;

use scriptum_common::protocol::jsonrpc::{Request, RequestId};
use scriptum_daemon::engine::ydoc::YDoc;
use scriptum_daemon::git::attribution::{with_coauthor_trailers, UpdateAttribution};
use scriptum_daemon::git::commit::{
    generate_ai_commit_message, AiCommitClient, AiCommitError, RedactionPolicy,
};
use scriptum_daemon::git::triggers::{ChangeType, ChangedFile};
use scriptum_daemon::git::worker::GitWorker;
use scriptum_daemon::rpc::methods::{dispatch_request, RpcServerState};
use serde_json::json;
use tempfile::TempDir;
use uuid::Uuid;

struct FixedAiClient {
    message: String,
}

impl AiCommitClient for FixedAiClient {
    fn generate(
        &self,
        _system: &str,
        _user_prompt: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, AiCommitError>> + Send>> {
        let message = self.message.clone();
        Box::pin(async move { Ok(message) })
    }
}

#[tokio::test]
async fn mcp_edit_flows_through_daemon_crdt_file_and_git_with_attribution() {
    // This models the lifecycle:
    // MCP scriptum_edit -> daemon doc.edit -> CRDT sync (editor) -> file -> git commit.
    let temp = TempDir::new().expect("tempdir should be created");
    let remote_path = temp.path().join("remote.git");
    let repo_path = temp.path().join("repo");

    run_git(temp.path(), &["init", "--bare", remote_path.to_str().expect("utf8 remote path")]);
    run_git(temp.path(), &["init", "-b", "main", repo_path.to_str().expect("utf8 repo path")]);
    run_git(&repo_path, &["config", "user.name", "Scriptum Bot"]);
    run_git(&repo_path, &["config", "user.email", "scriptum-bot@example.test"]);
    run_git(
        &repo_path,
        &["remote", "add", "origin", remote_path.to_str().expect("utf8 remote path")],
    );

    std::fs::create_dir_all(repo_path.join("docs")).expect("docs directory should be created");
    std::fs::write(repo_path.join("docs/agent-flow.md"), "# Agent Flow\n\nInitial content.\n")
        .expect("seed file should be written");
    run_git(&repo_path, &["add", "."]);
    run_git(&repo_path, &["commit", "-m", "chore: initial commit"]);
    run_git(&repo_path, &["push", "-u", "origin", "main"]);

    let state = RpcServerState::default();
    let workspace_id = Uuid::new_v4();
    let doc_id = Uuid::new_v4();
    state
        .seed_doc(
            workspace_id,
            doc_id,
            "docs/agent-flow.md",
            "Agent Flow",
            "# Agent Flow\n\nInitial content.\n",
        )
        .await;

    let editor_state_before_edit = {
        let manager = state.doc_manager_for_test().read().await;
        let doc = manager.get_doc(doc_id).expect("doc should exist before edit");
        doc.encode_state()
    };
    let editor_client = YDoc::with_client_id(17);
    editor_client
        .apply_update(&editor_state_before_edit)
        .expect("editor client should apply initial state");

    let edit_request = Request::new(
        "doc.edit",
        Some(json!({
            "workspace_id": workspace_id,
            "doc_id": doc_id,
            "client_update_id": "mcp-update-1",
            "content_md": "# Agent Flow\n\nEdited from MCP.\n",
            "agent_id": "claude-code",
        })),
        RequestId::Number(1),
    );
    let edit_response = dispatch_request(edit_request, &state).await;
    assert!(
        edit_response.error.is_none(),
        "doc.edit should succeed for MCP flow: {edit_response:?}"
    );

    let daemon_diff_for_editor = {
        let manager = state.doc_manager_for_test().read().await;
        let doc = manager.get_doc(doc_id).expect("doc should exist after edit");
        let state_vector = editor_client.encode_state_vector();
        doc.encode_diff(&state_vector).expect("daemon should encode CRDT diff")
    };
    editor_client
        .apply_update(&daemon_diff_for_editor)
        .expect("editor client should apply daemon diff");
    let editor_text = editor_client.get_text_string("content");
    assert!(editor_text.contains("Edited from MCP."), "editor client should see updated content");

    let read_request = Request::new(
        "doc.read",
        Some(json!({
            "workspace_id": workspace_id,
            "doc_id": doc_id,
            "include_content": true,
        })),
        RequestId::Number(2),
    );
    let read_response = dispatch_request(read_request, &state).await;
    assert!(
        read_response.error.is_none(),
        "doc.read should succeed after MCP edit: {read_response:?}"
    );
    let read_result = read_response.result.expect("doc.read result should be populated");
    let content =
        read_result["content_md"].as_str().expect("content_md should be a string").to_string();
    assert!(content.contains("Edited from MCP."));
    assert_eq!(
        read_result["metadata"]["head_seq"].as_i64(),
        Some(1),
        "head_seq should advance after doc.edit"
    );

    std::fs::write(repo_path.join("docs/agent-flow.md"), &content)
        .expect("daemon content should sync to file");

    let changed_files = vec![ChangedFile {
        path: "docs/agent-flow.md".to_string(),
        doc_id: Some(doc_id),
        change_type: ChangeType::Modified,
    }];
    let ai_client = FixedAiClient { message: "docs(agent-flow): apply mcp edit".to_string() };
    let ai_message = generate_ai_commit_message(
        &ai_client,
        "@@ -1,3 +1,3 @@\n-Initial content.\n+Edited from MCP.\n",
        &changed_files,
        RedactionPolicy::Full,
    )
    .await
    .expect("ai message should be generated");
    let final_message =
        with_coauthor_trailers(&ai_message, &[UpdateAttribution::for_agent("claude-code")]);

    let worker = GitWorker::new(&repo_path);
    worker.add(&["docs/agent-flow.md"]).expect("git add should succeed");
    worker.commit(&final_message).expect("git commit should succeed");
    worker.push().expect("git push should succeed");

    let file_on_disk =
        std::fs::read_to_string(repo_path.join("docs/agent-flow.md")).expect("file should exist");
    assert!(file_on_disk.contains("Edited from MCP."));

    let commit_message = run_git_capture(&repo_path, &["log", "-1", "--pretty=%B"]);
    assert!(commit_message.contains("docs(agent-flow): apply mcp edit"));
    assert!(
        commit_message.contains("Co-authored-by: claude-code <agent:claude-code@scriptum>"),
        "expected agent coauthor trailer, got: {commit_message}"
    );
}

fn run_git(cwd: &Path, args: &[&str]) {
    let output =
        Command::new("git").args(args).current_dir(cwd).output().expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed:\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_git_capture(cwd: &Path, args: &[&str]) -> String {
    let output =
        Command::new("git").args(args).current_dir(cwd).output().expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed:\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf8 output")
}
