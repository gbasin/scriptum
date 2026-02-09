// `scriptum edit` — replace section body (heading preserved).

use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::json;

use scriptum_common::protocol::rpc_methods;

use crate::client::DaemonClient;
use crate::output::{self, OutputFormat};

#[derive(Debug, Args)]
pub struct EditArgs {
    /// Document path.
    pub doc: String,

    /// Section heading to edit (e.g. `## Auth`).
    #[arg(long)]
    section: String,

    /// New content for the section body (heading preserved).
    #[arg(long, group = "content_source")]
    content: Option<String>,

    /// Read content from a file.
    #[arg(long, group = "content_source")]
    file: Option<String>,

    /// Agent name performing the edit.
    #[arg(long)]
    agent: String,

    /// Summary of the edit (for attribution log).
    #[arg(long)]
    summary: Option<String>,

    /// Force JSON output.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditResult {
    pub doc_path: String,
    pub section_id: String,
    pub heading: String,
    pub bytes_written: usize,
    pub etag: String,
}

pub fn run(args: EditArgs) -> anyhow::Result<()> {
    let format = OutputFormat::detect(args.json);

    // Resolve content from --content or --file.
    let body = match (&args.content, &args.file) {
        (Some(c), _) => c.clone(),
        (_, Some(path)) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("failed to read content file `{path}`: {e}"))?,
        (None, None) => anyhow::bail!("either --content or --file is required"),
    };

    let params = EditParams {
        doc: args.doc,
        section: args.section,
        content: body,
        agent: args.agent,
        summary: args.summary,
    };

    let rt = tokio::runtime::Handle::try_current()
        .map(|h| h.block_on(call_edit(params.clone())))
        .unwrap_or_else(|_| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime should build")
                .block_on(call_edit(params))
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
struct EditParams {
    doc: String,
    section: String,
    content: String,
    agent: String,
    summary: Option<String>,
}

async fn call_edit(params: EditParams) -> anyhow::Result<EditResult> {
    let client = DaemonClient::default();
    let mut rpc_params = json!({
        "doc": params.doc,
        "section": params.section,
        "content": params.content,
        "agent": params.agent,
    });
    if let Some(summary) = &params.summary {
        rpc_params["summary"] = json!(summary);
    }
    client.call(rpc_methods::DOC_EDIT_SECTION, rpc_params).await
}

fn format_human(result: &EditResult) -> String {
    format!(
        "Edited {} > {} [{}] ({} bytes, etag: {})",
        result.doc_path, result.heading, result.section_id, result.bytes_written, result.etag
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_result() -> EditResult {
        EditResult {
            doc_path: "docs/readme.md".into(),
            section_id: "sec-auth".into(),
            heading: "## Auth".into(),
            bytes_written: 256,
            etag: "doc:abc:3".into(),
        }
    }

    #[test]
    fn human_format_shows_edit_confirmation() {
        let output = format_human(&sample_result());
        assert!(output.contains("Edited"));
        assert!(output.contains("docs/readme.md"));
        assert!(output.contains("## Auth"));
        assert!(output.contains("256 bytes"));
        assert!(output.contains("doc:abc:3"));
    }

    #[test]
    fn json_format_roundtrips() {
        let result = sample_result();
        let mut buf = Vec::new();
        output::write_output(&mut buf, OutputFormat::Json, &result, format_human).unwrap();
        let parsed: EditResult = serde_json::from_slice(&buf).unwrap();
        assert_eq!(parsed.section_id, "sec-auth");
        assert_eq!(parsed.bytes_written, 256);
    }

    #[test]
    fn content_file_reads_from_disk() {
        let dir = tempfile::TempDir::new().unwrap();
        let file_path = dir.path().join("content.md");
        std::fs::write(&file_path, "new section body").unwrap();

        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "new section body");
    }
}
