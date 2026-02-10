// `scriptum whoami` — show agent identity and workspace state.

use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::json;

use scriptum_common::protocol::rpc_methods;

use crate::client::DaemonClient;
use crate::output::{self, OutputFormat};

#[derive(Debug, Args)]
pub struct WhoamiArgs {
    /// Force JSON output.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhoamiResult {
    pub agent_id: String,
    pub display_name: String,
    pub editor_type: String,
    pub workspace_root: String,
    #[serde(default)]
    pub workspace_id: Option<String>,
}

pub fn run(args: WhoamiArgs) -> anyhow::Result<()> {
    let format = OutputFormat::detect(args.json);
    let rt = tokio::runtime::Handle::try_current()
        .map(|h| {
            // Already inside a runtime — use it.
            h.block_on(call_whoami())
        })
        .unwrap_or_else(|_| {
            // Build a throwaway current-thread runtime.
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime should build")
                .block_on(call_whoami())
        });

    match rt {
        Ok(result) => {
            output::print_output(format, &result, format_human)?;
            Ok(())
        }
        Err(e) => {
            output::print_anyhow_error(format, &e);
            Err(e)
        }
    }
}

async fn call_whoami() -> anyhow::Result<WhoamiResult> {
    let client = DaemonClient::default();
    client.call(rpc_methods::AGENT_WHOAMI, json!({})).await
}

fn format_human(result: &WhoamiResult) -> String {
    let mut lines = Vec::new();
    lines.push(format!("Agent:     {} ({})", result.display_name, result.agent_id));
    lines.push(format!("Type:      {}", result.editor_type));
    lines.push(format!("Workspace: {}", result.workspace_root));
    if let Some(ws_id) = &result.workspace_id {
        lines.push(format!("Remote ID: {ws_id}"));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_result() -> WhoamiResult {
        WhoamiResult {
            agent_id: "agent-abc123".into(),
            display_name: "claude-code".into(),
            editor_type: "agent".into(),
            workspace_root: "/home/user/project".into(),
            workspace_id: Some("ws-456".into()),
        }
    }

    #[test]
    fn human_format_includes_all_fields() {
        let output = format_human(&sample_result());
        assert!(output.contains("claude-code"));
        assert!(output.contains("agent-abc123"));
        assert!(output.contains("agent"));
        assert!(output.contains("/home/user/project"));
        assert!(output.contains("ws-456"));
    }

    #[test]
    fn human_format_omits_remote_id_when_none() {
        let mut result = sample_result();
        result.workspace_id = None;
        let output = format_human(&result);
        assert!(!output.contains("Remote ID"));
    }

    #[test]
    fn json_format_roundtrips() {
        let result = sample_result();
        let mut buf = Vec::new();
        output::write_output(&mut buf, OutputFormat::Json, &result, format_human).unwrap();
        let parsed: WhoamiResult = serde_json::from_slice(&buf).unwrap();
        assert_eq!(parsed.agent_id, "agent-abc123");
        assert_eq!(parsed.display_name, "claude-code");
    }
}
