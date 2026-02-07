// Agent recent edit tracking: CRUD + pruning.
//
// Records character-level edit spans per agent per document.
// Used for attribution, activity feeds, and reconciliation UI.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};

// ── Types ────────────────────────────────────────────────────────────

/// A new edit to record (id is auto-generated).
#[derive(Debug, Clone)]
pub struct NewEdit {
    pub doc_id: String,
    pub agent_id: String,
    pub start_offset_utf16: i64,
    pub end_offset_utf16: i64,
    pub ts: DateTime<Utc>,
}

/// A persisted recent edit record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRecentEdit {
    pub id: i64,
    pub doc_id: String,
    pub agent_id: String,
    pub start_offset_utf16: i64,
    pub end_offset_utf16: i64,
    pub ts: DateTime<Utc>,
}

// ── Store ────────────────────────────────────────────────────────────

/// Stateless CRUD operations on the `agent_recent_edits` table.
pub struct EditStore;

impl EditStore {
    /// Record a new edit. Returns the auto-generated row ID.
    pub fn record(conn: &Connection, edit: &NewEdit) -> Result<i64> {
        conn.execute(
            "INSERT INTO agent_recent_edits \
             (doc_id, agent_id, start_offset_utf16, end_offset_utf16, ts) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                edit.doc_id,
                edit.agent_id,
                edit.start_offset_utf16,
                edit.end_offset_utf16,
                edit.ts.to_rfc3339(),
            ],
        )
        .context("failed to insert agent recent edit")?;
        Ok(conn.last_insert_rowid())
    }

    /// List recent edits for a document, newest first.
    pub fn list_by_doc(
        conn: &Connection,
        doc_id: &str,
        limit: usize,
    ) -> Result<Vec<AgentRecentEdit>> {
        let mut stmt = conn
            .prepare(
                "SELECT id, doc_id, agent_id, start_offset_utf16, end_offset_utf16, ts \
                 FROM agent_recent_edits WHERE doc_id = ?1 \
                 ORDER BY ts DESC LIMIT ?2",
            )
            .context("failed to prepare doc edits query")?;

        let rows = stmt
            .query_map(params![doc_id, limit as i64], row_to_edit)
            .context("failed to query edits by doc")?;

        rows.collect::<std::result::Result<Vec<_>, _>>().context("failed to collect doc edits")
    }

    /// List recent edits by an agent across all docs, newest first.
    pub fn list_by_agent(
        conn: &Connection,
        agent_id: &str,
        limit: usize,
    ) -> Result<Vec<AgentRecentEdit>> {
        let mut stmt = conn
            .prepare(
                "SELECT id, doc_id, agent_id, start_offset_utf16, end_offset_utf16, ts \
                 FROM agent_recent_edits WHERE agent_id = ?1 \
                 ORDER BY ts DESC LIMIT ?2",
            )
            .context("failed to prepare agent edits query")?;

        let rows = stmt
            .query_map(params![agent_id, limit as i64], row_to_edit)
            .context("failed to query edits by agent")?;

        rows.collect::<std::result::Result<Vec<_>, _>>().context("failed to collect agent edits")
    }

    /// Count edits for a document.
    pub fn count_by_doc(conn: &Connection, doc_id: &str) -> Result<usize> {
        conn.query_row(
            "SELECT COUNT(*) FROM agent_recent_edits WHERE doc_id = ?1",
            params![doc_id],
            |row| row.get::<_, i64>(0),
        )
        .map(|n| n as usize)
        .context("failed to count edits by doc")
    }

    /// Delete all edits for a document.
    pub fn delete_by_doc(conn: &Connection, doc_id: &str) -> Result<usize> {
        conn.execute("DELETE FROM agent_recent_edits WHERE doc_id = ?1", params![doc_id])
            .context("failed to delete edits by doc")
    }

    /// Prune edits older than `cutoff`.
    pub fn prune_older_than(conn: &Connection, cutoff: DateTime<Utc>) -> Result<usize> {
        conn.execute("DELETE FROM agent_recent_edits WHERE ts < ?1", params![cutoff.to_rfc3339()])
            .context("failed to prune old agent edits")
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

fn row_to_edit(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentRecentEdit> {
    let ts_raw: String = row.get(5)?;
    let ts = ts_raw.parse::<DateTime<Utc>>().map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
    })?;

    Ok(AgentRecentEdit {
        id: row.get(0)?,
        doc_id: row.get(1)?,
        agent_id: row.get(2)?,
        start_offset_utf16: row.get(3)?,
        end_offset_utf16: row.get(4)?,
        ts,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::store::meta_db::MetaDb;

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn setup() -> (MetaDb, PathBuf) {
        let path = unique_path("edits");
        let db = MetaDb::open(&path).expect("meta db should open");
        (db, path)
    }

    fn unique_path(prefix: &str) -> PathBuf {
        let nanos =
            SystemTime::now().duration_since(UNIX_EPOCH).expect("time should work").as_nanos();
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("scriptum-test-{prefix}-{nanos}-{seq}"));
        std::fs::create_dir_all(&dir).expect("should create temp test dir");
        dir.join("meta.db")
    }

    fn cleanup(path: &PathBuf) {
        let s = path.display().to_string();
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(format!("{s}-wal"));
        let _ = std::fs::remove_file(format!("{s}-shm"));
    }

    fn ts(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).single().expect("timestamp should be valid")
    }

    fn make_edit(doc: &str, agent: &str, start: i64, end: i64, at: DateTime<Utc>) -> NewEdit {
        NewEdit {
            doc_id: doc.into(),
            agent_id: agent.into(),
            start_offset_utf16: start,
            end_offset_utf16: end,
            ts: at,
        }
    }

    #[test]
    fn record_and_list_by_doc() {
        let (db, path) = setup();
        let t1 = ts(1_700_000_000);
        let t2 = ts(1_700_000_001);

        let id1 = EditStore::record(db.connection(), &make_edit("doc-1", "alice", 0, 10, t1))
            .expect("record should succeed");
        let id2 = EditStore::record(db.connection(), &make_edit("doc-1", "bob", 20, 30, t2))
            .expect("record should succeed");
        assert!(id2 > id1);

        let edits =
            EditStore::list_by_doc(db.connection(), "doc-1", 10).expect("list should succeed");
        assert_eq!(edits.len(), 2);
        // Newest first.
        assert_eq!(edits[0].agent_id, "bob");
        assert_eq!(edits[1].agent_id, "alice");
        assert_eq!(edits[0].start_offset_utf16, 20);
        assert_eq!(edits[0].end_offset_utf16, 30);

        drop(db);
        cleanup(&path);
    }

    #[test]
    fn list_by_doc_respects_limit() {
        let (db, path) = setup();
        let now = ts(1_700_000_100);

        for i in 0..5 {
            EditStore::record(
                db.connection(),
                &make_edit("doc-1", "alice", i * 10, i * 10 + 5, now),
            )
            .unwrap();
        }

        let edits =
            EditStore::list_by_doc(db.connection(), "doc-1", 3).expect("list should succeed");
        assert_eq!(edits.len(), 3);

        drop(db);
        cleanup(&path);
    }

    #[test]
    fn list_by_agent_across_docs() {
        let (db, path) = setup();
        let t1 = ts(1_700_000_200);
        let t2 = ts(1_700_000_201);

        EditStore::record(db.connection(), &make_edit("doc-1", "alice", 0, 5, t1)).unwrap();
        EditStore::record(db.connection(), &make_edit("doc-2", "alice", 10, 20, t2)).unwrap();
        EditStore::record(db.connection(), &make_edit("doc-1", "bob", 0, 5, t1)).unwrap();

        let edits =
            EditStore::list_by_agent(db.connection(), "alice", 10).expect("list should succeed");
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].doc_id, "doc-2"); // newest first
        assert_eq!(edits[1].doc_id, "doc-1");

        drop(db);
        cleanup(&path);
    }

    #[test]
    fn count_by_doc() {
        let (db, path) = setup();
        let now = ts(1_700_000_300);

        EditStore::record(db.connection(), &make_edit("doc-1", "alice", 0, 5, now)).unwrap();
        EditStore::record(db.connection(), &make_edit("doc-1", "bob", 10, 15, now)).unwrap();
        EditStore::record(db.connection(), &make_edit("doc-2", "alice", 0, 5, now)).unwrap();

        assert_eq!(EditStore::count_by_doc(db.connection(), "doc-1").unwrap(), 2);
        assert_eq!(EditStore::count_by_doc(db.connection(), "doc-2").unwrap(), 1);
        assert_eq!(EditStore::count_by_doc(db.connection(), "doc-3").unwrap(), 0);

        drop(db);
        cleanup(&path);
    }

    #[test]
    fn delete_by_doc_removes_all_edits_for_doc() {
        let (db, path) = setup();
        let now = ts(1_700_000_400);

        EditStore::record(db.connection(), &make_edit("doc-1", "alice", 0, 5, now)).unwrap();
        EditStore::record(db.connection(), &make_edit("doc-1", "bob", 10, 15, now)).unwrap();
        EditStore::record(db.connection(), &make_edit("doc-2", "alice", 0, 5, now)).unwrap();

        let removed =
            EditStore::delete_by_doc(db.connection(), "doc-1").expect("delete should succeed");
        assert_eq!(removed, 2);
        assert_eq!(EditStore::count_by_doc(db.connection(), "doc-1").unwrap(), 0);
        assert_eq!(EditStore::count_by_doc(db.connection(), "doc-2").unwrap(), 1);

        drop(db);
        cleanup(&path);
    }

    #[test]
    fn prune_removes_old_edits() {
        let (db, path) = setup();
        let old = ts(1_700_000_000);
        let recent = ts(1_700_001_000);
        let cutoff = ts(1_700_000_500);

        EditStore::record(db.connection(), &make_edit("doc-1", "alice", 0, 5, old)).unwrap();
        EditStore::record(db.connection(), &make_edit("doc-1", "bob", 10, 15, recent)).unwrap();

        let removed =
            EditStore::prune_older_than(db.connection(), cutoff).expect("prune should succeed");
        assert_eq!(removed, 1);

        let remaining = EditStore::list_by_doc(db.connection(), "doc-1", 10).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].agent_id, "bob");

        drop(db);
        cleanup(&path);
    }

    #[test]
    fn empty_queries_return_empty() {
        let (db, path) = setup();

        assert_eq!(EditStore::list_by_doc(db.connection(), "doc-1", 10).unwrap().len(), 0);
        assert_eq!(EditStore::list_by_agent(db.connection(), "alice", 10).unwrap().len(), 0);
        assert_eq!(EditStore::count_by_doc(db.connection(), "doc-1").unwrap(), 0);

        drop(db);
        cleanup(&path);
    }
}
