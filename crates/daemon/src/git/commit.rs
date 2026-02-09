// AI-assisted commit message generation.
//
// Calls an LLM (Claude Haiku) with a diff summary to produce concise
// conventional commit messages. Respects the workspace redaction policy:
// - Disabled: no AI calls, returns an error.
// - Redacted: sends sanitized diff content with sensitive values removed.
// - Full: sends the complete diff for richer messages.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;

use super::triggers::{ChangeType, ChangedFile};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// System prompt instructing the LLM to generate conventional commit messages.
pub const SYSTEM_PROMPT: &str =
    "Generate concise git commit (max 72 chars first line). Focus on WHAT and WHY.";

pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-haiku-4-5-20250929";
const DEFAULT_MAX_TOKENS: usize = 200;
const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_API_VERSION: &str = "2023-06-01";

/// Redaction policy for AI commit message generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RedactionPolicy {
    /// AI commit messages disabled entirely.
    Disabled,
    /// Send sanitized diff content with sensitive values redacted.
    #[default]
    Redacted,
    /// Send full diff to AI.
    Full,
}

/// Error from AI commit message generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiCommitError {
    /// AI commit messages are disabled by policy.
    Disabled,
    /// The AI client returned an error.
    ClientError(String),
    /// The AI returned an empty response.
    EmptyResponse,
}

impl Display for AiCommitError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AiCommitError::Disabled => write!(f, "AI commit messages are disabled"),
            AiCommitError::ClientError(msg) => write!(f, "AI client error: {msg}"),
            AiCommitError::EmptyResponse => write!(f, "AI returned an empty response"),
        }
    }
}

impl Error for AiCommitError {}

/// Trait for calling an LLM to generate commit messages.
///
/// In production this calls Claude Haiku via the Anthropic API. Tests inject
/// a mock that returns canned responses.
pub trait AiCommitClient: Send + Sync {
    fn generate(
        &self,
        system: &str,
        user_prompt: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, AiCommitError>> + Send>>;
}

#[derive(Debug, Clone)]
pub struct AnthropicCommitClient {
    http: reqwest::Client,
    api_url: String,
    api_key: Option<String>,
    model: String,
    max_tokens: usize,
}

#[derive(Debug, Serialize)]
struct AnthropicMessageRequest {
    model: String,
    max_tokens: usize,
    system: String,
    messages: Vec<AnthropicMessage>,
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: &'static str,
    content: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicMessageResponse {
    #[serde(default)]
    content: Vec<AnthropicContentBlock>,
}

#[derive(Debug, Deserialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: Option<String>,
}

impl Default for AnthropicCommitClient {
    fn default() -> Self {
        Self::new()
    }
}

impl AnthropicCommitClient {
    pub fn new() -> Self {
        let global_config = crate::config::GlobalConfig::load();
        Self::from_global_config(&global_config)
    }

    pub fn from_global_config(config: &crate::config::GlobalConfig) -> Self {
        let model = config
            .ai
            .model
            .as_deref()
            .and_then(trimmed_non_empty)
            .unwrap_or_else(|| DEFAULT_ANTHROPIC_MODEL.to_string());
        let api_key = if config.ai.enabled {
            resolve_api_key(config)
        } else {
            None
        };

        Self {
            http: reqwest::Client::new(),
            api_url: ANTHROPIC_API_URL.to_string(),
            api_key,
            model,
            max_tokens: DEFAULT_MAX_TOKENS,
        }
    }

    pub fn is_configured(&self) -> bool {
        self.api_key.is_some()
    }
}

impl AiCommitClient for AnthropicCommitClient {
    fn generate(
        &self,
        system: &str,
        user_prompt: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, AiCommitError>> + Send>> {
        let http = self.http.clone();
        let api_url = self.api_url.clone();
        let api_key = self.api_key.clone();
        let model = self.model.clone();
        let system = system.to_string();
        let user_prompt = user_prompt.to_string();
        let max_tokens = self.max_tokens;

        Box::pin(async move {
            let api_key = api_key.ok_or_else(|| {
                AiCommitError::ClientError(
                    "Anthropic API key not configured (set ANTHROPIC_API_KEY or [ai].api_key)"
                        .to_string(),
                )
            })?;

            let request = AnthropicMessageRequest {
                model,
                max_tokens,
                system,
                messages: vec![AnthropicMessage { role: "user", content: user_prompt }],
            };

            let response = http
                .post(api_url)
                .header("x-api-key", api_key)
                .header("anthropic-version", ANTHROPIC_API_VERSION)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .json(&request)
                .send()
                .await
                .map_err(|error| {
                    AiCommitError::ClientError(format!("failed to call Anthropic API: {error}"))
                })?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                let detail = if body.trim().is_empty() {
                    format!("status {status}")
                } else {
                    let trimmed = body.trim();
                    let truncated = if trimmed.len() > 240 {
                        format!("{}...", &trimmed[..240])
                    } else {
                        trimmed.to_string()
                    };
                    format!("status {status}: {truncated}")
                };
                return Err(AiCommitError::ClientError(format!(
                    "Anthropic API returned error ({detail})"
                )));
            }

            let payload: AnthropicMessageResponse = response.json().await.map_err(|error| {
                AiCommitError::ClientError(format!(
                    "failed to decode Anthropic response payload: {error}"
                ))
            })?;

            for block in payload.content {
                if block.kind == "text" {
                    if let Some(text) = block.text.as_deref().and_then(trimmed_non_empty) {
                        return Ok(text);
                    }
                }
            }

            Err(AiCommitError::EmptyResponse)
        })
    }
}

fn resolve_api_key(config: &crate::config::GlobalConfig) -> Option<String> {
    std::env::var("ANTHROPIC_API_KEY")
        .ok()
        .as_deref()
        .and_then(trimmed_non_empty)
        .or_else(|| config.ai.api_key.as_deref().and_then(trimmed_non_empty))
}

fn trimmed_non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Build the user prompt from the diff summary and changed files.
pub fn build_prompt(
    diff_summary: &str,
    changed_files: &[ChangedFile],
    policy: RedactionPolicy,
) -> String {
    let mut prompt = String::new();

    if !changed_files.is_empty() {
        prompt.push_str("Changed files:\n");
        for file in changed_files {
            let marker = match file.change_type {
                ChangeType::Added => "A",
                ChangeType::Modified => "M",
                ChangeType::Deleted => "D",
            };
            prompt.push_str(&format!("  {marker} {}\n", file.path));
        }
        prompt.push('\n');
    }

    match policy {
        RedactionPolicy::Full => {
            prompt.push_str("Diff:\n");
            prompt.push_str(diff_summary);
        }
        RedactionPolicy::Redacted => {
            prompt.push_str("Diff (redacted):\n");
            prompt.push_str("(Sensitive values redacted by policy.)\n");
            prompt.push_str(&redact_sensitive_content(diff_summary));
        }
        RedactionPolicy::Disabled => {}
    }

    prompt
}

struct RedactionRule {
    pattern: Regex,
    replacement: &'static str,
}

fn sensitive_patterns() -> &'static [RedactionRule] {
    static PATTERNS: OnceLock<Vec<RedactionRule>> = OnceLock::new();
    PATTERNS
        .get_or_init(|| {
            vec![
                // key = value style assignments.
                RedactionRule {
                    pattern: Regex::new(
                        r#"(?im)\b(api[_-]?key|secret|token|password|passwd|credential|client[_-]?secret|access[_-]?key|private[_-]?key)\b(\s*[:=]\s*)(['"]?)[^'"\s]+(['"]?)"#,
                    )
                    .expect("assignment redaction pattern should compile"),
                    replacement: "${1}${2}${3}[REDACTED]${4}",
                },
                // Authorization bearer token headers.
                RedactionRule {
                    pattern: Regex::new(r"(?im)\b(authorization\s*:\s*bearer\s+)\S+")
                        .expect("authorization header redaction pattern should compile"),
                    replacement: "${1}[REDACTED]",
                },
                // URI credentials like scheme://user:password@host.
                RedactionRule {
                    pattern: Regex::new(r"(?i)\b([a-z][a-z0-9+.-]*://[^/\s:@]+:)([^@\s/]+)(@)")
                        .expect("url credential redaction pattern should compile"),
                    replacement: "${1}[REDACTED]${3}",
                },
                // AWS-style access keys.
                RedactionRule {
                    pattern: Regex::new(r"(?i)\b(?:AKIA|ASIA)[A-Z0-9]{16}\b")
                        .expect("aws key redaction pattern should compile"),
                    replacement: "[REDACTED]",
                },
                // GitHub access tokens.
                RedactionRule {
                    pattern: Regex::new(r"(?i)\bgh[pousr]_[A-Za-z0-9]{30,}\b")
                        .expect("github token redaction pattern should compile"),
                    replacement: "[REDACTED]",
                },
                // Common API key prefixes.
                RedactionRule {
                    pattern: Regex::new(r"(?i)\bsk-(?:live|test)-[A-Za-z0-9]{16,}\b")
                        .expect("api key prefix redaction pattern should compile"),
                    replacement: "[REDACTED]",
                },
                // JWT-like bearer tokens.
                RedactionRule {
                    pattern: Regex::new(
                        r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b",
                    )
                    .expect("jwt redaction pattern should compile"),
                    replacement: "[REDACTED]",
                },
                // PEM private keys.
                RedactionRule {
                    pattern: Regex::new(
                        r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----",
                    )
                    .expect("pem redaction pattern should compile"),
                    replacement: "-----BEGIN PRIVATE KEY-----\n[REDACTED]\n-----END PRIVATE KEY-----",
                },
            ]
        })
        .as_slice()
}

fn redact_sensitive_content(diff_summary: &str) -> String {
    let mut redacted = diff_summary.to_string();

    for rule in sensitive_patterns() {
        redacted = rule.pattern.replace_all(&redacted, rule.replacement).into_owned();
    }

    redacted
}

/// Generate an AI-assisted commit message.
///
/// Returns `Err(AiCommitError::Disabled)` when the policy forbids AI calls.
pub async fn generate_ai_commit_message(
    client: &dyn AiCommitClient,
    diff_summary: &str,
    changed_files: &[ChangedFile],
    policy: RedactionPolicy,
) -> Result<String, AiCommitError> {
    if policy == RedactionPolicy::Disabled {
        return Err(AiCommitError::Disabled);
    }

    let user_prompt = build_prompt(diff_summary, changed_files, policy);
    let response = client.generate(SYSTEM_PROMPT, &user_prompt).await?;

    let trimmed = response.trim().to_string();
    if trimmed.is_empty() {
        return Err(AiCommitError::EmptyResponse);
    }

    Ok(enforce_first_line_limit(&trimmed, 72))
}

/// Generate a deterministic fallback commit message when AI generation fails.
///
/// Format: `Update {n} file(s): {paths}`
pub fn fallback_commit_message(changed_files: &[ChangedFile]) -> String {
    let mut paths = changed_files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>();
    paths.sort_unstable();
    paths.dedup();

    let path_list = if paths.is_empty() { String::from("(none)") } else { paths.join(", ") };
    format!("Update {} file(s): {path_list}", paths.len())
}

/// Generate a commit message with deterministic fallback.
///
/// Returns AI output when available; otherwise falls back to a stable
/// path-based message to keep commits non-empty and predictable.
pub async fn generate_commit_message_with_fallback(
    client: &dyn AiCommitClient,
    diff_summary: &str,
    changed_files: &[ChangedFile],
    policy: RedactionPolicy,
) -> String {
    match generate_ai_commit_message(client, diff_summary, changed_files, policy).await {
        Ok(message) => message,
        Err(_) => fallback_commit_message(changed_files),
    }
}

/// Truncate the first line of a commit message to `max_len` characters,
/// breaking at a word boundary when possible.
fn enforce_first_line_limit(message: &str, max_len: usize) -> String {
    let mut lines = message.lines();
    let first_line = match lines.next() {
        Some(line) => line,
        None => return message.to_string(),
    };

    if first_line.len() <= max_len {
        return message.to_string();
    }

    let truncated = &first_line[..max_len];
    let truncated = match truncated.rfind(' ') {
        Some(pos) if pos > max_len / 2 => &truncated[..pos],
        _ => truncated,
    };

    let rest: Vec<&str> = lines.collect();
    if rest.is_empty() {
        truncated.to_string()
    } else {
        format!("{truncated}\n{}", rest.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex, OnceLock};

    use axum::{
        http::{HeaderMap, StatusCode},
        routing::post,
        Json, Router,
    };
    use serde_json::json;
    use tokio::sync::oneshot;

    use super::*;

    struct MockClient {
        response: Mutex<Option<Result<String, AiCommitError>>>,
        captured_system: Mutex<Option<String>>,
        captured_prompt: Mutex<Option<String>>,
    }

    impl MockClient {
        fn ok(message: &str) -> Self {
            Self {
                response: Mutex::new(Some(Ok(message.to_string()))),
                captured_system: Mutex::new(None),
                captured_prompt: Mutex::new(None),
            }
        }

        fn err(error: AiCommitError) -> Self {
            Self {
                response: Mutex::new(Some(Err(error))),
                captured_system: Mutex::new(None),
                captured_prompt: Mutex::new(None),
            }
        }

        fn captured_system(&self) -> Option<String> {
            self.captured_system.lock().unwrap().clone()
        }

        fn captured_prompt(&self) -> Option<String> {
            self.captured_prompt.lock().unwrap().clone()
        }
    }

    impl AiCommitClient for MockClient {
        fn generate(
            &self,
            system: &str,
            user_prompt: &str,
        ) -> Pin<Box<dyn Future<Output = Result<String, AiCommitError>> + Send>> {
            *self.captured_system.lock().unwrap() = Some(system.to_string());
            *self.captured_prompt.lock().unwrap() = Some(user_prompt.to_string());
            let result =
                self.response.lock().unwrap().take().expect("mock response consumed twice");
            Box::pin(async move { result })
        }
    }

    fn test_files() -> Vec<ChangedFile> {
        vec![
            ChangedFile {
                path: "docs/api.md".into(),
                doc_id: None,
                change_type: ChangeType::Modified,
            },
            ChangedFile {
                path: "docs/new-guide.md".into(),
                doc_id: None,
                change_type: ChangeType::Added,
            },
        ]
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn global_config_with_api(api_key: Option<&str>, enabled: bool) -> crate::config::GlobalConfig {
        crate::config::GlobalConfig {
            ai: crate::config::AiConfig {
                api_key: api_key.map(str::to_string),
                model: None,
                enabled,
            },
            ..crate::config::GlobalConfig::default()
        }
    }

    #[derive(Debug)]
    struct CapturedAnthropicCall {
        x_api_key: Option<String>,
        anthropic_version: Option<String>,
        content_type: Option<String>,
        body: serde_json::Value,
    }

    // ── AnthropicCommitClient ────────────────────────────────────────

    #[test]
    fn resolve_api_key_prefers_environment_variable() {
        let _guard = env_lock().lock().expect("env lock should be acquirable");
        std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-env");
        let config = global_config_with_api(Some("sk-ant-config"), true);
        assert_eq!(resolve_api_key(&config).as_deref(), Some("sk-ant-env"));
        std::env::remove_var("ANTHROPIC_API_KEY");
    }

    #[test]
    fn resolve_api_key_falls_back_to_global_config_when_env_missing() {
        let _guard = env_lock().lock().expect("env lock should be acquirable");
        std::env::remove_var("ANTHROPIC_API_KEY");
        let config = global_config_with_api(Some("sk-ant-config"), true);
        assert_eq!(resolve_api_key(&config).as_deref(), Some("sk-ant-config"));
    }

    #[test]
    fn anthropic_client_respects_global_ai_enabled_flag() {
        let _guard = env_lock().lock().expect("env lock should be acquirable");
        std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-env");
        let config = global_config_with_api(None, false);
        let client = AnthropicCommitClient::from_global_config(&config);
        assert!(!client.is_configured(), "ai should be disabled when global ai.enabled is false");
        std::env::remove_var("ANTHROPIC_API_KEY");
    }

    #[tokio::test]
    async fn anthropic_client_posts_expected_request_format() {
        let (tx, rx) = oneshot::channel::<CapturedAnthropicCall>();
        let sender = Arc::new(Mutex::new(Some(tx)));

        let app = Router::new().route(
            "/v1/messages",
            post({
                let sender = Arc::clone(&sender);
                move |headers: HeaderMap, Json(body): Json<serde_json::Value>| {
                    let sender = Arc::clone(&sender);
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

                        (
                            StatusCode::OK,
                            Json(json!({
                                "content": [
                                    { "type": "text", "text": "docs: refresh auth guide" }
                                ]
                            })),
                        )
                    }
                }
            }),
        );

        let listener =
            tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener should bind");
        let address = listener.local_addr().expect("listener should expose address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("mock anthropic server should run");
        });

        let client = AnthropicCommitClient {
            http: reqwest::Client::new(),
            api_url: format!("http://{address}/v1/messages"),
            api_key: Some("sk-ant-local-test".to_string()),
            model: DEFAULT_ANTHROPIC_MODEL.to_string(),
            max_tokens: DEFAULT_MAX_TOKENS,
        };
        let prompt =
            build_prompt("@@ -1 +1 @@\n+docs update", &test_files(), RedactionPolicy::Full);
        let message =
            client.generate(SYSTEM_PROMPT, &prompt).await.expect("anthropic call should succeed");

        assert_eq!(message, "docs: refresh auth guide");

        let captured = rx.await.expect("request should be captured");
        assert_eq!(captured.x_api_key.as_deref(), Some("sk-ant-local-test"));
        assert_eq!(captured.anthropic_version.as_deref(), Some(ANTHROPIC_API_VERSION));
        assert!(captured
            .content_type
            .as_deref()
            .is_some_and(|value| value.contains("application/json")));
        assert_eq!(captured.body["model"], DEFAULT_ANTHROPIC_MODEL);
        assert_eq!(captured.body["max_tokens"], DEFAULT_MAX_TOKENS);
        assert_eq!(captured.body["system"], SYSTEM_PROMPT);
        assert_eq!(captured.body["messages"][0]["role"], "user");
        assert!(captured.body["messages"][0]["content"]
            .as_str()
            .is_some_and(|value| value.contains("Changed files:")));

        server.abort();
        let _ = server.await;
    }

    #[tokio::test]
    async fn anthropic_client_failure_uses_structured_fallback() {
        let client = AnthropicCommitClient {
            http: reqwest::Client::new(),
            api_url: "http://127.0.0.1:1/v1/messages".to_string(),
            api_key: Some("sk-ant-local-test".to_string()),
            model: DEFAULT_ANTHROPIC_MODEL.to_string(),
            max_tokens: DEFAULT_MAX_TOKENS,
        };
        let files = vec![
            ChangedFile {
                path: "docs/b.md".into(),
                doc_id: None,
                change_type: ChangeType::Modified,
            },
            ChangedFile { path: "docs/a.md".into(), doc_id: None, change_type: ChangeType::Added },
        ];

        let message = generate_commit_message_with_fallback(
            &client,
            "@@ -1 +1 @@\n+new content",
            &files,
            RedactionPolicy::Full,
        )
        .await;

        assert_eq!(message, "Update 2 file(s): docs/a.md, docs/b.md");
    }

    // ── generate_ai_commit_message ────────────────────────────────────

    #[tokio::test]
    async fn generates_message_with_full_policy() {
        let client = MockClient::ok("docs(api): update authentication section");
        let files = test_files();
        let diff = "@@ -1,3 +1,5 @@\n+Bearer tokens now required.\n";

        let msg =
            generate_ai_commit_message(&client, diff, &files, RedactionPolicy::Full).await.unwrap();

        assert_eq!(msg, "docs(api): update authentication section");

        // Verify the system prompt was sent.
        assert_eq!(client.captured_system().unwrap(), SYSTEM_PROMPT);

        // Verify the user prompt includes the diff.
        let prompt = client.captured_prompt().unwrap();
        assert!(prompt.contains("Diff:"), "full policy should include diff");
        assert!(prompt.contains("Bearer tokens"), "diff content should be present");
        assert!(prompt.contains("M docs/api.md"), "file list should be present");
        assert!(prompt.contains("A docs/new-guide.md"), "added file should be listed");
    }

    #[tokio::test]
    async fn generates_message_with_redacted_policy() {
        let client = MockClient::ok("docs: update documentation");
        let files = test_files();
        let diff = "token=top-secret-token";

        let msg = generate_ai_commit_message(&client, diff, &files, RedactionPolicy::Redacted)
            .await
            .unwrap();

        assert_eq!(msg, "docs: update documentation");

        let prompt = client.captured_prompt().unwrap();
        assert!(prompt.contains("redacted by policy"), "should mention redaction");
        assert!(!prompt.contains("top-secret-token"), "sensitive token must not leak");
        assert!(prompt.contains("token=[REDACTED]"), "token should be masked");
        assert!(prompt.contains("M docs/api.md"), "file names should still be present");
    }

    #[tokio::test]
    async fn disabled_policy_returns_error_without_calling_client() {
        let client = MockClient::ok("should not be used");

        let result =
            generate_ai_commit_message(&client, "diff", &[], RedactionPolicy::Disabled).await;

        assert_eq!(result.unwrap_err(), AiCommitError::Disabled);
        // Client was never called.
        assert!(client.captured_system().is_none());
    }

    #[tokio::test]
    async fn empty_response_returns_error() {
        let client = MockClient::ok("   \n  ");

        let result = generate_ai_commit_message(&client, "diff", &[], RedactionPolicy::Full).await;

        assert_eq!(result.unwrap_err(), AiCommitError::EmptyResponse);
    }

    #[tokio::test]
    async fn client_error_propagates() {
        let client = MockClient::err(AiCommitError::ClientError("timeout".into()));

        let result = generate_ai_commit_message(&client, "diff", &[], RedactionPolicy::Full).await;

        assert_eq!(result.unwrap_err(), AiCommitError::ClientError("timeout".into()));
    }

    #[tokio::test]
    async fn trims_whitespace_from_response() {
        let client = MockClient::ok("\n  feat: add new feature  \n\n");

        let msg =
            generate_ai_commit_message(&client, "diff", &[], RedactionPolicy::Full).await.unwrap();

        assert_eq!(msg, "feat: add new feature");
    }

    #[tokio::test]
    async fn fallback_wrapper_uses_ai_message_when_available() {
        let client = MockClient::ok("docs: refresh README");
        let files = test_files();

        let message =
            generate_commit_message_with_fallback(&client, "diff", &files, RedactionPolicy::Full)
                .await;

        assert_eq!(message, "docs: refresh README");
    }

    #[tokio::test]
    async fn fallback_wrapper_uses_structured_fallback_on_ai_error() {
        let client = MockClient::err(AiCommitError::ClientError("timeout".into()));
        let files = vec![
            ChangedFile {
                path: "docs/b.md".into(),
                doc_id: None,
                change_type: ChangeType::Modified,
            },
            ChangedFile { path: "docs/a.md".into(), doc_id: None, change_type: ChangeType::Added },
            ChangedFile {
                path: "docs/a.md".into(),
                doc_id: None,
                change_type: ChangeType::Deleted,
            },
        ];

        let message =
            generate_commit_message_with_fallback(&client, "diff", &files, RedactionPolicy::Full)
                .await;

        assert_eq!(message, "Update 2 file(s): docs/a.md, docs/b.md");
    }

    // ── enforce_first_line_limit ──────────────────────────────────────

    #[test]
    fn short_first_line_unchanged() {
        let msg = "fix: correct typo in readme";
        assert_eq!(enforce_first_line_limit(msg, 72), msg);
    }

    #[test]
    fn long_first_line_truncated_at_word_boundary() {
        let msg = "refactor(authentication): migrate from session-based auth to JWT bearer tokens across all API endpoints";
        let result = enforce_first_line_limit(msg, 72);
        assert!(result.len() <= 72, "first line should be at most 72 chars: {}", result.len());
        assert!(result.starts_with("refactor(authentication):"));
        assert!(!result.contains("endpoints"));
    }

    #[test]
    fn long_first_line_preserves_body() {
        let msg = "refactor(authentication): migrate from session-based auth to JWT bearer tokens across all API endpoints\n\nThis is a breaking change.";
        let result = enforce_first_line_limit(msg, 72);
        let lines: Vec<&str> = result.lines().collect();
        assert!(lines[0].len() <= 72);
        assert!(result.contains("This is a breaking change."));
    }

    #[test]
    fn exactly_72_chars_unchanged() {
        let msg = "a]".to_string() + &"b".repeat(70); // exactly 72 chars
        assert_eq!(msg.len(), 72);
        assert_eq!(enforce_first_line_limit(&msg, 72), msg);
    }

    // ── build_prompt ──────────────────────────────────────────────────

    #[test]
    fn full_prompt_includes_files_and_diff() {
        let files = test_files();
        let prompt = build_prompt("+ new line\n", &files, RedactionPolicy::Full);

        assert!(prompt.contains("M docs/api.md"));
        assert!(prompt.contains("A docs/new-guide.md"));
        assert!(prompt.contains("Diff:"));
        assert!(prompt.contains("+ new line"));
    }

    #[test]
    fn redacted_prompt_includes_sanitized_diff_content() {
        let files = test_files();
        let prompt = build_prompt(
            "api_key = \"sk-live-1234567890abcdef\"\npassword=supersecret",
            &files,
            RedactionPolicy::Redacted,
        );

        assert!(prompt.contains("M docs/api.md"));
        assert!(prompt.contains("Diff (redacted):"));
        assert!(prompt.contains("api_key = \"[REDACTED]\""));
        assert!(prompt.contains("password=[REDACTED]"));
        assert!(!prompt.contains("sk-live-1234567890abcdef"));
        assert!(!prompt.contains("supersecret"));
    }

    #[test]
    fn empty_files_omits_file_section() {
        let prompt = build_prompt("diff", &[], RedactionPolicy::Full);
        assert!(!prompt.contains("Changed files:"));
        assert!(prompt.contains("Diff:"));
    }

    #[test]
    fn deleted_files_marked_with_d() {
        let files = vec![ChangedFile {
            path: "old.md".into(),
            doc_id: None,
            change_type: ChangeType::Deleted,
        }];
        let prompt = build_prompt("", &files, RedactionPolicy::Full);
        assert!(prompt.contains("D old.md"));
    }

    #[test]
    fn fallback_message_handles_empty_file_list() {
        let message = fallback_commit_message(&[]);
        assert_eq!(message, "Update 0 file(s): (none)");
    }

    #[test]
    fn fallback_message_is_sorted_and_deduplicated() {
        let files = vec![
            ChangedFile {
                path: "docs/z.md".into(),
                doc_id: None,
                change_type: ChangeType::Modified,
            },
            ChangedFile { path: "docs/a.md".into(), doc_id: None, change_type: ChangeType::Added },
            ChangedFile {
                path: "docs/z.md".into(),
                doc_id: None,
                change_type: ChangeType::Deleted,
            },
        ];
        let message = fallback_commit_message(&files);
        assert_eq!(message, "Update 2 file(s): docs/a.md, docs/z.md");
    }

    #[test]
    fn redacts_tokens_and_private_keys_from_diff() {
        let diff = "\
+Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.abcdefghi123456789.xyz987654321qwe\n\
+Authorization: Bearer not-a-jwt-secret-token\n\
+aws_access_key_id=AKIAABCDEFGHIJKLMNOP\n\
+github_pat=ghp_abcdefghijklmnopqrstuvwxyzABCDEFGH\n\
+github_actions_token=ghs_abcdefghijklmnopqrstuvwxyzABCDEFGH\n\
+DATABASE_URL=postgres://alice:supersecret@localhost:5432/scriptum\n\
+-----BEGIN PRIVATE KEY-----\n\
+MIIEvQIBADANBgkqhkiG9w0BAQEFAASC...\n\
+-----END PRIVATE KEY-----\n";

        let redacted = redact_sensitive_content(diff);
        assert!(!redacted.contains("eyJhbGciOiJIUzI1Ni"));
        assert!(!redacted.contains("not-a-jwt-secret-token"));
        assert!(!redacted.contains("AKIAABCDEFGHIJKLMNOP"));
        assert!(!redacted.contains("ghp_abcdefghijklmnopqrstuvwxyzABCDEFGH"));
        assert!(!redacted.contains("ghs_abcdefghijklmnopqrstuvwxyzABCDEFGH"));
        assert!(!redacted.contains("supersecret"));
        assert!(!redacted.contains("MIIEvQIBADANBgkqhkiG9w0BAQEFAASC"));
        assert!(redacted.contains("[REDACTED]"));
        assert!(redacted.contains("Authorization: Bearer [REDACTED]"));
        assert!(redacted.contains("postgres://alice:[REDACTED]@localhost:5432/scriptum"));
        assert!(redacted.contains("BEGIN PRIVATE KEY"));
    }
}
