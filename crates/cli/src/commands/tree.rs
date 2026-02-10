// `scriptum tree` — show section tree structure with IDs.

use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::json;

use scriptum_common::protocol::rpc_methods;

use crate::client::DaemonClient;
use crate::output::{self, OutputFormat};

#[derive(Debug, Args)]
pub struct TreeArgs {
    /// Document path.
    pub doc: String,

    /// Force JSON output.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeResult {
    pub doc_path: String,
    #[serde(default)]
    pub sections: Vec<TreeSection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeSection {
    pub section_id: String,
    pub heading: String,
    pub level: u8,
    #[serde(default)]
    pub children: Vec<TreeSection>,
}

pub fn run(args: TreeArgs) -> anyhow::Result<()> {
    let format = OutputFormat::detect(args.json);
    let doc = args.doc;
    let rt = tokio::runtime::Handle::try_current()
        .map(|h| h.block_on(call_tree(doc.clone())))
        .unwrap_or_else(|_| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime should build")
                .block_on(call_tree(doc))
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

async fn call_tree(doc: String) -> anyhow::Result<TreeResult> {
    let client = DaemonClient::default();
    client.call(rpc_methods::DOC_TREE, json!({ "doc": doc })).await
}

fn format_human(result: &TreeResult) -> String {
    let mut lines = Vec::new();
    lines.push(result.doc_path.clone());
    for section in &result.sections {
        render_tree_node(&mut lines, section, 0);
    }
    lines.join("\n")
}

fn render_tree_node(lines: &mut Vec<String>, section: &TreeSection, depth: usize) {
    let indent = "  ".repeat(depth);
    let prefix = if depth == 0 { "" } else { "├─ " };
    lines.push(format!("{indent}{prefix}{} [{}]", section.heading, section.section_id));
    for child in &section.children {
        render_tree_node(lines, child, depth + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_result() -> TreeResult {
        TreeResult {
            doc_path: "docs/readme.md".into(),
            sections: vec![TreeSection {
                section_id: "sec-1".into(),
                heading: "# README".into(),
                level: 1,
                children: vec![
                    TreeSection {
                        section_id: "sec-2".into(),
                        heading: "## Getting Started".into(),
                        level: 2,
                        children: vec![],
                    },
                    TreeSection {
                        section_id: "sec-3".into(),
                        heading: "## API".into(),
                        level: 2,
                        children: vec![TreeSection {
                            section_id: "sec-4".into(),
                            heading: "### Endpoints".into(),
                            level: 3,
                            children: vec![],
                        }],
                    },
                ],
            }],
        }
    }

    #[test]
    fn human_format_renders_tree() {
        let output = format_human(&sample_result());
        assert!(output.contains("docs/readme.md"));
        assert!(output.contains("# README [sec-1]"));
        assert!(output.contains("## Getting Started [sec-2]"));
        assert!(output.contains("## API [sec-3]"));
        assert!(output.contains("### Endpoints [sec-4]"));
    }

    #[test]
    fn human_format_empty_tree() {
        let result = TreeResult { doc_path: "empty.md".into(), sections: vec![] };
        let output = format_human(&result);
        assert_eq!(output, "empty.md");
    }

    #[test]
    fn json_format_roundtrips() {
        let result = sample_result();
        let mut buf = Vec::new();
        output::write_output(&mut buf, OutputFormat::Json, &result, format_human).unwrap();
        let parsed: TreeResult = serde_json::from_slice(&buf).unwrap();
        assert_eq!(parsed.sections.len(), 1);
        assert_eq!(parsed.sections[0].children.len(), 2);
    }
}
