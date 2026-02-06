// `scriptum claim` — advisory section lease.

use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::client::DaemonClient;
use crate::output::{self, OutputFormat};

#[derive(Debug, Args)]
pub struct ClaimArgs {
    /// Document path.
    pub doc: String,

    /// Section heading to claim.
    #[arg(long)]
    section: String,

    /// Agent name.
    #[arg(long)]
    agent: String,

    /// Intent description (e.g. "editing", "refactoring").
    #[arg(long, default_value = "editing")]
    intent: String,

    /// Release an existing claim instead of acquiring.
    #[arg(long)]
    release: bool,

    /// Force JSON output.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimResult {
    pub doc_path: String,
    pub section_id: String,
    pub heading: String,
    pub action: String,
    #[serde(default)]
    pub warning: Option<String>,
}

pub fn run(args: ClaimArgs) -> anyhow::Result<()> {
    let format = OutputFormat::detect(args.json);
    let params = ClaimParams {
        doc: args.doc,
        section: args.section,
        agent: args.agent,
        intent: args.intent,
        release: args.release,
    };
    let rt = tokio::runtime::Handle::try_current()
        .map(|h| h.block_on(call_claim(params.clone())))
        .unwrap_or_else(|_| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime should build")
                .block_on(call_claim(params))
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

#[derive(Debug, Clone)]
struct ClaimParams {
    doc: String,
    section: String,
    agent: String,
    intent: String,
    release: bool,
}

async fn call_claim(params: ClaimParams) -> anyhow::Result<ClaimResult> {
    let client = DaemonClient::default();
    let rpc_params = json!({
        "doc": params.doc,
        "section": params.section,
        "agent": params.agent,
        "intent": params.intent,
        "release": params.release,
    });
    client.call("lease.claim", rpc_params).await
}

fn format_human(result: &ClaimResult) -> String {
    let mut msg = format!(
        "{}: {} > {} [{}]",
        result.action, result.doc_path, result.heading, result.section_id
    );
    if let Some(warning) = &result.warning {
        msg.push_str(&format!("\n  Warning: {warning}"));
    }
    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_format_claimed() {
        let result = ClaimResult {
            doc_path: "docs/api.md".into(),
            section_id: "sec-1".into(),
            heading: "## Auth".into(),
            action: "claimed".into(),
            warning: None,
        };
        let output = format_human(&result);
        assert!(output.contains("claimed"));
        assert!(output.contains("docs/api.md"));
        assert!(!output.contains("Warning"));
    }

    #[test]
    fn human_format_with_warning() {
        let result = ClaimResult {
            doc_path: "docs/api.md".into(),
            section_id: "sec-1".into(),
            heading: "## Auth".into(),
            action: "claimed".into(),
            warning: Some("another agent is also editing this section".into()),
        };
        let output = format_human(&result);
        assert!(output.contains("Warning"));
        assert!(output.contains("another agent"));
    }

    #[test]
    fn json_format_roundtrips() {
        let result = ClaimResult {
            doc_path: "d.md".into(),
            section_id: "s1".into(),
            heading: "## X".into(),
            action: "released".into(),
            warning: None,
        };
        let mut buf = Vec::new();
        output::write_output(&mut buf, OutputFormat::Json, &result, format_human).unwrap();
        let parsed: ClaimResult = serde_json::from_slice(&buf).unwrap();
        assert_eq!(parsed.action, "released");
    }
}
