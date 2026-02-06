// `scriptum diff` — show pending changes since last commit.

use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::client::DaemonClient;
use crate::output::{self, OutputFormat};

#[derive(Debug, Args)]
pub struct DiffArgs {
    /// Document path (optional, omit for all docs).
    pub doc: Option<String>,

    /// Force JSON output.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffResult {
    #[serde(default)]
    pub changes: Vec<DocChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocChange {
    pub doc_path: String,
    pub status: String,
    #[serde(default)]
    pub hunks: Vec<DiffHunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    pub section_heading: String,
    pub added_lines: usize,
    pub removed_lines: usize,
    #[serde(default)]
    pub patch: Option<String>,
}

pub fn run(args: DiffArgs) -> anyhow::Result<()> {
    let format = OutputFormat::detect(args.json);
    let doc = args.doc;
    let rt = tokio::runtime::Handle::try_current()
        .map(|h| h.block_on(call_diff(doc.clone())))
        .unwrap_or_else(|_| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime should build")
                .block_on(call_diff(doc))
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

async fn call_diff(doc: Option<String>) -> anyhow::Result<DiffResult> {
    let client = DaemonClient::default();
    let params = match doc {
        Some(d) => json!({ "doc": d }),
        None => json!({}),
    };
    client.call("workspace.diff", params).await
}

fn format_human(result: &DiffResult) -> String {
    if result.changes.is_empty() {
        return "No pending changes.".into();
    }

    let mut lines = Vec::new();
    for c in &result.changes {
        lines.push(format!("{} ({})", c.doc_path, c.status));
        for h in &c.hunks {
            lines.push(format!(
                "  {} +{} -{}",
                h.section_heading, h.added_lines, h.removed_lines
            ));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_result() -> DiffResult {
        DiffResult {
            changes: vec![DocChange {
                doc_path: "docs/readme.md".into(),
                status: "modified".into(),
                hunks: vec![DiffHunk {
                    section_heading: "## Auth".into(),
                    added_lines: 5,
                    removed_lines: 2,
                    patch: None,
                }],
            }],
        }
    }

    #[test]
    fn human_format_shows_changes() {
        let output = format_human(&sample_result());
        assert!(output.contains("docs/readme.md"));
        assert!(output.contains("modified"));
        assert!(output.contains("+5 -2"));
    }

    #[test]
    fn human_format_no_changes() {
        let result = DiffResult { changes: vec![] };
        assert!(format_human(&result).contains("No pending changes"));
    }

    #[test]
    fn json_format_roundtrips() {
        let result = sample_result();
        let mut buf = Vec::new();
        output::write_output(&mut buf, OutputFormat::Json, &result, format_human).unwrap();
        let parsed: DiffResult = serde_json::from_slice(&buf).unwrap();
        assert_eq!(parsed.changes.len(), 1);
        assert_eq!(parsed.changes[0].hunks[0].added_lines, 5);
    }
}
