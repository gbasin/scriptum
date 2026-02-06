// documents_local table access: create, update, read, list.
//
// Tracks local file projection state for each document in a workspace.

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

/// A row in the `documents_local` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDocumentRecord {
    pub doc_id: String,
    pub workspace_id: String,
    pub abs_path: String,
    pub line_ending_style: String,
    pub last_fs_mtime_ns: i64,
    pub last_content_hash: String,
    pub projection_rev: i64,
}

/// CRUD operations for `documents_local`.
pub struct DocumentsLocalStore;

impl DocumentsLocalStore {
    /// Insert a new local document row.
    pub fn insert(conn: &Connection, record: &LocalDocumentRecord) -> Result<()> {
        conn.execute(
            "INSERT INTO documents_local \
             (doc_id, workspace_id, abs_path, line_ending_style, last_fs_mtime_ns, \
              last_content_hash, projection_rev) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                record.doc_id,
                record.workspace_id,
                record.abs_path,
                record.line_ending_style,
                record.last_fs_mtime_ns,
                record.last_content_hash,
                record.projection_rev,
            ],
        )
        .context("failed to insert documents_local row")?;
        Ok(())
    }

    /// Update an existing local document row by `doc_id`.
    pub fn update(conn: &Connection, record: &LocalDocumentRecord) -> Result<bool> {
        let changed = conn
            .execute(
                "UPDATE documents_local \
                 SET workspace_id = ?1, abs_path = ?2, line_ending_style = ?3, \
                     last_fs_mtime_ns = ?4, last_content_hash = ?5, projection_rev = ?6 \
                 WHERE doc_id = ?7",
                params![
                    record.workspace_id,
                    record.abs_path,
                    record.line_ending_style,
                    record.last_fs_mtime_ns,
                    record.last_content_hash,
                    record.projection_rev,
                    record.doc_id,
                ],
            )
            .context("failed to update documents_local row")?;
        Ok(changed > 0)
    }

    /// Fetch a local document row by `doc_id`.
    pub fn get_by_doc_id(conn: &Connection, doc_id: &str) -> Result<Option<LocalDocumentRecord>> {
        let mut stmt = conn
            .prepare(
                "SELECT doc_id, workspace_id, abs_path, line_ending_style, \
                        last_fs_mtime_ns, last_content_hash, projection_rev \
                 FROM documents_local \
                 WHERE doc_id = ?1",
            )
            .context("failed to prepare documents_local by doc_id query")?;

        let mut rows = stmt
            .query_map(params![doc_id], row_to_record)
            .context("failed to query documents_local by doc_id")?;

        match rows.next() {
            Some(row) => Ok(Some(row.context("failed to decode documents_local row")?)),
            None => Ok(None),
        }
    }

    /// List local document rows for a workspace.
    pub fn list_by_workspace(
        conn: &Connection,
        workspace_id: &str,
    ) -> Result<Vec<LocalDocumentRecord>> {
        let mut stmt = conn
            .prepare(
                "SELECT doc_id, workspace_id, abs_path, line_ending_style, \
                        last_fs_mtime_ns, last_content_hash, projection_rev \
                 FROM documents_local \
                 WHERE workspace_id = ?1 \
                 ORDER BY abs_path ASC, doc_id ASC",
            )
            .context("failed to prepare documents_local by workspace query")?;

        let rows = stmt
            .query_map(params![workspace_id], row_to_record)
            .context("failed to query documents_local by workspace")?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to collect documents_local rows")
    }
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<LocalDocumentRecord> {
    Ok(LocalDocumentRecord {
        doc_id: row.get(0)?,
        workspace_id: row.get(1)?,
        abs_path: row.get(2)?,
        line_ending_style: row.get(3)?,
        last_fs_mtime_ns: row.get(4)?,
        last_content_hash: row.get(5)?,
        projection_rev: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::store::meta_db::MetaDb;

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn setup() -> (MetaDb, PathBuf) {
        let path = unique_path("documents-local");
        let db = MetaDb::open(&path).expect("meta db should open");
        (db, path)
    }

    fn unique_path(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should work")
            .as_nanos();
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("scriptum-{prefix}-{nanos}-{seq}.db"))
    }

    fn cleanup(path: &PathBuf) {
        let s = path.display().to_string();
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(format!("{s}-wal"));
        let _ = std::fs::remove_file(format!("{s}-shm"));
    }

    fn rec(doc: &str, ws: &str, path: &str, rev: i64) -> LocalDocumentRecord {
        LocalDocumentRecord {
            doc_id: doc.to_string(),
            workspace_id: ws.to_string(),
            abs_path: path.to_string(),
            line_ending_style: "lf".to_string(),
            last_fs_mtime_ns: 1_700_000_000_000_000_000,
            last_content_hash: format!("hash-{doc}-{rev}"),
            projection_rev: rev,
        }
    }

    #[test]
    fn insert_and_get_by_doc_id() {
        let (db, path) = setup();
        let row = rec("doc-1", "ws-1", "/repo/docs/a.md", 3);

        DocumentsLocalStore::insert(db.connection(), &row).expect("insert should succeed");
        let loaded = DocumentsLocalStore::get_by_doc_id(db.connection(), "doc-1")
            .expect("query should succeed")
            .expect("row should exist");

        assert_eq!(loaded, row);

        drop(db);
        cleanup(&path);
    }

    #[test]
    fn update_existing_row_by_doc_id() {
        let (db, path) = setup();
        DocumentsLocalStore::insert(db.connection(), &rec("doc-1", "ws-1", "/repo/a.md", 1))
            .expect("seed insert should succeed");

        let updated = LocalDocumentRecord {
            line_ending_style: "crlf".to_string(),
            last_fs_mtime_ns: 1_700_000_000_000_123_456,
            ..rec("doc-1", "ws-1", "/repo/renamed.md", 2)
        };
        let changed =
            DocumentsLocalStore::update(db.connection(), &updated).expect("update should succeed");
        assert!(changed);

        let loaded = DocumentsLocalStore::get_by_doc_id(db.connection(), "doc-1")
            .expect("query should succeed")
            .expect("row should exist");
        assert_eq!(loaded, updated);

        drop(db);
        cleanup(&path);
    }

    #[test]
    fn update_missing_row_returns_false() {
        let (db, path) = setup();
        let changed =
            DocumentsLocalStore::update(db.connection(), &rec("missing", "ws-1", "/x.md", 1))
                .expect("update should succeed");
        assert!(!changed);

        drop(db);
        cleanup(&path);
    }

    #[test]
    fn get_missing_row_returns_none() {
        let (db, path) = setup();
        let loaded = DocumentsLocalStore::get_by_doc_id(db.connection(), "missing")
            .expect("query should succeed");
        assert!(loaded.is_none());

        drop(db);
        cleanup(&path);
    }

    #[test]
    fn list_by_workspace_returns_only_matching_rows() {
        let (db, path) = setup();
        DocumentsLocalStore::insert(db.connection(), &rec("doc-a", "ws-1", "/repo/docs/a.md", 1))
            .unwrap();
        DocumentsLocalStore::insert(db.connection(), &rec("doc-b", "ws-1", "/repo/docs/b.md", 2))
            .unwrap();
        DocumentsLocalStore::insert(db.connection(), &rec("doc-c", "ws-2", "/repo/docs/c.md", 3))
            .unwrap();

        let rows =
            DocumentsLocalStore::list_by_workspace(db.connection(), "ws-1").expect("list should succeed");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].doc_id, "doc-a");
        assert_eq!(rows[1].doc_id, "doc-b");

        drop(db);
        cleanup(&path);
    }
}

