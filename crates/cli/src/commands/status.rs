// `scriptum status` — show agent's active sections and overlaps.

use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::json;

use scriptum_common::protocol::rpc_methods;

use crate::client::DaemonClient;
use crate::output::{self, OutputFormat};

#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Force JSON output.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatusResult {
    pub agent_id: String,
    pub display_name: String,
    #[serde(default)]
    pub ai_commits_configured: Option<bool>,
    #[serde(default)]
    pub active_sections: Vec<ActiveSection>,
    #[serde(default)]
    pub overlaps: Vec<SectionOverlap>,
}

#[derive(Debug, Clone, Deserialize)]
struct GitStatusResult {
    #[serde(default)]
    ai_configured: Option<bool>,
    #[serde(default)]
    ai_commit_enabled: Option<bool>,
    #[serde(default)]
    ai_enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveSection {
    pub doc_path: String,
    pub section_id: String,
    pub heading: String,
    pub intent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionOverlap {
    pub doc_path: String,
    pub section_id: String,
    pub heading: String,
    pub other_agent: String,
    pub other_intent: String,
}

pub fn run(args: StatusArgs) -> anyhow::Result<()> {
    let format = OutputFormat::detect(args.json);
    let rt = tokio::runtime::Handle::try_current()
        .map(|h| h.block_on(call_status()))
        .unwrap_or_else(|_| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime should build")
                .block_on(call_status())
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

async fn call_status() -> anyhow::Result<AgentStatusResult> {
    let client = DaemonClient::default();
    let mut status: AgentStatusResult = client.call(rpc_methods::AGENT_STATUS, json!({})).await?;

    if let Ok(git_status) =
        client.call::<_, GitStatusResult>(rpc_methods::GIT_STATUS, json!({})).await
    {
        status.ai_commits_configured =
            git_status.ai_configured.or(git_status.ai_commit_enabled).or(git_status.ai_enabled);
    }

    Ok(status)
}

fn format_human(result: &AgentStatusResult) -> String {
    let mut lines = Vec::new();
    lines.push(format!("{} ({})", result.display_name, result.agent_id));
    if let Some(ai_configured) = result.ai_commits_configured {
        lines.push(format!(
            "  AI commits: {}",
            if ai_configured { "configured" } else { "not configured" }
        ));
    }

    if result.active_sections.is_empty() {
        lines.push("  No active sections.".into());
    } else {
        lines.push(format!("  Active sections ({})", result.active_sections.len()));
        for s in &result.active_sections {
            lines.push(format!(
                "    {} > {} [{}] ({})",
                s.doc_path, s.heading, s.section_id, s.intent
            ));
        }
    }

    if !result.overlaps.is_empty() {
        lines.push(String::new());
        lines.push(format!("  Overlaps ({}):", result.overlaps.len()));
        for o in &result.overlaps {
            lines.push(format!(
                "    {} > {} — {} ({})",
                o.doc_path, o.heading, o.other_agent, o.other_intent
            ));
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_result() -> AgentStatusResult {
        AgentStatusResult {
            agent_id: "agent-1".into(),
            display_name: "claude".into(),
            ai_commits_configured: Some(true),
            active_sections: vec![ActiveSection {
                doc_path: "docs/readme.md".into(),
                section_id: "sec-abc".into(),
                heading: "## Auth".into(),
                intent: "editing".into(),
            }],
            overlaps: vec![SectionOverlap {
                doc_path: "docs/readme.md".into(),
                section_id: "sec-abc".into(),
                heading: "## Auth".into(),
                other_agent: "copilot".into(),
                other_intent: "reading".into(),
            }],
        }
    }

    #[test]
    fn human_format_shows_sections_and_overlaps() {
        let output = format_human(&sample_result());
        assert!(output.contains("claude"));
        assert!(output.contains("AI commits: configured"));
        assert!(output.contains("docs/readme.md"));
        assert!(output.contains("## Auth"));
        assert!(output.contains("sec-abc"));
        assert!(output.contains("copilot"));
    }

    #[test]
    fn human_format_empty_sections() {
        let result = AgentStatusResult {
            agent_id: "a".into(),
            display_name: "test".into(),
            ai_commits_configured: Some(false),
            active_sections: vec![],
            overlaps: vec![],
        };
        let output = format_human(&result);
        assert!(output.contains("AI commits: not configured"));
        assert!(output.contains("No active sections"));
    }

    #[test]
    fn json_format_roundtrips() {
        let result = sample_result();
        let mut buf = Vec::new();
        output::write_output(&mut buf, OutputFormat::Json, &result, format_human).unwrap();
        let parsed: AgentStatusResult = serde_json::from_slice(&buf).unwrap();
        assert_eq!(parsed.active_sections.len(), 1);
        assert_eq!(parsed.overlaps.len(), 1);
    }
}
