// `scriptum sections` — list sections with metadata.

use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::json;

use scriptum_common::protocol::rpc_methods;

use crate::client::DaemonClient;
use crate::output::{self, OutputFormat};

#[derive(Debug, Args)]
pub struct SectionsArgs {
    /// Document path.
    pub doc: String,

    /// Force JSON output.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionsResult {
    pub doc_path: String,
    #[serde(default)]
    pub sections: Vec<SectionMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionMeta {
    pub section_id: String,
    pub heading: String,
    pub level: u8,
    pub byte_offset: usize,
    pub byte_length: usize,
    #[serde(default)]
    pub claimed_by: Option<String>,
}

pub fn run(args: SectionsArgs) -> anyhow::Result<()> {
    let format = OutputFormat::detect(args.json);
    let doc = args.doc;
    let rt = tokio::runtime::Handle::try_current()
        .map(|h| h.block_on(call_sections(doc.clone())))
        .unwrap_or_else(|_| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime should build")
                .block_on(call_sections(doc))
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

async fn call_sections(doc: String) -> anyhow::Result<SectionsResult> {
    let client = DaemonClient::default();
    client.call(rpc_methods::DOC_SECTIONS, json!({ "doc": doc })).await
}

fn format_human(result: &SectionsResult) -> String {
    let mut lines = Vec::new();
    lines.push(format!("{} — {} section(s)", result.doc_path, result.sections.len()));

    if result.sections.is_empty() {
        return lines.join("\n");
    }

    // Header line.
    lines.push(format!("  {:<12} {:<4} {:>8} {:>8}  {}", "ID", "LVL", "OFFSET", "SIZE", "HEADING"));

    for s in &result.sections {
        let claimed = s.claimed_by.as_deref().map(|c| format!(" [{c}]")).unwrap_or_default();
        lines.push(format!(
            "  {:<12} H{:<3} {:>8} {:>8}  {}{}",
            s.section_id, s.level, s.byte_offset, s.byte_length, s.heading, claimed
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_result() -> SectionsResult {
        SectionsResult {
            doc_path: "docs/guide.md".into(),
            sections: vec![
                SectionMeta {
                    section_id: "sec-a".into(),
                    heading: "# Guide".into(),
                    level: 1,
                    byte_offset: 0,
                    byte_length: 500,
                    claimed_by: None,
                },
                SectionMeta {
                    section_id: "sec-b".into(),
                    heading: "## Install".into(),
                    level: 2,
                    byte_offset: 500,
                    byte_length: 200,
                    claimed_by: Some("claude".into()),
                },
            ],
        }
    }

    #[test]
    fn human_format_shows_table() {
        let output = format_human(&sample_result());
        assert!(output.contains("docs/guide.md"));
        assert!(output.contains("2 section(s)"));
        assert!(output.contains("sec-a"));
        assert!(output.contains("# Guide"));
        assert!(output.contains("sec-b"));
        assert!(output.contains("[claude]"));
    }

    #[test]
    fn human_format_empty() {
        let result = SectionsResult { doc_path: "empty.md".into(), sections: vec![] };
        let output = format_human(&result);
        assert!(output.contains("0 section(s)"));
    }

    #[test]
    fn json_format_roundtrips() {
        let result = sample_result();
        let mut buf = Vec::new();
        output::write_output(&mut buf, OutputFormat::Json, &result, format_human).unwrap();
        let parsed: SectionsResult = serde_json::from_slice(&buf).unwrap();
        assert_eq!(parsed.sections.len(), 2);
        assert_eq!(parsed.sections[1].claimed_by.as_deref(), Some("claude"));
    }
}
