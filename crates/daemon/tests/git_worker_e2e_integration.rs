use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::process::Command;

use scriptum_daemon::git::attribution::{with_coauthor_trailers, UpdateAttribution};
use scriptum_daemon::git::commit::{
    generate_ai_commit_message, AiCommitClient, AiCommitError, RedactionPolicy,
};
use scriptum_daemon::git::triggers::{
    ChangeType, ChangedFile, TriggerCollector, TriggerConfig, TriggerEvent,
};
use scriptum_daemon::git::worker::GitWorker;
use tempfile::TempDir;

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
async fn git_worker_e2e_commit_with_ai_message_and_coauthors_and_push() {
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

    std::fs::write(repo_path.join("README.md"), "# Scriptum\n\nInitial\n")
        .expect("seed file should be written");
    run_git(&repo_path, &["add", "."]);
    run_git(&repo_path, &["commit", "-m", "chore: initial commit"]);
    run_git(&repo_path, &["push", "-u", "origin", "main"]);

    std::fs::write(
        repo_path.join("README.md"),
        "# Scriptum\n\nInitial\n\nGit worker integration.\n",
    )
    .expect("updated file should be written");

    let mut collector = TriggerCollector::new(TriggerConfig::default());
    collector.push_trigger(TriggerEvent::LeaseReleased {
        agent: "claude".to_string(),
        doc_path: "README.md".to_string(),
        section_heading: "Git Worker".to_string(),
    });
    collector.push_trigger(TriggerEvent::CommentResolved {
        agent: "cursor".to_string(),
        doc_path: "README.md".to_string(),
        thread_id: "thread-1".to_string(),
    });

    let changed_files = vec![ChangedFile {
        path: "README.md".to_string(),
        doc_id: None,
        change_type: ChangeType::Modified,
    }];
    let context = collector
        .take_commit_context(changed_files.clone())
        .expect("commit context should be available");

    let ai_client =
        FixedAiClient { message: "docs(readme): capture git worker integration flow".to_string() };
    let ai_message = generate_ai_commit_message(
        &ai_client,
        "@@ -1,3 +1,5 @@\n+Git worker integration.\n",
        &changed_files,
        RedactionPolicy::Full,
    )
    .await
    .expect("ai message should be generated");

    let attributions: Vec<UpdateAttribution> =
        context.agents_involved.iter().cloned().map(UpdateAttribution::for_agent).collect();
    let final_message = with_coauthor_trailers(&ai_message, &attributions);

    let worker = GitWorker::new(&repo_path);
    worker.add(&["README.md"]).expect("git add should succeed");
    worker.commit(&final_message).expect("git commit should succeed");
    worker.push().expect("git push should succeed");

    let commit_message = run_git_capture(&repo_path, &["log", "-1", "--pretty=%B"]);
    assert!(commit_message.contains("docs(readme): capture git worker integration flow"));
    assert!(
        commit_message.contains("Co-authored-by: claude <agent:claude@scriptum>"),
        "expected claude trailer, got: {commit_message}"
    );
    assert!(
        commit_message.contains("Co-authored-by: cursor <agent:cursor@scriptum>"),
        "expected cursor trailer, got: {commit_message}"
    );

    let local_head = run_git_capture(&repo_path, &["rev-parse", "HEAD"]);
    let remote_head = run_git_capture(
        temp.path(),
        &[
            "--git-dir",
            remote_path.to_str().expect("utf8 remote path"),
            "rev-parse",
            "refs/heads/main",
        ],
    );
    assert_eq!(local_head.trim(), remote_head.trim(), "remote should receive pushed commit");
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
