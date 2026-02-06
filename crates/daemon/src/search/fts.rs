// FTS5-based full-text search index backed by SQLite.
// Behind the SearchIndex trait so we can swap to Tantivy later.

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use tracing::debug;

/// A single search hit returned by the index.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub doc_id: String,
    pub title: String,
    pub snippet: String,
    pub rank: f64,
}

/// Document to be indexed.
#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub doc_id: String,
    pub title: String,
    pub content: String,
}

/// Abstraction over full-text search. V1 uses FTS5; can be swapped to Tantivy.
pub trait SearchIndex {
    /// Ensure the index schema exists.
    fn ensure_schema(&self) -> Result<()>;

    /// Index or update a document.
    fn upsert(&self, entry: &IndexEntry) -> Result<()>;

    /// Remove a document from the index.
    fn remove(&self, doc_id: &str) -> Result<()>;

    /// Search the index. Returns hits ranked by relevance.
    fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>>;

    /// Drop all indexed data and rebuild from the provided entries.
    fn rebuild(&self, entries: &[IndexEntry]) -> Result<()>;
}

/// SQLite FTS5-backed search index.
///
/// Uses the same `Connection` as meta.db — the FTS5 virtual table lives
/// alongside the regular tables. This keeps deployment simple (no extra files).
pub struct Fts5Index<'a> {
    conn: &'a Connection,
}

impl<'a> Fts5Index<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }
}

impl SearchIndex for Fts5Index<'_> {
    fn ensure_schema(&self) -> Result<()> {
        // FTS5 virtual table. content= means we manage the content ourselves
        // (no shadow content table — we store content in the FTS index directly).
        // tokenize: unicode61 handles Unicode word boundaries well.
        self.conn
            .execute_batch(
                "CREATE VIRTUAL TABLE IF NOT EXISTS search_index USING fts5(
                    doc_id UNINDEXED,
                    title,
                    content,
                    tokenize = 'unicode61'
                );",
            )
            .context("failed to create FTS5 search_index table")?;

        debug!("FTS5 search_index table ensured");
        Ok(())
    }

    fn upsert(&self, entry: &IndexEntry) -> Result<()> {
        // FTS5 doesn't support ON CONFLICT, so delete-then-insert.
        self.conn
            .execute("DELETE FROM search_index WHERE doc_id = ?1", params![entry.doc_id])
            .context("failed to delete old search entry")?;

        self.conn
            .execute(
                "INSERT INTO search_index (doc_id, title, content) VALUES (?1, ?2, ?3)",
                params![entry.doc_id, entry.title, entry.content],
            )
            .context("failed to insert search entry")?;

        Ok(())
    }

    fn remove(&self, doc_id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM search_index WHERE doc_id = ?1", params![doc_id])
            .context("failed to remove search entry")?;
        Ok(())
    }

    fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        if query.trim().is_empty() {
            return Ok(vec![]);
        }

        // Use MATCH for FTS5 full-text search. bm25() returns negative values
        // (lower = better), so we negate for a positive score.
        let mut stmt = self
            .conn
            .prepare(
                "SELECT doc_id, title, snippet(search_index, 2, '<b>', '</b>', '...', 32),
                        -rank
                 FROM search_index
                 WHERE search_index MATCH ?1
                 ORDER BY rank
                 LIMIT ?2",
            )
            .context("failed to prepare search query")?;

        let hits = stmt
            .query_map(params![query, limit as i64], |row| {
                Ok(SearchHit {
                    doc_id: row.get(0)?,
                    title: row.get(1)?,
                    snippet: row.get(2)?,
                    rank: row.get(3)?,
                })
            })
            .context("failed to execute search query")?
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to collect search results")?;

        Ok(hits)
    }

    fn rebuild(&self, entries: &[IndexEntry]) -> Result<()> {
        let tx =
            self.conn.unchecked_transaction().context("failed to start rebuild transaction")?;

        tx.execute("DELETE FROM search_index", [])
            .context("failed to clear search_index for rebuild")?;

        for entry in entries {
            tx.execute(
                "INSERT INTO search_index (doc_id, title, content) VALUES (?1, ?2, ?3)",
                params![entry.doc_id, entry.title, entry.content],
            )
            .context("failed to insert entry during rebuild")?;
        }

        tx.commit().context("failed to commit search index rebuild")?;

        debug!(count = entries.len(), "search index rebuilt");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_index() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        let idx = Fts5Index::new(&conn);
        idx.ensure_schema().unwrap();
        idx.upsert(&IndexEntry {
            doc_id: "doc-1".into(),
            title: "Getting Started".into(),
            content: "Welcome to Scriptum, a collaborative markdown editor.".into(),
        })
        .unwrap();
        idx.upsert(&IndexEntry {
            doc_id: "doc-2".into(),
            title: "Architecture".into(),
            content: "The daemon manages CRDT state and file watching.".into(),
        })
        .unwrap();
        idx.upsert(&IndexEntry {
            doc_id: "doc-3".into(),
            title: "API Reference".into(),
            content: "JSON-RPC methods: doc.open, doc.edit, doc.search.".into(),
        })
        .unwrap();
        conn
    }

    #[test]
    fn test_ensure_schema_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        let idx = Fts5Index::new(&conn);
        idx.ensure_schema().unwrap();
        idx.ensure_schema().unwrap(); // second call should not fail
    }

    #[test]
    fn test_search_returns_matching_docs() {
        let conn = setup_index();
        let idx = Fts5Index::new(&conn);

        let hits = idx.search("collaborative", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].doc_id, "doc-1");
        assert!(hits[0].rank > 0.0);
    }

    #[test]
    fn test_search_multiple_results() {
        let conn = Connection::open_in_memory().unwrap();
        let idx = Fts5Index::new(&conn);
        idx.ensure_schema().unwrap();

        // Insert docs sharing a common term
        for i in 0..3 {
            idx.upsert(&IndexEntry {
                doc_id: format!("multi-{i}"),
                title: format!("Document {i}"),
                content: format!("This document discusses Scriptum features part {i}."),
            })
            .unwrap();
        }

        let hits = idx.search("Scriptum", 10).unwrap();
        assert_eq!(hits.len(), 3);
    }

    #[test]
    fn test_search_respects_limit() {
        let conn = setup_index();
        let idx = Fts5Index::new(&conn);

        let hits = idx.search("doc", 1).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn test_search_empty_query_returns_nothing() {
        let conn = setup_index();
        let idx = Fts5Index::new(&conn);

        let hits = idx.search("", 10).unwrap();
        assert!(hits.is_empty());

        let hits = idx.search("   ", 10).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn test_search_no_match_returns_empty() {
        let conn = setup_index();
        let idx = Fts5Index::new(&conn);

        let hits = idx.search("zzzznonexistent", 10).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn test_upsert_updates_existing() {
        let conn = setup_index();
        let idx = Fts5Index::new(&conn);

        // Update doc-1 with new content
        idx.upsert(&IndexEntry {
            doc_id: "doc-1".into(),
            title: "Updated Title".into(),
            content: "Completely different content about kangaroos.".into(),
        })
        .unwrap();

        // Old content should not match
        let hits = idx.search("Welcome", 10).unwrap();
        assert!(hits.is_empty());

        // New content should match
        let hits = idx.search("kangaroos", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].doc_id, "doc-1");
        assert_eq!(hits[0].title, "Updated Title");
    }

    #[test]
    fn test_remove_deletes_from_index() {
        let conn = setup_index();
        let idx = Fts5Index::new(&conn);

        idx.remove("doc-1").unwrap();

        let hits = idx.search("collaborative", 10).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn test_remove_nonexistent_is_ok() {
        let conn = setup_index();
        let idx = Fts5Index::new(&conn);

        // Should not error
        idx.remove("doc-999").unwrap();
    }

    #[test]
    fn test_rebuild_replaces_all_content() {
        let conn = setup_index();
        let idx = Fts5Index::new(&conn);

        // Rebuild with completely new data
        idx.rebuild(&[IndexEntry {
            doc_id: "fresh-1".into(),
            title: "Fresh Doc".into(),
            content: "Brand new content about elephants.".into(),
        }])
        .unwrap();

        // Old docs should be gone
        let hits = idx.search("collaborative", 10).unwrap();
        assert!(hits.is_empty());

        // New doc should be found
        let hits = idx.search("elephants", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].doc_id, "fresh-1");
    }

    #[test]
    fn test_rebuild_empty_clears_index() {
        let conn = setup_index();
        let idx = Fts5Index::new(&conn);

        idx.rebuild(&[]).unwrap();

        let hits = idx.search("collaborative", 10).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn test_snippet_contains_highlight_markers() {
        let conn = setup_index();
        let idx = Fts5Index::new(&conn);

        let hits = idx.search("CRDT", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(
            hits[0].snippet.contains("<b>") && hits[0].snippet.contains("</b>"),
            "snippet should contain highlight markers: {}",
            hits[0].snippet
        );
    }

    #[test]
    fn test_title_is_searchable() {
        let conn = setup_index();
        let idx = Fts5Index::new(&conn);

        let hits = idx.search("Architecture", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].doc_id, "doc-2");
    }
}
