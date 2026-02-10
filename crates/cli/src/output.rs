// Output format auto-detection for the CLI.
//
// TTY → human-readable text. Piped/redirected → structured JSON.
// `--json` flag forces JSON output regardless of terminal.

use crate::client::daemon_unavailable_exit_code;

use serde::Serialize;
use std::io::{self, IsTerminal, Write};

const ANSI_RED: &str = "\x1b[31m";
const ANSI_YELLOW: &str = "\x1b[33m";
const ANSI_RESET: &str = "\x1b[0m";

/// Output format for CLI commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable text (tables, colors, etc.).
    Human,
    /// Machine-readable JSON (one object per response).
    Json,
}

impl OutputFormat {
    /// Auto-detect format: JSON if `--json` was passed or stdout is not a TTY.
    pub fn detect(json_flag: bool) -> Self {
        if json_flag {
            return Self::Json;
        }
        Self::detect_from_terminal(io::stdout().is_terminal())
    }

    /// Testable variant that takes an explicit `is_tty` flag.
    pub fn detect_from_terminal(is_tty: bool) -> Self {
        if is_tty {
            Self::Human
        } else {
            Self::Json
        }
    }
}

/// Write a value to stdout in the selected format.
///
/// - `Human`: calls `human_fn` to produce a human-readable string.
/// - `Json`: serializes `value` as JSON.
pub fn print_output<T, F>(format: OutputFormat, value: &T, human_fn: F) -> io::Result<()>
where
    T: Serialize,
    F: FnOnce(&T) -> String,
{
    let mut out = io::stdout().lock();
    match format {
        OutputFormat::Human => {
            writeln!(out, "{}", human_fn(value))
        }
        OutputFormat::Json => {
            serde_json::to_writer(&mut out, value).map_err(io::Error::other)?;
            writeln!(out)
        }
    }
}

/// Write a value to a provided writer (useful for testing).
pub fn write_output<W, T, F>(
    writer: &mut W,
    format: OutputFormat,
    value: &T,
    human_fn: F,
) -> io::Result<()>
where
    W: Write,
    T: Serialize,
    F: FnOnce(&T) -> String,
{
    match format {
        OutputFormat::Human => {
            writeln!(writer, "{}", human_fn(value))
        }
        OutputFormat::Json => {
            serde_json::to_writer(&mut *writer, value).map_err(io::Error::other)?;
            writeln!(writer)
        }
    }
}

/// Write an error to stderr in the selected format.
pub fn print_error(format: OutputFormat, code: &str, message: &str) {
    let mut err = io::stderr().lock();
    match format {
        OutputFormat::Human => {
            let line =
                render_human_stderr_line("error", message, io::stderr().is_terminal(), ANSI_RED);
            let _ = writeln!(err, "{line}");
        }
        OutputFormat::Json => {
            let obj = serde_json::json!({
                "error": {
                    "code": code,
                    "message": message,
                }
            });
            let _ = serde_json::to_writer(&mut err, &obj);
            let _ = writeln!(err);
        }
    }
}

/// Write a warning to stderr in the selected format.
pub fn print_warning(format: OutputFormat, code: &str, message: &str) {
    let mut err = io::stderr().lock();
    match format {
        OutputFormat::Human => {
            let line = render_human_stderr_line(
                "warning",
                message,
                io::stderr().is_terminal(),
                ANSI_YELLOW,
            );
            let _ = writeln!(err, "{line}");
        }
        OutputFormat::Json => {
            let obj = serde_json::json!({
                "warning": {
                    "code": code,
                    "message": message,
                }
            });
            let _ = serde_json::to_writer(&mut err, &obj);
            let _ = writeln!(err);
        }
    }
}

/// Print a mapped, actionable error for a command failure.
pub fn print_anyhow_error(format: OutputFormat, error: &anyhow::Error) {
    let (code, message) = actionable_error(error);
    print_error(format, code, &message);
}

fn actionable_error(error: &anyhow::Error) -> (&'static str, String) {
    let message = format!("{error:#}");
    let lower = message.to_ascii_lowercase();

    if daemon_unavailable_exit_code(error).is_some()
        || (lower.contains("daemon")
            && lower.contains("socket")
            && (lower.contains("connection refused")
                || lower.contains("not found")
                || lower.contains("failed to connect")))
    {
        return (
            "DAEMON_NOT_RUNNING",
            "Daemon is not running. Start it with: scriptumd (or it auto-starts with most commands)"
                .to_string(),
        );
    }

    if lower.contains("timed out") {
        return (
            "NETWORK_TIMEOUT",
            "Could not reach daemon. Check if scriptumd is running: ps aux | grep scriptumd"
                .to_string(),
        );
    }

    if lower.contains("workspace") && lower.contains("not found") {
        let cwd = std::env::current_dir()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| ".".to_string());
        return ("WORKSPACE_NOT_FOUND", format!("No workspace found at {cwd}. Run: scriptum init"));
    }

    if (lower.contains("document") && lower.contains("not found"))
        || (lower.contains("doc") && lower.contains("not found"))
    {
        let doc_name = extract_document_name(&message).unwrap_or_else(|| "<name>".to_string());
        return (
            "DOCUMENT_NOT_FOUND",
            format!("Document {doc_name} not found. Run: scriptum ls to see available documents"),
        );
    }

    if lower.contains("permission denied") || (lower.contains("requires") && lower.contains("role"))
    {
        let role =
            extract_json_string_field(&message, "role").unwrap_or_else(|| "unknown".to_string());
        let required_role = extract_json_string_field(&message, "required_role")
            .unwrap_or_else(|| "required role".to_string());
        return (
            "PERMISSION_DENIED",
            format!("Permission denied. Your role is {role}, this requires {required_role}."),
        );
    }

    if lower.contains("auth")
        || lower.contains("unauthorized")
        || lower.contains("forbidden")
        || lower.contains("token")
    {
        return (
            "AUTH_FAILURE",
            "Authentication failed. Check relay connection and run: scriptum setup".to_string(),
        );
    }

    ("RPC_ERROR", message)
}

fn render_human_stderr_line(label: &str, message: &str, is_tty: bool, color: &str) -> String {
    if is_tty {
        format!("{color}{label}:{ANSI_RESET} {message}")
    } else {
        format!("{label}: {message}")
    }
}

fn extract_json_string_field(message: &str, field: &str) -> Option<String> {
    let key = format!("\"{field}\":\"");
    let start = message.find(&key)? + key.len();
    let tail = &message[start..];
    let end = tail.find('"')?;
    Some(tail[..end].to_string())
}

fn extract_document_name(message: &str) -> Option<String> {
    let mut parts = message.split('`');
    let _before = parts.next()?;
    let candidate = parts.next()?.trim();
    if candidate.is_empty() {
        None
    } else {
        Some(candidate.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_tty_returns_human() {
        assert_eq!(OutputFormat::detect_from_terminal(true), OutputFormat::Human);
    }

    #[test]
    fn detect_pipe_returns_json() {
        assert_eq!(OutputFormat::detect_from_terminal(false), OutputFormat::Json);
    }

    #[test]
    fn detect_json_flag_overrides_tty() {
        // --json should force JSON even when detect() would normally check
        // the real stdout. We test the flag logic directly.
        assert_eq!(OutputFormat::detect(true), OutputFormat::Json);
    }

    #[test]
    fn write_output_human_format() {
        #[derive(Serialize)]
        struct Info {
            name: String,
        }
        let info = Info { name: "alice".into() };
        let mut buf = Vec::new();
        write_output(&mut buf, OutputFormat::Human, &info, |i| format!("Name: {}", i.name))
            .unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "Name: alice\n");
    }

    #[test]
    fn write_output_json_format() {
        #[derive(Serialize)]
        struct Info {
            name: String,
            count: u32,
        }
        let info = Info { name: "bob".into(), count: 42 };
        let mut buf = Vec::new();
        write_output(&mut buf, OutputFormat::Json, &info, |_| {
            unreachable!("human_fn should not be called in JSON mode")
        })
        .unwrap();
        let output = String::from_utf8(buf).unwrap();
        // Should be valid JSON followed by a newline.
        assert!(output.ends_with('\n'));
        let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        assert_eq!(parsed["name"], "bob");
        assert_eq!(parsed["count"], 42);
    }

    #[test]
    fn write_output_json_does_not_call_human_fn() {
        #[derive(Serialize)]
        struct Empty {}
        let mut buf = Vec::new();
        write_output(&mut buf, OutputFormat::Json, &Empty {}, |_| {
            panic!("should not be called");
        })
        .unwrap();
        // Just verify it didn't panic.
        assert!(!buf.is_empty());
    }

    #[test]
    fn print_error_human_format() {
        // We can't capture stderr easily in a unit test, but we can verify
        // the function doesn't panic.
        print_error(OutputFormat::Human, "TEST_ERR", "something broke");
    }

    #[test]
    fn print_error_json_format() {
        print_error(OutputFormat::Json, "TEST_ERR", "something broke");
    }

    #[test]
    fn print_warning_json_format() {
        print_warning(OutputFormat::Json, "WARN", "heads up");
    }

    #[test]
    fn format_equality() {
        assert_eq!(OutputFormat::Human, OutputFormat::Human);
        assert_eq!(OutputFormat::Json, OutputFormat::Json);
        assert_ne!(OutputFormat::Human, OutputFormat::Json);
    }

    #[test]
    fn write_output_empty_string_human() {
        #[derive(Serialize)]
        struct Empty {}
        let mut buf = Vec::new();
        write_output(&mut buf, OutputFormat::Human, &Empty {}, |_| String::new()).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "\n");
    }

    #[test]
    fn write_output_nested_json() {
        #[derive(Serialize)]
        struct Outer {
            inner: Inner,
        }
        #[derive(Serialize)]
        struct Inner {
            value: i32,
        }
        let data = Outer { inner: Inner { value: 99 } };
        let mut buf = Vec::new();
        write_output(&mut buf, OutputFormat::Json, &data, |_| String::new()).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        assert_eq!(parsed["inner"]["value"], 99);
    }

    #[test]
    fn render_human_error_uses_color_for_tty() {
        let line = render_human_stderr_line("error", "boom", true, ANSI_RED);
        assert!(line.contains(ANSI_RED));
        assert!(line.contains(ANSI_RESET));
        assert!(line.contains("boom"));
    }

    #[test]
    fn render_human_warning_without_tty_is_plain() {
        let line = render_human_stderr_line("warning", "careful", false, ANSI_YELLOW);
        assert_eq!(line, "warning: careful");
    }

    #[test]
    fn actionable_error_daemon_not_running_message() {
        let err = anyhow::anyhow!("failed to connect to daemon socket: connection refused");
        let (code, message) = actionable_error(&err);
        assert_eq!(code, "DAEMON_NOT_RUNNING");
        assert!(message.contains("Daemon is not running"));
        assert!(message.contains("scriptumd"));
    }

    #[test]
    fn actionable_error_workspace_not_found_message() {
        let err = anyhow::anyhow!("workspace 00000000-0000-0000-0000-000000000000 not found");
        let (code, message) = actionable_error(&err);
        assert_eq!(code, "WORKSPACE_NOT_FOUND");
        assert!(message.contains("Run: scriptum init"));
    }

    #[test]
    fn actionable_error_document_not_found_message() {
        let err = anyhow::anyhow!("doc `docs/missing.md` not found");
        let (code, message) = actionable_error(&err);
        assert_eq!(code, "DOCUMENT_NOT_FOUND");
        assert!(message.contains("docs/missing.md"));
        assert!(message.contains("scriptum ls"));
    }

    #[test]
    fn actionable_error_auth_failure_message() {
        let err = anyhow::anyhow!("AUTH_FORBIDDEN");
        let (code, message) = actionable_error(&err);
        assert_eq!(code, "AUTH_FAILURE");
        assert!(message.contains("scriptum setup"));
    }

    #[test]
    fn actionable_error_timeout_message() {
        let err = anyhow::anyhow!("timed out waiting for json-rpc response");
        let (code, message) = actionable_error(&err);
        assert_eq!(code, "NETWORK_TIMEOUT");
        assert!(message.contains("ps aux | grep scriptumd"));
    }

    #[test]
    fn actionable_error_permission_denied_with_roles() {
        let err = anyhow::anyhow!(
            "permission denied (data: {{\"role\":\"viewer\",\"required_role\":\"owner\"}})"
        );
        let (code, message) = actionable_error(&err);
        assert_eq!(code, "PERMISSION_DENIED");
        assert!(message.contains("role is viewer"));
        assert!(message.contains("requires owner"));
    }
}
