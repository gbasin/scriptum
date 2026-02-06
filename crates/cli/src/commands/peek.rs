// `scriptum peek` — read section content without registering intent.

use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::client::DaemonClient;
use crate::output::{self, OutputFormat};

#[derive(Debug, Args)]
pub struct PeekArgs {
    /// Document path.
    pub doc: String,

    /// Section heading to peek at (e.g. `## Auth`).
    #[arg(long)]
    section: Option<String>,

    /// Force JSON output.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeekResult {
    pub doc_path: String,
    #[serde(default)]
    pub section_heading: Option<String>,
    #[serde(default)]
    pub section_id: Option<String>,
    pub content: String,
    pub byte_length: usize,
}

pub fn run(args: PeekArgs) -> anyhow::Result<()> {
    let format = OutputFormat::detect(args.json);
    let doc = args.doc;
    let section = args.section;
    let rt = tokio::runtime::Handle::try_current()
        .map(|h| h.block_on(call_peek(doc.clone(), section.clone())))
        .unwrap_or_else(|_| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime should build")
                .block_on(call_peek(doc, section))
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

async fn call_peek(doc: String, section: Option<String>) -> anyhow::Result<PeekResult> {
    let client = DaemonClient::default();
    let mut params = json!({ "doc": doc, "passive": true });
    if let Some(s) = &section {
        params["section"] = json!(s);
    }
    client.call("doc.peek", params).await
}

fn format_human(result: &PeekResult) -> String {
    let mut lines = Vec::new();
    let header = match &result.section_heading {
        Some(h) => format!("{} > {} ({} bytes)", result.doc_path, h, result.byte_length),
        None => format!("{} ({} bytes)", result.doc_path, result.byte_length),
    };
    lines.push(header);
    lines.push(String::new());
    lines.push(result.content.clone());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_section_result() -> PeekResult {
        PeekResult {
            doc_path: "docs/api.md".into(),
            section_heading: Some("## Auth".into()),
            section_id: Some("sec-auth".into()),
            content: "JWT tokens are used for authentication.".into(),
            byte_length: 39,
        }
    }

    fn sample_full_result() -> PeekResult {
        PeekResult {
            doc_path: "docs/readme.md".into(),
            section_heading: None,
            section_id: None,
            content: "# README\n\nHello world.".into(),
            byte_length: 21,
        }
    }

    #[test]
    fn human_format_section() {
        let output = format_human(&sample_section_result());
        assert!(output.contains("docs/api.md"));
        assert!(output.contains("## Auth"));
        assert!(output.contains("39 bytes"));
        assert!(output.contains("JWT tokens"));
    }

    #[test]
    fn human_format_full_doc() {
        let output = format_human(&sample_full_result());
        assert!(output.contains("docs/readme.md"));
        assert!(output.contains("21 bytes"));
        assert!(!output.contains(" > "));
        assert!(output.contains("Hello world"));
    }

    #[test]
    fn json_format_roundtrips() {
        let result = sample_section_result();
        let mut buf = Vec::new();
        output::write_output(&mut buf, OutputFormat::Json, &result, format_human).unwrap();
        let parsed: PeekResult = serde_json::from_slice(&buf).unwrap();
        assert_eq!(parsed.doc_path, "docs/api.md");
        assert_eq!(parsed.section_heading.as_deref(), Some("## Auth"));
        assert_eq!(parsed.byte_length, 39);
    }

    #[test]
    fn json_format_full_doc_roundtrips() {
        let result = sample_full_result();
        let mut buf = Vec::new();
        output::write_output(&mut buf, OutputFormat::Json, &result, format_human).unwrap();
        let parsed: PeekResult = serde_json::from_slice(&buf).unwrap();
        assert_eq!(parsed.section_heading, None);
        assert_eq!(parsed.section_id, None);
    }
}
