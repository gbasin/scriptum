// Output format auto-detection for the CLI.
//
// TTY → human-readable text. Piped/redirected → structured JSON.
// `--json` flag forces JSON output regardless of terminal.

use serde::Serialize;
use std::io::{self, IsTerminal, Write};

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
pub fn print_output<T, F>(
    format: OutputFormat,
    value: &T,
    human_fn: F,
) -> io::Result<()>
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
            serde_json::to_writer(&mut out, value)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
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
            serde_json::to_writer(&mut *writer, value)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            writeln!(writer)
        }
    }
}

/// Write an error to stderr in the selected format.
pub fn print_error(format: OutputFormat, code: &str, message: &str) {
    let mut err = io::stderr().lock();
    match format {
        OutputFormat::Human => {
            let _ = writeln!(err, "error: {message}");
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
        write_output(&mut buf, OutputFormat::Human, &info, |i| {
            format!("Name: {}", i.name)
        })
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
        write_output(&mut buf, OutputFormat::Human, &Empty {}, |_| String::new())
            .unwrap();
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
}
