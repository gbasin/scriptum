// `scriptum bundle` — context bundle with token budget for agents.

use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::json;

use scriptum_common::protocol::rpc_methods;

use crate::client::DaemonClient;
use crate::output::{self, OutputFormat};

#[derive(Debug, Args)]
pub struct BundleArgs {
    /// Document path.
    pub doc: String,

    /// Maximum token budget (approximate).
    #[arg(long, default_value = "4000")]
    tokens: usize,

    /// Sections to include (repeatable). Omit for full doc.
    #[arg(long)]
    section: Vec<String>,

    /// Include metadata (section IDs, timestamps, agents).
    #[arg(long)]
    metadata: bool,

    /// Force JSON output.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleResult {
    pub doc_path: String,
    pub token_count: usize,
    pub token_budget: usize,
    pub content: String,
    #[serde(default)]
    pub sections_included: Vec<String>,
    #[serde(default)]
    pub truncated: bool,
}

pub fn run(args: BundleArgs) -> anyhow::Result<()> {
    let format = OutputFormat::detect(args.json);
    let params = BundleParams {
        doc: args.doc,
        tokens: args.tokens,
        sections: args.section,
        metadata: args.metadata,
    };
    let rt = tokio::runtime::Handle::try_current()
        .map(|h| h.block_on(call_bundle(params.clone())))
        .unwrap_or_else(|_| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime should build")
                .block_on(call_bundle(params))
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
struct BundleParams {
    doc: String,
    tokens: usize,
    sections: Vec<String>,
    metadata: bool,
}

async fn call_bundle(params: BundleParams) -> anyhow::Result<BundleResult> {
    let client = DaemonClient::default();
    let mut rpc_params = json!({
        "doc": params.doc,
        "token_budget": params.tokens,
        "metadata": params.metadata,
    });
    if !params.sections.is_empty() {
        rpc_params["sections"] = json!(params.sections);
    }
    client.call(rpc_methods::DOC_BUNDLE, rpc_params).await
}

fn format_human(result: &BundleResult) -> String {
    let mut lines = Vec::new();
    let truncated = if result.truncated { " (truncated)" } else { "" };
    lines.push(format!(
        "Bundle: {} — {}/{} tokens{}",
        result.doc_path, result.token_count, result.token_budget, truncated
    ));
    if !result.sections_included.is_empty() {
        lines.push(format!("Sections: {}", result.sections_included.join(", ")));
    }
    lines.push(String::new());
    lines.push(result.content.clone());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_result() -> BundleResult {
        BundleResult {
            doc_path: "docs/readme.md".into(),
            token_count: 1200,
            token_budget: 4000,
            content: "# README\n\nHello world.".into(),
            sections_included: vec!["# README".into(), "## Getting Started".into()],
            truncated: false,
        }
    }

    #[test]
    fn human_format_shows_bundle() {
        let output = format_human(&sample_result());
        assert!(output.contains("1200/4000"));
        assert!(output.contains("docs/readme.md"));
        assert!(output.contains("# README, ## Getting Started"));
        assert!(output.contains("Hello world"));
    }

    #[test]
    fn human_format_truncated() {
        let mut result = sample_result();
        result.truncated = true;
        let output = format_human(&result);
        assert!(output.contains("(truncated)"));
    }

    #[test]
    fn json_format_roundtrips() {
        let result = sample_result();
        let mut buf = Vec::new();
        output::write_output(&mut buf, OutputFormat::Json, &result, format_human).unwrap();
        let parsed: BundleResult = serde_json::from_slice(&buf).unwrap();
        assert_eq!(parsed.token_count, 1200);
        assert_eq!(parsed.sections_included.len(), 2);
    }
}
