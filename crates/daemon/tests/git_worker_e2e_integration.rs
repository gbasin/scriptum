use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{
    extract::Json,
    http::{HeaderMap, StatusCode},
    routing::post,
    Router,
};
use scriptum_common::protocol::jsonrpc::{Request as RpcRequest, RequestId};
use scriptum_daemon::git::attribution::{with_coauthor_trailers, UpdateAttribution};
use scriptum_daemon::git::commit::{
    generate_ai_commit_message, AiCommitClient, AiCommitError, RedactionPolicy,
};
use scriptum_daemon::git::triggers::{
    ChangeType, ChangedFile, TriggerCollector, TriggerConfig, TriggerEvent,
};
use scriptum_daemon::git::worker::{GitWorker, ProcessCommandExecutor};
use scriptum_daemon::rpc::methods::{dispatch_request, GitState, RpcServerState};
use serde_json::json;
use tempfile::TempDir;
use tokio::sync::oneshot;

const ANTHROPIC_API_VERSION: &str = "2023-06-01";
const ANTHROPIC_MODEL: &str = "claude-haiku-4-5-20250929";
const ANTHROPIC_MAX_TOKENS: usize = 200;

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

#[derive(Debug)]
struct CapturedAnthropicCall {
    x_api_key: Option<String>,
    anthropic_version: Option<String>,
    content_type: Option<String>,
    body: serde_json::Value,
}

#[derive(Clone)]
struct AnthropicWireMockClient {
    http: reqwest::Client,
    api_url: String,
    api_key: String,
    model: String,
    max_tokens: usize,
}

impl AnthropicWireMockClient {
    fn new(api_url: String, api_key: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_url,
            api_key: api_key.to_string(),
            model: ANTHROPIC_MODEL.to_string(),
            max_tokens: ANTHROPIC_MAX_TOKENS,
        }
    }
}

impl AiCommitClient for AnthropicWireMockClient {
    fn generate(
        &self,
        system: &str,
        user_prompt: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, AiCommitError>> + Send>> {
        let http = self.http.clone();
        let api_url = self.api_url.clone();
        let api_key = self.api_key.clone();
        let model = self.model.clone();
        let max_tokens = self.max_tokens;
        let system = system.to_string();
        let user_prompt = user_prompt.to_string();

        Box::pin(async move {
            let request_body = json!({
                "model": model,
                "max_tokens": max_tokens,
                "system": system,
                "messages": [
                    {
                        "role": "user",
                        "content": user_prompt
                    }
                ]
            });

            let response = http
                .post(api_url)
                .header("x-api-key", api_key)
                .header("anthropic-version", ANTHROPIC_API_VERSION)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .json(&request_body)
                .send()
                .await
                .map_err(|error| {
                    AiCommitError::ClientError(format!(
                        "failed to call mock anthropic api: {error}"
                    ))
                })?;

            if !response.status().is_success() {
                return Err(AiCommitError::ClientError(format!(
                    "mock anthropic api returned status {}",
                    response.status()
                )));
            }

            let payload: serde_json::Value = response.json().await.map_err(|error| {
                AiCommitError::ClientError(format!(
                    "failed to decode mock anthropic response payload: {error}"
                ))
            })?;

            let message = payload["content"]
                .as_array()
                .and_then(|blocks| {
                    blocks.iter().find_map(|block| {
                        if block.get("type").and_then(|value| value.as_str()) != Some("text") {
                            return None;
                        }
                        block
                            .get("text")
                            .and_then(|value| value.as_str())
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(str::to_string)
                    })
                })
                .ok_or_else(|| {
                    AiCommitError::ClientError(
                        "mock anthropic response did not contain text content".to_string(),
                    )
                })?;

            Ok(message)
        })
    }
}

#[derive(Clone, Default)]
struct CountingAiClient {
    calls: Arc<AtomicUsize>,
}

impl CountingAiClient {
    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl AiCommitClient for CountingAiClient {
    fn generate(
        &self,
        _system: &str,
        _user_prompt: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, AiCommitError>> + Send>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok("feat: should not be used when AI is disabled".to_string()) })
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

#[tokio::test]
async fn git_sync_e2e_generates_ai_message_and_validates_anthropic_request_shape() {
    let temp = TempDir::new().expect("tempdir should be created");
    let repo_path = setup_repo_for_sync(&temp);
    write_repo_edit(&repo_path, "# Scriptum\n\nEdited by e2e AI flow.\n");

    let (api_url, captured_rx, server) = spawn_mock_anthropic_server(
        StatusCode::OK,
        json!({
            "content": [
                { "type": "text", "text": "feat: generate commit from anthropic mock" }
            ]
        }),
    )
    .await;

    let ai_client = Arc::new(AnthropicWireMockClient::new(api_url, "sk-ant-local-test"));
    run_git_sync_commit(
        &repo_path,
        ai_client,
        true,
        RedactionPolicy::Full,
        "docs: checkpoint from e2e",
    )
    .await;

    let commit_message = latest_commit_message(&repo_path);
    assert_eq!(
        commit_message.lines().next().unwrap_or_default(),
        "feat: generate commit from anthropic mock",
    );
    assert!(
        commit_message.contains("Scriptum-Trigger: checkpoint"),
        "expected checkpoint trigger trailer, got: {commit_message}"
    );

    let captured = tokio::time::timeout(Duration::from_secs(2), captured_rx)
        .await
        .expect("mock anthropic server should receive request")
        .expect("captured request should be present");
    assert_eq!(captured.x_api_key.as_deref(), Some("sk-ant-local-test"));
    assert_eq!(captured.anthropic_version.as_deref(), Some(ANTHROPIC_API_VERSION));
    assert!(captured
        .content_type
        .as_deref()
        .is_some_and(|value| value.contains("application/json")));
    assert_eq!(captured.body["model"], json!(ANTHROPIC_MODEL));
    assert_eq!(captured.body["max_tokens"], json!(ANTHROPIC_MAX_TOKENS));
    assert!(captured.body["system"]
        .as_str()
        .is_some_and(|value| value.contains("Generate concise git commit")));
    assert_eq!(captured.body["messages"][0]["role"], "user");
    let prompt = captured.body["messages"][0]["content"]
        .as_str()
        .expect("anthropic user prompt should be a string");
    assert!(prompt.contains("Trigger summary:"));
    assert!(prompt.contains("docs: checkpoint from e2e"));
    assert!(prompt.contains("Staged diff:"));
    assert!(prompt.contains("docs/readme.md"));

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn git_sync_e2e_uses_fallback_message_when_ai_disabled() {
    let temp = TempDir::new().expect("tempdir should be created");
    let repo_path = setup_repo_for_sync(&temp);
    write_repo_edit(&repo_path, "# Scriptum\n\nEdited with AI disabled.\n");

    let ai_client = CountingAiClient::default();
    run_git_sync_commit(
        &repo_path,
        Arc::new(ai_client.clone()),
        false,
        RedactionPolicy::Disabled,
        "docs: disabled ai checkpoint",
    )
    .await;

    assert_eq!(ai_client.call_count(), 0, "AI client should not be called when disabled");
    let commit_message = latest_commit_message(&repo_path);
    assert_eq!(
        commit_message.lines().next().unwrap_or_default(),
        "Update 1 file(s): docs/readme.md"
    );
    assert!(
        commit_message.contains("Scriptum-Trigger: checkpoint"),
        "expected checkpoint trigger trailer, got: {commit_message}"
    );
}

#[tokio::test]
async fn git_sync_e2e_uses_fallback_message_when_ai_api_fails() {
    let temp = TempDir::new().expect("tempdir should be created");
    let repo_path = setup_repo_for_sync(&temp);
    write_repo_edit(&repo_path, "# Scriptum\n\nEdited with failing AI API.\n");

    let (api_url, captured_rx, server) = spawn_mock_anthropic_server(
        StatusCode::INTERNAL_SERVER_ERROR,
        json!({ "error": { "message": "mock failure" } }),
    )
    .await;

    let ai_client = Arc::new(AnthropicWireMockClient::new(api_url, "sk-ant-local-test"));
    run_git_sync_commit(
        &repo_path,
        ai_client,
        true,
        RedactionPolicy::Redacted,
        "docs: failing ai checkpoint",
    )
    .await;

    let commit_message = latest_commit_message(&repo_path);
    assert_eq!(
        commit_message.lines().next().unwrap_or_default(),
        "Update 1 file(s): docs/readme.md"
    );
    assert!(
        commit_message.contains("Scriptum-Trigger: checkpoint"),
        "expected checkpoint trigger trailer, got: {commit_message}"
    );

    let captured = tokio::time::timeout(Duration::from_secs(2), captured_rx)
        .await
        .expect("mock anthropic server should receive request")
        .expect("captured request should be present");
    assert_eq!(captured.x_api_key.as_deref(), Some("sk-ant-local-test"));
    assert_eq!(captured.anthropic_version.as_deref(), Some(ANTHROPIC_API_VERSION));
    assert_eq!(captured.body["messages"][0]["role"], "user");

    server.abort();
    let _ = server.await;
}

async fn run_git_sync_commit(
    repo_path: &Path,
    ai_client: Arc<dyn AiCommitClient>,
    ai_enabled: bool,
    redaction_policy: RedactionPolicy,
    checkpoint_message: &str,
) {
    let git_state = GitState::with_executor_and_ai(
        repo_path.to_path_buf(),
        ProcessCommandExecutor,
        ai_client,
        ai_enabled,
        redaction_policy,
    );
    let state = RpcServerState::default().with_git_state(git_state);
    let request = RpcRequest::new(
        "git.sync",
        Some(json!({
            "action": {
                "commit": {
                    "message": checkpoint_message
                }
            }
        })),
        RequestId::Number(1001),
    );

    let response = dispatch_request(request, &state).await;
    assert!(response.error.is_none(), "git.sync should succeed: {response:?}");
}

async fn spawn_mock_anthropic_server(
    response_status: StatusCode,
    response_body: serde_json::Value,
) -> (String, oneshot::Receiver<CapturedAnthropicCall>, tokio::task::JoinHandle<()>) {
    let (tx, rx) = oneshot::channel::<CapturedAnthropicCall>();
    let sender = Arc::new(Mutex::new(Some(tx)));
    let response_body = Arc::new(response_body);
    let app = Router::new().route(
        "/v1/messages",
        post({
            let sender = Arc::clone(&sender);
            let response_body = Arc::clone(&response_body);
            move |headers: HeaderMap, Json(body): Json<serde_json::Value>| {
                let sender = Arc::clone(&sender);
                let response_body = Arc::clone(&response_body);
                async move {
                    if let Some(tx) =
                        sender.lock().expect("sender lock should not be poisoned").take()
                    {
                        let _ = tx.send(CapturedAnthropicCall {
                            x_api_key: headers
                                .get("x-api-key")
                                .and_then(|value| value.to_str().ok())
                                .map(str::to_string),
                            anthropic_version: headers
                                .get("anthropic-version")
                                .and_then(|value| value.to_str().ok())
                                .map(str::to_string),
                            content_type: headers
                                .get("content-type")
                                .and_then(|value| value.to_str().ok())
                                .map(str::to_string),
                            body,
                        });
                    }
                    (response_status, Json((*response_body).clone()))
                }
            }
        }),
    );

    let listener =
        tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener should bind");
    let address = listener.local_addr().expect("listener should expose address");
    let server =
        tokio::spawn(
            async move { axum::serve(listener, app).await.expect("mock server should run") },
        );

    (format!("http://{address}/v1/messages"), rx, server)
}

fn setup_repo_for_sync(temp: &TempDir) -> PathBuf {
    let repo_path = temp.path().join("repo");
    run_git(temp.path(), &["init", "-b", "main", repo_path.to_str().expect("utf8 repo path")]);
    run_git(&repo_path, &["config", "user.name", "Scriptum Bot"]);
    run_git(&repo_path, &["config", "user.email", "scriptum-bot@example.test"]);
    std::fs::create_dir_all(repo_path.join("docs")).expect("docs directory should be created");
    std::fs::write(repo_path.join("docs/readme.md"), "# Scriptum\n\nInitial content.\n")
        .expect("seed readme should be written");
    run_git(&repo_path, &["add", "."]);
    run_git(&repo_path, &["commit", "-m", "chore: initial commit"]);
    repo_path
}

fn write_repo_edit(repo_path: &Path, content: &str) {
    std::fs::write(repo_path.join("docs/readme.md"), content)
        .expect("updated readme should be written");
}

fn latest_commit_message(repo_path: &Path) -> String {
    run_git_capture(repo_path, &["log", "-1", "--pretty=%B"])
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
