// Incremental search index updates on file save / CRDT update.
//
// Receives PipelineEvents from the watcher and updates the FTS5 index.
// Handles creates, modifies, deletes, and renames.

use std::path::Path;

use anyhow::Result;
use tracing::debug;
use uuid::Uuid;

use super::fts::{IndexEntry, SearchIndex};

/// Incrementally updates the search index in response to document changes.
pub struct IndexUpdater<'a> {
    index: &'a dyn SearchIndex,
}

impl<'a> IndexUpdater<'a> {
    pub fn new(index: &'a dyn SearchIndex) -> Self {
        Self { index }
    }

    /// Update the index after a document was created or modified.
    pub fn on_doc_updated(&self, doc_id: Uuid, path: &Path, content: &str) -> Result<()> {
        let title = extract_title(content, path);
        self.index.upsert(&IndexEntry {
            doc_id: doc_id.to_string(),
            title,
            content: content.to_string(),
        })?;
        debug!(%doc_id, "search index updated");
        Ok(())
    }

    /// Remove a document from the index after it was deleted.
    pub fn on_doc_removed(&self, doc_id: Uuid) -> Result<()> {
        self.index.remove(&doc_id.to_string())?;
        debug!(%doc_id, "search index entry removed");
        Ok(())
    }

    /// Handle a document rename: remove the old entry, index the new one.
    pub fn on_doc_renamed(
        &self,
        old_doc_id: Uuid,
        new_doc_id: Uuid,
        new_path: &Path,
        content: &str,
    ) -> Result<()> {
        self.index.remove(&old_doc_id.to_string())?;
        let title = extract_title(content, new_path);
        self.index.upsert(&IndexEntry {
            doc_id: new_doc_id.to_string(),
            title,
            content: content.to_string(),
        })?;
        debug!(old = %old_doc_id, new = %new_doc_id, "search index rename handled");
        Ok(())
    }
}

/// Extract a document title from markdown content.
///
/// Uses the first `# Heading` found. Falls back to the filename without extension.
pub fn extract_title(content: &str, path: &Path) -> String {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(heading) = trimmed.strip_prefix("# ") {
            let title = heading.trim();
            if !title.is_empty() {
                return title.to_string();
            }
        }
    }
    path.file_stem().and_then(|s| s.to_str()).unwrap_or("Untitled").to_string()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rusqlite::Connection;
    use uuid::Uuid;

    use super::*;
    use crate::search::fts::Fts5Index;

    fn setup() -> (Connection, Uuid, Uuid, Uuid) {
        let conn = Connection::open_in_memory().unwrap();
        let idx = Fts5Index::new(&conn);
        idx.ensure_schema().unwrap();
        (conn, Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4())
    }

    // ── extract_title ─────────────────────────────────────────────────

    #[test]
    fn title_from_h1_heading() {
        let content = "# My Document\n\nSome body text.\n";
        assert_eq!(extract_title(content, &PathBuf::from("notes.md")), "My Document");
    }

    #[test]
    fn title_from_h1_with_leading_whitespace() {
        let content = "  # Indented Heading  \n\nBody.\n";
        assert_eq!(extract_title(content, &PathBuf::from("x.md")), "Indented Heading");
    }

    #[test]
    fn title_falls_back_to_filename() {
        let content = "No heading here, just body text.\n";
        assert_eq!(extract_title(content, &PathBuf::from("/docs/readme.md")), "readme");
    }

    #[test]
    fn title_ignores_h2_headings() {
        let content = "## Not H1\n\nBody.\n";
        assert_eq!(extract_title(content, &PathBuf::from("guide.md")), "guide");
    }

    #[test]
    fn title_skips_empty_h1() {
        let content = "# \n\n## Real Section\n";
        assert_eq!(extract_title(content, &PathBuf::from("doc.md")), "doc");
    }

    #[test]
    fn title_from_empty_content_uses_filename() {
        assert_eq!(extract_title("", &PathBuf::from("empty.md")), "empty");
    }

    // ── on_doc_updated ────────────────────────────────────────────────

    #[test]
    fn updated_doc_is_searchable() {
        let (conn, doc_a, _, _) = setup();
        let idx = Fts5Index::new(&conn);
        let updater = IndexUpdater::new(&idx);

        updater
            .on_doc_updated(
                doc_a,
                &PathBuf::from("api.md"),
                "# API Reference\n\nEndpoints for users.\n",
            )
            .unwrap();

        let hits = idx.search("Endpoints", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].doc_id, doc_a.to_string());
        assert_eq!(hits[0].title, "API Reference");
    }

    #[test]
    fn updated_doc_replaces_previous_content() {
        let (conn, doc_a, _, _) = setup();
        let idx = Fts5Index::new(&conn);
        let updater = IndexUpdater::new(&idx);

        updater
            .on_doc_updated(
                doc_a,
                &PathBuf::from("notes.md"),
                "# Notes\n\nOld content about cats.\n",
            )
            .unwrap();
        updater
            .on_doc_updated(
                doc_a,
                &PathBuf::from("notes.md"),
                "# Notes\n\nNew content about dogs.\n",
            )
            .unwrap();

        let hits = idx.search("cats", 10).unwrap();
        assert!(hits.is_empty(), "old content should be gone");

        let hits = idx.search("dogs", 10).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn multiple_docs_independently_searchable() {
        let (conn, doc_a, doc_b, _) = setup();
        let idx = Fts5Index::new(&conn);
        let updater = IndexUpdater::new(&idx);

        updater
            .on_doc_updated(doc_a, &PathBuf::from("alpha.md"), "# Alpha\n\nFirst document.\n")
            .unwrap();
        updater
            .on_doc_updated(doc_b, &PathBuf::from("beta.md"), "# Beta\n\nSecond document.\n")
            .unwrap();

        let hits = idx.search("Alpha", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].doc_id, doc_a.to_string());

        let hits = idx.search("Second", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].doc_id, doc_b.to_string());
    }

    // ── on_doc_removed ────────────────────────────────────────────────

    #[test]
    fn removed_doc_no_longer_searchable() {
        let (conn, doc_a, _, _) = setup();
        let idx = Fts5Index::new(&conn);
        let updater = IndexUpdater::new(&idx);

        updater
            .on_doc_updated(doc_a, &PathBuf::from("temp.md"), "# Temp\n\nEphemeral content.\n")
            .unwrap();

        let hits = idx.search("Ephemeral", 10).unwrap();
        assert_eq!(hits.len(), 1);

        updater.on_doc_removed(doc_a).unwrap();

        let hits = idx.search("Ephemeral", 10).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn removing_nonexistent_doc_is_ok() {
        let (conn, _, _, _) = setup();
        let idx = Fts5Index::new(&conn);
        let updater = IndexUpdater::new(&idx);

        // Should not error.
        updater.on_doc_removed(Uuid::new_v4()).unwrap();
    }

    #[test]
    fn remove_does_not_affect_other_docs() {
        let (conn, doc_a, doc_b, _) = setup();
        let idx = Fts5Index::new(&conn);
        let updater = IndexUpdater::new(&idx);

        updater.on_doc_updated(doc_a, &PathBuf::from("a.md"), "# A\n\nContent A.\n").unwrap();
        updater.on_doc_updated(doc_b, &PathBuf::from("b.md"), "# B\n\nContent B.\n").unwrap();

        updater.on_doc_removed(doc_a).unwrap();

        let hits = idx.search("Content", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].doc_id, doc_b.to_string());
    }

    // ── on_doc_renamed ────────────────────────────────────────────────

    #[test]
    fn rename_removes_old_and_indexes_new() {
        let (conn, old_id, new_id, _) = setup();
        let idx = Fts5Index::new(&conn);
        let updater = IndexUpdater::new(&idx);

        updater
            .on_doc_updated(old_id, &PathBuf::from("old-name.md"), "# Old\n\nOriginal content.\n")
            .unwrap();

        updater
            .on_doc_renamed(
                old_id,
                new_id,
                &PathBuf::from("new-name.md"),
                "# Renamed\n\nOriginal content.\n",
            )
            .unwrap();

        // Old doc_id should be gone.
        let hits = idx.search("Original", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].doc_id, new_id.to_string());
        assert_eq!(hits[0].title, "Renamed");
    }

    #[test]
    fn rename_with_same_id_updates_in_place() {
        let (conn, doc_id, _, _) = setup();
        let idx = Fts5Index::new(&conn);
        let updater = IndexUpdater::new(&idx);

        updater
            .on_doc_updated(doc_id, &PathBuf::from("draft.md"), "# Draft\n\nWork in progress.\n")
            .unwrap();

        // Same doc_id, different path and content (moved file).
        updater
            .on_doc_renamed(
                doc_id,
                doc_id,
                &PathBuf::from("final.md"),
                "# Final\n\nCompleted work.\n",
            )
            .unwrap();

        let hits = idx.search("Completed", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Final");

        let hits = idx.search("Draft", 10).unwrap();
        assert!(hits.is_empty());
    }
}
