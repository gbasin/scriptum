// `scriptum read` — read document content, optionally scoped to a section.

use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::client::DaemonClient;
use crate::output::{self, OutputFormat};

#[derive(Debug, Args)]
pub struct ReadArgs {
    /// Document path.
    pub doc: String,

    /// Section heading to scope the read (e.g. `## Auth`).
    #[arg(long)]
    section: Option<String>,

    /// Agent name (registers read intent).
    #[arg(long)]
    agent: Option<String>,

    /// Force JSON output.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadResult {
    pub doc_path: String,
    #[serde(default)]
    pub section_heading: Option<String>,
    pub content: String,
    #[serde(default)]
    pub section_id: Option<String>,
}

pub fn run(args: ReadArgs) -> anyhow::Result<()> {
    let format = OutputFormat::detect(args.json);
    let params = ReadParams {
        doc: args.doc,
        section: args.section,
        agent: args.agent,
    };
    let rt = tokio::runtime::Handle::try_current()
        .map(|h| h.block_on(call_read(params.clone())))
        .unwrap_or_else(|_| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime should build")
                .block_on(call_read(params))
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
struct ReadParams {
    doc: String,
    section: Option<String>,
    agent: Option<String>,
}

async fn call_read(params: ReadParams) -> anyhow::Result<ReadResult> {
    let client = DaemonClient::default();
    let mut rpc_params = json!({ "doc": params.doc });
    if let Some(section) = &params.section {
        rpc_params["section"] = json!(section);
    }
    if let Some(agent) = &params.agent {
        rpc_params["agent"] = json!(agent);
    }
    client.call("doc.read_section", rpc_params).await
}

fn format_human(result: &ReadResult) -> String {
    let mut lines = Vec::new();
    if let Some(heading) = &result.section_heading {
        lines.push(format!("# {} > {}", result.doc_path, heading));
    } else {
        lines.push(format!("# {}", result.doc_path));
    }
    lines.push(String::new());
    lines.push(result.content.clone());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_result() -> ReadResult {
        ReadResult {
            doc_path: "docs/readme.md".into(),
            section_heading: Some("## Auth".into()),
            content: "Authentication is handled via JWT tokens.\n\nSee `auth.rs`.".into(),
            section_id: Some("sec-auth".into()),
        }
    }

    fn full_doc_result() -> ReadResult {
        ReadResult {
            doc_path: "docs/readme.md".into(),
            section_heading: None,
            content: "# README\n\nHello world.".into(),
            section_id: None,
        }
    }

    #[test]
    fn human_format_section_read() {
        let output = format_human(&sample_result());
        assert!(output.contains("docs/readme.md > ## Auth"));
        assert!(output.contains("Authentication is handled"));
    }

    #[test]
    fn human_format_full_doc() {
        let output = format_human(&full_doc_result());
        assert!(output.contains("# docs/readme.md"));
        assert!(!output.contains(">"));
        assert!(output.contains("Hello world"));
    }

    #[test]
    fn json_format_roundtrips() {
        let result = sample_result();
        let mut buf = Vec::new();
        output::write_output(&mut buf, OutputFormat::Json, &result, format_human).unwrap();
        let parsed: ReadResult = serde_json::from_slice(&buf).unwrap();
        assert_eq!(parsed.doc_path, "docs/readme.md");
        assert_eq!(parsed.section_heading.as_deref(), Some("## Auth"));
        assert_eq!(parsed.section_id.as_deref(), Some("sec-auth"));
    }

    #[test]
    fn json_format_full_doc() {
        let result = full_doc_result();
        let mut buf = Vec::new();
        output::write_output(&mut buf, OutputFormat::Json, &result, format_human).unwrap();
        let parsed: ReadResult = serde_json::from_slice(&buf).unwrap();
        assert!(parsed.section_heading.is_none());
        assert!(parsed.section_id.is_none());
    }
}
