// `scriptum ls` — list workspace documents.

use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::client::DaemonClient;
use crate::output::{self, OutputFormat};

#[derive(Debug, Args)]
pub struct LsArgs {
    /// Force JSON output.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LsResult {
    #[serde(default)]
    pub documents: Vec<DocEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocEntry {
    pub path: String,
    pub title: String,
    pub doc_id: String,
    pub sections: usize,
    #[serde(default)]
    pub active_agents: usize,
}

pub fn run(args: LsArgs) -> anyhow::Result<()> {
    let format = OutputFormat::detect(args.json);
    let rt = tokio::runtime::Handle::try_current()
        .map(|h| h.block_on(call_ls()))
        .unwrap_or_else(|_| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime should build")
                .block_on(call_ls())
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

async fn call_ls() -> anyhow::Result<LsResult> {
    let client = DaemonClient::default();
    client.call("workspace.ls", json!({})).await
}

fn format_human(result: &LsResult) -> String {
    if result.documents.is_empty() {
        return "No documents in workspace.".into();
    }

    let mut lines = Vec::new();
    lines.push(format!("{} document(s)", result.documents.len()));
    for d in &result.documents {
        let agents = if d.active_agents > 0 {
            format!(" ({} agent(s))", d.active_agents)
        } else {
            String::new()
        };
        lines.push(format!(
            "  {} — {} ({} sections){}",
            d.path, d.title, d.sections, agents
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_result() -> LsResult {
        LsResult {
            documents: vec![
                DocEntry {
                    path: "docs/readme.md".into(),
                    title: "README".into(),
                    doc_id: "doc-1".into(),
                    sections: 5,
                    active_agents: 2,
                },
                DocEntry {
                    path: "docs/api.md".into(),
                    title: "API Reference".into(),
                    doc_id: "doc-2".into(),
                    sections: 12,
                    active_agents: 0,
                },
            ],
        }
    }

    #[test]
    fn human_format_shows_documents() {
        let output = format_human(&sample_result());
        assert!(output.contains("2 document(s)"));
        assert!(output.contains("docs/readme.md"));
        assert!(output.contains("README"));
        assert!(output.contains("5 sections"));
        assert!(output.contains("2 agent(s)"));
        assert!(output.contains("docs/api.md"));
    }

    #[test]
    fn human_format_no_agents_omitted() {
        let output = format_human(&sample_result());
        // The api.md entry with 0 agents should not show "(0 agent(s))".
        let api_line = output.lines().find(|l| l.contains("api.md")).unwrap();
        assert!(!api_line.contains("agent(s)"));
    }

    #[test]
    fn human_format_empty() {
        let result = LsResult { documents: vec![] };
        let output = format_human(&result);
        assert!(output.contains("No documents"));
    }

    #[test]
    fn json_format_roundtrips() {
        let result = sample_result();
        let mut buf = Vec::new();
        output::write_output(&mut buf, OutputFormat::Json, &result, format_human).unwrap();
        let parsed: LsResult = serde_json::from_slice(&buf).unwrap();
        assert_eq!(parsed.documents.len(), 2);
        assert_eq!(parsed.documents[0].title, "README");
    }
}
