// Agent session persistence: CRUD + pruning.
//
// Each agent connection creates a session row in `agent_sessions`.
// Sessions transition: active → disconnected → expired.
// Old sessions are pruned by `last_seen_at` cutoff.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};

// ── Types ────────────────────────────────────────────────────────────

/// Session lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    Active,
    Disconnected,
    Expired,
}

impl SessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Disconnected => "disconnected",
            Self::Expired => "expired",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "disconnected" => Some(Self::Disconnected),
            "expired" => Some(Self::Expired),
            _ => None,
        }
    }
}

/// A persisted agent session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSession {
    pub session_id: String,
    pub agent_id: String,
    pub workspace_id: String,
    pub started_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub status: SessionStatus,
}

// ── Store ────────────────────────────────────────────────────────────

/// Stateless CRUD operations on the `agent_sessions` table.
pub struct SessionStore;

impl SessionStore {
    /// Insert a new session.
    pub fn create(conn: &Connection, session: &AgentSession) -> Result<()> {
        conn.execute(
            "INSERT INTO agent_sessions \
             (session_id, agent_id, workspace_id, started_at, last_seen_at, status) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                session.session_id,
                session.agent_id,
                session.workspace_id,
                session.started_at.to_rfc3339(),
                session.last_seen_at.to_rfc3339(),
                session.status.as_str(),
            ],
        )
        .context("failed to insert agent session")?;
        Ok(())
    }

    /// Get a session by ID.
    pub fn get(conn: &Connection, session_id: &str) -> Result<Option<AgentSession>> {
        let mut stmt = conn
            .prepare(
                "SELECT session_id, agent_id, workspace_id, started_at, last_seen_at, status \
                 FROM agent_sessions WHERE session_id = ?1",
            )
            .context("failed to prepare session query")?;

        let mut rows = stmt
            .query_map(params![session_id], row_to_session)
            .context("failed to query agent session")?;

        match rows.next() {
            Some(row) => Ok(Some(row.context("failed to decode session row")?)),
            None => Ok(None),
        }
    }

    /// List sessions for a workspace, ordered by `last_seen_at` descending.
    pub fn list_by_workspace(conn: &Connection, workspace_id: &str) -> Result<Vec<AgentSession>> {
        let mut stmt = conn
            .prepare(
                "SELECT session_id, agent_id, workspace_id, started_at, last_seen_at, status \
                 FROM agent_sessions WHERE workspace_id = ?1 \
                 ORDER BY last_seen_at DESC",
            )
            .context("failed to prepare workspace sessions query")?;

        let rows = stmt
            .query_map(params![workspace_id], row_to_session)
            .context("failed to query workspace sessions")?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to collect workspace sessions")
    }

    /// List active sessions for a workspace.
    pub fn list_active(conn: &Connection, workspace_id: &str) -> Result<Vec<AgentSession>> {
        let mut stmt = conn
            .prepare(
                "SELECT session_id, agent_id, workspace_id, started_at, last_seen_at, status \
                 FROM agent_sessions WHERE workspace_id = ?1 AND status = 'active' \
                 ORDER BY last_seen_at DESC",
            )
            .context("failed to prepare active sessions query")?;

        let rows = stmt
            .query_map(params![workspace_id], row_to_session)
            .context("failed to query active sessions")?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to collect active sessions")
    }

    /// Update session status.
    pub fn update_status(
        conn: &Connection,
        session_id: &str,
        status: SessionStatus,
    ) -> Result<bool> {
        let changed = conn
            .execute(
                "UPDATE agent_sessions SET status = ?1 WHERE session_id = ?2",
                params![status.as_str(), session_id],
            )
            .context("failed to update session status")?;
        Ok(changed > 0)
    }

    /// Update `last_seen_at` (heartbeat).
    pub fn touch(conn: &Connection, session_id: &str, now: DateTime<Utc>) -> Result<bool> {
        let changed = conn
            .execute(
                "UPDATE agent_sessions SET last_seen_at = ?1 WHERE session_id = ?2",
                params![now.to_rfc3339(), session_id],
            )
            .context("failed to touch session")?;
        Ok(changed > 0)
    }

    /// Delete a session by ID.
    pub fn delete(conn: &Connection, session_id: &str) -> Result<bool> {
        let changed = conn
            .execute(
                "DELETE FROM agent_sessions WHERE session_id = ?1",
                params![session_id],
            )
            .context("failed to delete agent session")?;
        Ok(changed > 0)
    }

    /// Prune sessions with `last_seen_at` older than `cutoff`.
    pub fn prune_older_than(conn: &Connection, cutoff: DateTime<Utc>) -> Result<usize> {
        let removed = conn
            .execute(
                "DELETE FROM agent_sessions WHERE last_seen_at < ?1",
                params![cutoff.to_rfc3339()],
            )
            .context("failed to prune old agent sessions")?;
        Ok(removed)
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

fn row_to_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentSession> {
    let started_raw: String = row.get(3)?;
    let seen_raw: String = row.get(4)?;
    let status_raw: String = row.get(5)?;

    let started_at = started_raw
        .parse::<DateTime<Utc>>()
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e)))?;
    let last_seen_at = seen_raw
        .parse::<DateTime<Utc>>()
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e)))?;
    let status = SessionStatus::parse(&status_raw).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            5,
            rusqlite::types::Type::Text,
            format!("invalid session status `{status_raw}`").into(),
        )
    })?;

    Ok(AgentSession {
        session_id: row.get(0)?,
        agent_id: row.get(1)?,
        workspace_id: row.get(2)?,
        started_at,
        last_seen_at,
        status,
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
        let path = unique_path("session");
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

    fn ts(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).single().expect("timestamp should be valid")
    }

    fn make_session(id: &str, agent: &str, workspace: &str, at: DateTime<Utc>) -> AgentSession {
        AgentSession {
            session_id: id.into(),
            agent_id: agent.into(),
            workspace_id: workspace.into(),
            started_at: at,
            last_seen_at: at,
            status: SessionStatus::Active,
        }
    }

    #[test]
    fn create_and_get_session() {
        let (db, path) = setup();
        let now = ts(1_700_000_000);
        let session = make_session("s1", "claude-1", "ws-1", now);

        SessionStore::create(db.connection(), &session).expect("create should succeed");
        let loaded = SessionStore::get(db.connection(), "s1")
            .expect("get should succeed")
            .expect("session should exist");

        assert_eq!(loaded.session_id, "s1");
        assert_eq!(loaded.agent_id, "claude-1");
        assert_eq!(loaded.workspace_id, "ws-1");
        assert_eq!(loaded.status, SessionStatus::Active);
        assert_eq!(loaded.started_at, now);

        drop(db);
        cleanup(&path);
    }

    #[test]
    fn get_missing_returns_none() {
        let (db, path) = setup();
        let result = SessionStore::get(db.connection(), "nonexistent")
            .expect("get should succeed");
        assert!(result.is_none());
        drop(db);
        cleanup(&path);
    }

    #[test]
    fn list_by_workspace_orders_by_last_seen_desc() {
        let (db, path) = setup();
        let t1 = ts(1_700_000_100);
        let t2 = ts(1_700_000_200);
        let t3 = ts(1_700_000_300);

        SessionStore::create(db.connection(), &make_session("s1", "a1", "ws-1", t1)).unwrap();
        SessionStore::create(db.connection(), &make_session("s2", "a2", "ws-1", t3)).unwrap();
        SessionStore::create(db.connection(), &make_session("s3", "a3", "ws-1", t2)).unwrap();
        SessionStore::create(db.connection(), &make_session("s4", "a4", "ws-2", t1)).unwrap();

        let sessions = SessionStore::list_by_workspace(db.connection(), "ws-1")
            .expect("list should succeed");
        assert_eq!(sessions.len(), 3);
        assert_eq!(sessions[0].session_id, "s2"); // most recent
        assert_eq!(sessions[1].session_id, "s3");
        assert_eq!(sessions[2].session_id, "s1"); // oldest

        drop(db);
        cleanup(&path);
    }

    #[test]
    fn list_active_filters_by_status() {
        let (db, path) = setup();
        let now = ts(1_700_000_400);

        SessionStore::create(db.connection(), &make_session("s1", "a1", "ws-1", now)).unwrap();
        SessionStore::create(db.connection(), &make_session("s2", "a2", "ws-1", now)).unwrap();
        SessionStore::update_status(db.connection(), "s2", SessionStatus::Disconnected).unwrap();

        let active = SessionStore::list_active(db.connection(), "ws-1")
            .expect("list_active should succeed");
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].session_id, "s1");

        drop(db);
        cleanup(&path);
    }

    #[test]
    fn update_status_transitions() {
        let (db, path) = setup();
        let now = ts(1_700_000_500);

        SessionStore::create(db.connection(), &make_session("s1", "a1", "ws-1", now)).unwrap();

        let updated = SessionStore::update_status(db.connection(), "s1", SessionStatus::Disconnected)
            .expect("update should succeed");
        assert!(updated);

        let session = SessionStore::get(db.connection(), "s1").unwrap().unwrap();
        assert_eq!(session.status, SessionStatus::Disconnected);

        // Update nonexistent returns false.
        let missing = SessionStore::update_status(db.connection(), "nope", SessionStatus::Expired)
            .expect("update should succeed");
        assert!(!missing);

        drop(db);
        cleanup(&path);
    }

    #[test]
    fn touch_updates_last_seen_at() {
        let (db, path) = setup();
        let t1 = ts(1_700_000_600);
        let t2 = ts(1_700_000_700);

        SessionStore::create(db.connection(), &make_session("s1", "a1", "ws-1", t1)).unwrap();
        SessionStore::touch(db.connection(), "s1", t2).expect("touch should succeed");

        let session = SessionStore::get(db.connection(), "s1").unwrap().unwrap();
        assert_eq!(session.last_seen_at, t2);
        assert_eq!(session.started_at, t1); // unchanged

        drop(db);
        cleanup(&path);
    }

    #[test]
    fn delete_removes_session() {
        let (db, path) = setup();
        let now = ts(1_700_000_800);

        SessionStore::create(db.connection(), &make_session("s1", "a1", "ws-1", now)).unwrap();
        let deleted = SessionStore::delete(db.connection(), "s1").expect("delete should succeed");
        assert!(deleted);

        let gone = SessionStore::get(db.connection(), "s1").unwrap();
        assert!(gone.is_none());

        let not_found = SessionStore::delete(db.connection(), "s1").unwrap();
        assert!(!not_found);

        drop(db);
        cleanup(&path);
    }

    #[test]
    fn prune_removes_old_sessions() {
        let (db, path) = setup();
        let old = ts(1_700_000_000);
        let recent = ts(1_700_001_000);
        let cutoff = ts(1_700_000_500);

        SessionStore::create(db.connection(), &make_session("s1", "a1", "ws-1", old)).unwrap();
        SessionStore::create(db.connection(), &make_session("s2", "a2", "ws-1", recent)).unwrap();

        let removed = SessionStore::prune_older_than(db.connection(), cutoff)
            .expect("prune should succeed");
        assert_eq!(removed, 1);

        assert!(SessionStore::get(db.connection(), "s1").unwrap().is_none());
        assert!(SessionStore::get(db.connection(), "s2").unwrap().is_some());

        drop(db);
        cleanup(&path);
    }

    #[test]
    fn duplicate_session_id_errors() {
        let (db, path) = setup();
        let now = ts(1_700_002_000);

        SessionStore::create(db.connection(), &make_session("s1", "a1", "ws-1", now)).unwrap();
        let dup = SessionStore::create(db.connection(), &make_session("s1", "a2", "ws-1", now));
        assert!(dup.is_err());

        drop(db);
        cleanup(&path);
    }
}
