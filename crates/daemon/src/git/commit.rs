// AI-assisted commit message generation.
//
// Calls an LLM (Claude Haiku) with a diff summary to produce concise
// conventional commit messages. Respects the workspace redaction policy:
// - Disabled: no AI calls, returns an error.
// - Redacted: sends file names and change types only (no diff content).
// - Full: sends the complete diff for richer messages.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;

use super::triggers::{ChangeType, ChangedFile};

/// System prompt instructing the LLM to generate conventional commit messages.
pub const SYSTEM_PROMPT: &str = "\
You are a commit message generator. Write a single conventional commit message.\n\
Rules:\n\
- First line: imperative mood, max 72 characters, format: type(scope): description\n\
- Types: feat, fix, docs, refactor, test, chore, style, perf, ci\n\
- Scope is optional; if used, derive from the primary file or module changed\n\
- If a body is needed, add a blank line then a concise explanation (max 3 lines)\n\
- Do not include file lists, diff details, or attribution in the message\n\
- Output ONLY the commit message, nothing else";

/// Redaction policy for AI commit message generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RedactionPolicy {
    /// AI commit messages disabled entirely.
    Disabled,
    /// Send file names and change types but not diff content.
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
            prompt.push_str(
                "(Diff content redacted by policy. Generate message from file names only.)\n",
            );
        }
        RedactionPolicy::Disabled => {}
    }

    prompt
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
    use std::sync::Mutex;

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

        let msg =
            generate_ai_commit_message(&client, "secret diff", &files, RedactionPolicy::Redacted)
                .await
                .unwrap();

        assert_eq!(msg, "docs: update documentation");

        let prompt = client.captured_prompt().unwrap();
        assert!(prompt.contains("redacted by policy"), "should mention redaction");
        assert!(!prompt.contains("secret diff"), "diff content must not leak");
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
    fn redacted_prompt_excludes_diff_content() {
        let files = test_files();
        let prompt = build_prompt("secret content", &files, RedactionPolicy::Redacted);

        assert!(prompt.contains("M docs/api.md"));
        assert!(prompt.contains("redacted by policy"));
        assert!(!prompt.contains("secret content"));
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
}
