// `scriptum conflicts` — show section overlap warnings.

use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::json;

use scriptum_common::protocol::rpc_methods;

use crate::client::DaemonClient;
use crate::output::{self, OutputFormat};

#[derive(Debug, Args)]
pub struct ConflictsArgs {
    /// Filter by document path.
    #[arg(long)]
    doc: Option<String>,

    /// Force JSON output.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictsResult {
    #[serde(default)]
    pub conflicts: Vec<Conflict>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    pub doc_path: String,
    pub section_id: String,
    pub heading: String,
    pub agents: Vec<ConflictAgent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictAgent {
    pub agent_id: String,
    pub display_name: String,
    pub intent: String,
}

pub fn run(args: ConflictsArgs) -> anyhow::Result<()> {
    let format = OutputFormat::detect(args.json);
    let doc_filter = args.doc;
    let rt = tokio::runtime::Handle::try_current()
        .map(|h| h.block_on(call_conflicts(doc_filter.clone())))
        .unwrap_or_else(|_| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime should build")
                .block_on(call_conflicts(doc_filter))
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

async fn call_conflicts(doc: Option<String>) -> anyhow::Result<ConflictsResult> {
    let client = DaemonClient::default();
    let params = match doc {
        Some(d) => json!({ "doc": d }),
        None => json!({}),
    };
    client.call(rpc_methods::AGENT_CONFLICTS, params).await
}

fn format_human(result: &ConflictsResult) -> String {
    if result.conflicts.is_empty() {
        return "No conflicts detected.".into();
    }

    let mut lines = Vec::new();
    lines.push(format!("{} conflict(s):", result.conflicts.len()));
    for c in &result.conflicts {
        lines.push(format!("\n  {} > {} [{}]", c.doc_path, c.heading, c.section_id));
        for a in &c.agents {
            lines.push(format!("    {} ({}) — {}", a.display_name, a.agent_id, a.intent));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_result() -> ConflictsResult {
        ConflictsResult {
            conflicts: vec![Conflict {
                doc_path: "docs/api.md".into(),
                section_id: "sec-xyz".into(),
                heading: "## Endpoints".into(),
                agents: vec![
                    ConflictAgent {
                        agent_id: "a1".into(),
                        display_name: "claude".into(),
                        intent: "editing".into(),
                    },
                    ConflictAgent {
                        agent_id: "a2".into(),
                        display_name: "copilot".into(),
                        intent: "editing".into(),
                    },
                ],
            }],
        }
    }

    #[test]
    fn human_format_shows_conflicts() {
        let output = format_human(&sample_result());
        assert!(output.contains("1 conflict(s)"));
        assert!(output.contains("docs/api.md"));
        assert!(output.contains("## Endpoints"));
        assert!(output.contains("claude"));
        assert!(output.contains("copilot"));
    }

    #[test]
    fn human_format_no_conflicts() {
        let result = ConflictsResult { conflicts: vec![] };
        let output = format_human(&result);
        assert!(output.contains("No conflicts detected"));
    }

    #[test]
    fn json_format_roundtrips() {
        let result = sample_result();
        let mut buf = Vec::new();
        output::write_output(&mut buf, OutputFormat::Json, &result, format_human).unwrap();
        let parsed: ConflictsResult = serde_json::from_slice(&buf).unwrap();
        assert_eq!(parsed.conflicts.len(), 1);
        assert_eq!(parsed.conflicts[0].agents.len(), 2);
    }
}
