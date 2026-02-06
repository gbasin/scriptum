// `scriptum blame` — CRDT-based per-line attribution.

use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::client::DaemonClient;
use crate::output::{self, OutputFormat};

#[derive(Debug, Args)]
pub struct BlameArgs {
    /// Document path.
    pub doc: String,

    /// Restrict to a specific section.
    #[arg(long)]
    section: Option<String>,

    /// Force JSON output.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlameResult {
    pub doc_path: String,
    #[serde(default)]
    pub lines: Vec<BlameLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlameLine {
    pub line_number: usize,
    pub agent: String,
    #[serde(default)]
    pub summary: Option<String>,
    pub timestamp: String,
    pub content: String,
}

pub fn run(args: BlameArgs) -> anyhow::Result<()> {
    let format = OutputFormat::detect(args.json);
    let doc = args.doc;
    let section = args.section;
    let rt = tokio::runtime::Handle::try_current()
        .map(|h| h.block_on(call_blame(doc.clone(), section.clone())))
        .unwrap_or_else(|_| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime should build")
                .block_on(call_blame(doc, section))
        });

    match rt {
        Ok(result) => {
            output::print_output(format, &result, format_human)?;
            Ok(())
        }
        Err(e) => {
            output::print_error(format, "RPC_ERROR", &format!("{e:#}"));
            Err(e)
        }
    }
}

async fn call_blame(doc: String, section: Option<String>) -> anyhow::Result<BlameResult> {
    let client = DaemonClient::default();
    let mut params = json!({ "doc": doc });
    if let Some(s) = &section {
        params["section"] = json!(s);
    }
    client.call("doc.blame", params).await
}

fn format_human(result: &BlameResult) -> String {
    if result.lines.is_empty() {
        return format!("{}: (empty)", result.doc_path);
    }

    let mut lines = Vec::new();
    lines.push(format!("{}:", result.doc_path));

    // Compute column widths for alignment.
    let num_width = result
        .lines
        .last()
        .map(|l| digit_count(l.line_number))
        .unwrap_or(1);
    let agent_width = result
        .lines
        .iter()
        .map(|l| l.agent.len())
        .max()
        .unwrap_or(0);

    for bl in &result.lines {
        lines.push(format!(
            "{:>nw$} | {:<aw$} | {} | {}",
            bl.line_number,
            bl.agent,
            &bl.timestamp,
            bl.content,
            nw = num_width,
            aw = agent_width,
        ));
    }
    lines.join("\n")
}

fn digit_count(n: usize) -> usize {
    if n == 0 {
        return 1;
    }
    ((n as f64).log10().floor() as usize) + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_result() -> BlameResult {
        BlameResult {
            doc_path: "docs/readme.md".into(),
            lines: vec![
                BlameLine {
                    line_number: 1,
                    agent: "alice".into(),
                    summary: Some("initial draft".into()),
                    timestamp: "2025-01-15T10:00:00Z".into(),
                    content: "# README".into(),
                },
                BlameLine {
                    line_number: 2,
                    agent: "bob".into(),
                    summary: None,
                    timestamp: "2025-01-16T14:30:00Z".into(),
                    content: "".into(),
                },
                BlameLine {
                    line_number: 3,
                    agent: "alice".into(),
                    summary: Some("added intro".into()),
                    timestamp: "2025-01-15T10:00:00Z".into(),
                    content: "Welcome to the project.".into(),
                },
            ],
        }
    }

    #[test]
    fn human_format_shows_blame() {
        let output = format_human(&sample_result());
        assert!(output.contains("docs/readme.md"));
        assert!(output.contains("alice"));
        assert!(output.contains("bob"));
        assert!(output.contains("# README"));
        assert!(output.contains("Welcome to the project"));
    }

    #[test]
    fn human_format_empty_doc() {
        let result = BlameResult {
            doc_path: "empty.md".into(),
            lines: vec![],
        };
        let output = format_human(&result);
        assert!(output.contains("(empty)"));
    }

    #[test]
    fn human_format_aligns_columns() {
        let result = sample_result();
        let output = format_human(&result);
        // All lines should have the pipe delimiter for consistent alignment.
        let data_lines: Vec<&str> = output.lines().skip(1).collect();
        assert_eq!(data_lines.len(), 3);
        for line in &data_lines {
            assert!(line.contains(" | "));
        }
    }

    #[test]
    fn json_format_roundtrips() {
        let result = sample_result();
        let mut buf = Vec::new();
        output::write_output(&mut buf, OutputFormat::Json, &result, format_human).unwrap();
        let parsed: BlameResult = serde_json::from_slice(&buf).unwrap();
        assert_eq!(parsed.lines.len(), 3);
        assert_eq!(parsed.lines[0].agent, "alice");
        assert_eq!(parsed.lines[1].line_number, 2);
    }

    #[test]
    fn digit_count_works() {
        assert_eq!(digit_count(0), 1);
        assert_eq!(digit_count(1), 1);
        assert_eq!(digit_count(9), 1);
        assert_eq!(digit_count(10), 2);
        assert_eq!(digit_count(99), 2);
        assert_eq!(digit_count(100), 3);
    }
}
