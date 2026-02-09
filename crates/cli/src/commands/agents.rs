// `scriptum agents` — list active agents in the workspace.

use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::json;

use scriptum_common::protocol::rpc_methods;

use crate::client::DaemonClient;
use crate::output::{self, OutputFormat};

#[derive(Debug, Args)]
pub struct AgentsArgs {
    /// Force JSON output.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentsResult {
    #[serde(default)]
    pub agents: Vec<AgentInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub agent_id: String,
    pub display_name: String,
    pub editor_type: String,
    #[serde(default)]
    pub active_sections: usize,
    #[serde(default)]
    pub last_active: Option<String>,
}

pub fn run(args: AgentsArgs) -> anyhow::Result<()> {
    let format = OutputFormat::detect(args.json);
    let rt = tokio::runtime::Handle::try_current()
        .map(|h| h.block_on(call_agents()))
        .unwrap_or_else(|_| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime should build")
                .block_on(call_agents())
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

async fn call_agents() -> anyhow::Result<AgentsResult> {
    let client = DaemonClient::default();
    client.call(rpc_methods::AGENT_LIST, json!({})).await
}

fn format_human(result: &AgentsResult) -> String {
    if result.agents.is_empty() {
        return "No active agents.".into();
    }

    let mut lines = Vec::new();
    lines.push(format!("{} active agent(s):", result.agents.len()));
    for a in &result.agents {
        let sections = if a.active_sections > 0 {
            format!("{} section(s)", a.active_sections)
        } else {
            "idle".into()
        };
        let last = a.last_active.as_deref().unwrap_or("unknown");
        lines.push(format!(
            "  {} ({}) [{}] — {} — last active: {}",
            a.display_name, a.agent_id, a.editor_type, sections, last
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_result() -> AgentsResult {
        AgentsResult {
            agents: vec![
                AgentInfo {
                    agent_id: "a1".into(),
                    display_name: "claude".into(),
                    editor_type: "agent".into(),
                    active_sections: 3,
                    last_active: Some("2026-02-06T22:00:00Z".into()),
                },
                AgentInfo {
                    agent_id: "a2".into(),
                    display_name: "alice".into(),
                    editor_type: "human".into(),
                    active_sections: 0,
                    last_active: None,
                },
            ],
        }
    }

    #[test]
    fn human_format_shows_agents() {
        let output = format_human(&sample_result());
        assert!(output.contains("2 active agent(s)"));
        assert!(output.contains("claude"));
        assert!(output.contains("3 section(s)"));
        assert!(output.contains("alice"));
        assert!(output.contains("idle"));
    }

    #[test]
    fn human_format_no_agents() {
        let result = AgentsResult { agents: vec![] };
        let output = format_human(&result);
        assert!(output.contains("No active agents"));
    }

    #[test]
    fn json_format_roundtrips() {
        let result = sample_result();
        let mut buf = Vec::new();
        output::write_output(&mut buf, OutputFormat::Json, &result, format_human).unwrap();
        let parsed: AgentsResult = serde_json::from_slice(&buf).unwrap();
        assert_eq!(parsed.agents.len(), 2);
        assert_eq!(parsed.agents[0].display_name, "claude");
        assert_eq!(parsed.agents[1].active_sections, 0);
    }
}
