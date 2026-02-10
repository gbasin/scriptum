// `scriptum search` — full-text search across workspace documents.

use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::client::DaemonClient;
use crate::output::{self, OutputFormat};

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Search query.
    pub query: String,

    /// Limit results.
    #[arg(long, default_value = "20")]
    limit: usize,

    /// Force JSON output.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub query: String,
    pub total: usize,
    #[serde(default)]
    pub hits: Vec<SearchHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub doc_path: String,
    pub section_heading: String,
    pub snippet: String,
    pub score: f64,
}

pub fn run(args: SearchArgs) -> anyhow::Result<()> {
    let format = OutputFormat::detect(args.json);
    let query = args.query;
    let limit = args.limit;
    let rt = tokio::runtime::Handle::try_current()
        .map(|h| h.block_on(call_search(query.clone(), limit)))
        .unwrap_or_else(|_| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime should build")
                .block_on(call_search(query, limit))
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

async fn call_search(query: String, limit: usize) -> anyhow::Result<SearchResult> {
    let client = DaemonClient::default();
    client.call("workspace.search", json!({ "query": query, "limit": limit })).await
}

fn format_human(result: &SearchResult) -> String {
    if result.hits.is_empty() {
        return format!("No results for \"{}\".", result.query);
    }

    let mut lines = Vec::new();
    lines.push(format!("{} result(s) for \"{}\":", result.total, result.query));
    for h in &result.hits {
        lines.push(format!("\n  {} > {}", h.doc_path, h.section_heading));
        // Show snippet with leading indent.
        for line in h.snippet.lines() {
            lines.push(format!("    {line}"));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_result() -> SearchResult {
        SearchResult {
            query: "auth".into(),
            total: 2,
            hits: vec![
                SearchHit {
                    doc_path: "docs/api.md".into(),
                    section_heading: "## Authentication".into(),
                    snippet: "JWT tokens are used for...".into(),
                    score: 0.95,
                },
                SearchHit {
                    doc_path: "docs/readme.md".into(),
                    section_heading: "## Getting Started".into(),
                    snippet: "Set up auth by...".into(),
                    score: 0.72,
                },
            ],
        }
    }

    #[test]
    fn human_format_shows_results() {
        let output = format_human(&sample_result());
        assert!(output.contains("2 result(s)"));
        assert!(output.contains("docs/api.md"));
        assert!(output.contains("JWT tokens"));
    }

    #[test]
    fn human_format_no_results() {
        let result = SearchResult { query: "xyz".into(), total: 0, hits: vec![] };
        let output = format_human(&result);
        assert!(output.contains("No results"));
    }

    #[test]
    fn json_format_roundtrips() {
        let result = sample_result();
        let mut buf = Vec::new();
        output::write_output(&mut buf, OutputFormat::Json, &result, format_human).unwrap();
        let parsed: SearchResult = serde_json::from_slice(&buf).unwrap();
        assert_eq!(parsed.hits.len(), 2);
        assert_eq!(parsed.total, 2);
    }
}
