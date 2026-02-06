// Advisory section lease storage (in-memory + SQLite).
//
// Leases are TTL-driven only:
// - claim creates/refreshes a lease with `expires_at = now + ttl_sec`
// - activity extends the same lease by another full TTL window
// - expired leases are pruned from memory and SQLite

use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection};

/// Advisory lease mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseMode {
    Exclusive,
    Shared,
}

impl LeaseMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Exclusive => "exclusive",
            Self::Shared => "shared",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "exclusive" => Some(Self::Exclusive),
            "shared" => Some(Self::Shared),
            _ => None,
        }
    }
}

/// A persisted advisory lease.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionLease {
    pub workspace_id: String,
    pub doc_id: String,
    pub section_id: String,
    pub agent_id: String,
    pub ttl_sec: u32,
    pub mode: LeaseMode,
    pub note: Option<String>,
    pub expires_at: DateTime<Utc>,
}

impl SectionLease {
    pub fn is_expired_at(&self, now: DateTime<Utc>) -> bool {
        self.expires_at <= now
    }

    fn key(&self) -> LeaseKey {
        LeaseKey {
            workspace_id: self.workspace_id.clone(),
            doc_id: self.doc_id.clone(),
            section_id: self.section_id.clone(),
            agent_id: self.agent_id.clone(),
        }
    }
}

/// Claim request payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseClaim {
    pub workspace_id: String,
    pub doc_id: String,
    pub section_id: String,
    pub agent_id: String,
    pub ttl_sec: u32,
    pub mode: LeaseMode,
    pub note: Option<String>,
}

/// A conflicting active lease on the same section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseConflict {
    pub agent_id: String,
    pub section_id: String,
}

/// Result of claiming a lease.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimResult {
    pub lease: SectionLease,
    pub conflicts: Vec<LeaseConflict>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LeaseKey {
    workspace_id: String,
    doc_id: String,
    section_id: String,
    agent_id: String,
}

/// Lease store with in-memory fast path and SQLite durability.
#[derive(Debug, Default)]
pub struct LeaseStore {
    leases: HashMap<LeaseKey, SectionLease>,
}

impl LeaseStore {
    /// Load active leases from SQLite into memory.
    pub fn new(conn: &Connection, now: DateTime<Utc>) -> Result<Self> {
        let mut store = Self::default();
        store.prune_expired(conn, now)?;
        store.load_from_sqlite(conn, now)?;
        Ok(store)
    }

    /// Number of currently loaded active leases.
    pub fn len(&self) -> usize {
        self.leases.len()
    }

    /// Claim or refresh a lease.
    ///
    /// If a lease already exists for the same `(workspace, doc, section, agent)`,
    /// this updates its TTL/mode/note and extends expiration.
    pub fn claim(
        &mut self,
        conn: &Connection,
        claim: LeaseClaim,
        now: DateTime<Utc>,
    ) -> Result<ClaimResult> {
        if claim.ttl_sec == 0 {
            return Err(anyhow!("ttl_sec must be > 0"));
        }

        self.prune_expired(conn, now)?;

        let expires_at = now + Duration::seconds(i64::from(claim.ttl_sec));
        let conflicts = self.conflicts_for_section(
            &claim.workspace_id,
            &claim.doc_id,
            &claim.section_id,
            Some(&claim.agent_id),
            now,
        );
        let lease = SectionLease {
            workspace_id: claim.workspace_id,
            doc_id: claim.doc_id,
            section_id: claim.section_id,
            agent_id: claim.agent_id,
            ttl_sec: claim.ttl_sec,
            mode: claim.mode,
            note: claim.note,
            expires_at,
        };

        upsert_lease(conn, &lease)?;
        self.leases.insert(lease.key(), lease.clone());

        Ok(ClaimResult { lease, conflicts })
    }

    /// Extend an existing lease from activity.
    ///
    /// Returns `None` when no active lease exists for the given key.
    pub fn record_activity(
        &mut self,
        conn: &Connection,
        workspace_id: &str,
        doc_id: &str,
        section_id: &str,
        agent_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<SectionLease>> {
        self.prune_expired(conn, now)?;

        let key = LeaseKey {
            workspace_id: workspace_id.to_string(),
            doc_id: doc_id.to_string(),
            section_id: section_id.to_string(),
            agent_id: agent_id.to_string(),
        };
        let Some(lease) = self.leases.get_mut(&key) else {
            return Ok(None);
        };

        lease.expires_at = now + Duration::seconds(i64::from(lease.ttl_sec));
        upsert_lease(conn, lease)?;
        Ok(Some(lease.clone()))
    }

    /// Get active leases for a section.
    pub fn active_leases_for_section(
        &mut self,
        conn: &Connection,
        workspace_id: &str,
        doc_id: &str,
        section_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Vec<SectionLease>> {
        self.prune_expired(conn, now)?;

        let mut leases: Vec<SectionLease> = self
            .leases
            .values()
            .filter(|lease| {
                lease.workspace_id == workspace_id
                    && lease.doc_id == doc_id
                    && lease.section_id == section_id
                    && !lease.is_expired_at(now)
            })
            .cloned()
            .collect();
        leases.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));

        Ok(leases)
    }

    /// Remove expired leases from memory and SQLite.
    pub fn prune_expired(&mut self, conn: &Connection, now: DateTime<Utc>) -> Result<usize> {
        let before = self.leases.len();
        self.leases.retain(|_, lease| !lease.is_expired_at(now));
        let removed = before.saturating_sub(self.leases.len());

        conn.execute("DELETE FROM agent_leases WHERE expires_at <= ?1", params![now.to_rfc3339()])
            .context("failed to delete expired leases from sqlite")?;

        Ok(removed)
    }

    fn conflicts_for_section(
        &self,
        workspace_id: &str,
        doc_id: &str,
        section_id: &str,
        excluding_agent_id: Option<&str>,
        now: DateTime<Utc>,
    ) -> Vec<LeaseConflict> {
        let mut conflicts: Vec<LeaseConflict> = self
            .leases
            .values()
            .filter(|lease| {
                lease.workspace_id == workspace_id
                    && lease.doc_id == doc_id
                    && lease.section_id == section_id
                    && !lease.is_expired_at(now)
                    && excluding_agent_id.map(|agent| lease.agent_id != agent).unwrap_or(true)
            })
            .map(|lease| LeaseConflict {
                agent_id: lease.agent_id.clone(),
                section_id: lease.section_id.clone(),
            })
            .collect();
        conflicts.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
        conflicts
    }

    fn load_from_sqlite(&mut self, conn: &Connection, now: DateTime<Utc>) -> Result<()> {
        let mut stmt = conn
            .prepare(
                "SELECT workspace_id, doc_id, section_id, agent_id, ttl_sec, mode, note, expires_at \
                 FROM agent_leases \
                 WHERE expires_at > ?1",
            )
            .context("failed to prepare active lease query")?;
        let rows = stmt
            .query_map(params![now.to_rfc3339()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, String>(7)?,
                ))
            })
            .context("failed to query active leases from sqlite")?;

        for row in rows {
            let (
                workspace_id,
                doc_id,
                section_id,
                agent_id,
                ttl_sec_raw,
                mode_raw,
                note,
                expires_raw,
            ) = row.context("failed to decode lease row from sqlite")?;

            let ttl_sec = u32::try_from(ttl_sec_raw)
                .with_context(|| format!("invalid ttl_sec `{ttl_sec_raw}` in lease row"))?;
            let mode = LeaseMode::parse(&mode_raw)
                .ok_or_else(|| anyhow!("invalid lease mode `{mode_raw}` in lease row"))?;
            let expires_at = expires_raw.parse::<DateTime<Utc>>().with_context(|| {
                format!("invalid expires_at timestamp `{expires_raw}` in lease row")
            })?;
            let lease = SectionLease {
                workspace_id,
                doc_id,
                section_id,
                agent_id,
                ttl_sec,
                mode,
                note,
                expires_at,
            };
            self.leases.insert(lease.key(), lease);
        }

        Ok(())
    }
}

fn upsert_lease(conn: &Connection, lease: &SectionLease) -> Result<()> {
    conn.execute(
        "INSERT INTO agent_leases \
         (workspace_id, doc_id, section_id, agent_id, ttl_sec, mode, note, expires_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
         ON CONFLICT(workspace_id, doc_id, section_id, agent_id) DO UPDATE SET \
           ttl_sec = excluded.ttl_sec, \
           mode = excluded.mode, \
           note = excluded.note, \
           expires_at = excluded.expires_at",
        params![
            lease.workspace_id,
            lease.doc_id,
            lease.section_id,
            lease.agent_id,
            i64::from(lease.ttl_sec),
            lease.mode.as_str(),
            lease.note.as_deref(),
            lease.expires_at.to_rfc3339(),
        ],
    )
    .context("failed to upsert lease in sqlite")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::{Duration, TimeZone, Utc};

    use super::{LeaseClaim, LeaseMode, LeaseStore};
    use crate::store::meta_db::MetaDb;

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn setup() -> (MetaDb, PathBuf) {
        let path = unique_temp_db_path("leases");
        let db = MetaDb::open(&path).expect("meta db should open");
        (db, path)
    }

    fn unique_temp_db_path(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
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

    fn ts(seconds: i64) -> chrono::DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).single().expect("timestamp should be valid")
    }

    #[test]
    fn claim_persists_and_reloads_active_lease() {
        let (db, path) = setup();
        let now = ts(1_700_000_000);
        let mut store = LeaseStore::new(db.connection(), now).expect("store should load");
        let claim = LeaseClaim {
            workspace_id: "ws-1".into(),
            doc_id: "doc-1".into(),
            section_id: "auth".into(),
            agent_id: "claude-1".into(),
            ttl_sec: 600,
            mode: LeaseMode::Exclusive,
            note: Some("rewriting auth".into()),
        };

        let result = store.claim(db.connection(), claim, now).expect("claim should succeed");
        assert_eq!(result.conflicts.len(), 0);
        assert_eq!(store.len(), 1);

        let mut reloaded = LeaseStore::new(db.connection(), now + Duration::seconds(1))
            .expect("reload should succeed");
        let active = reloaded
            .active_leases_for_section(db.connection(), "ws-1", "doc-1", "auth", now)
            .expect("active lease query should succeed");
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].agent_id, "claude-1");
        assert_eq!(active[0].note.as_deref(), Some("rewriting auth"));

        drop(db);
        cleanup(&path);
    }

    #[test]
    fn activity_extends_ttl_and_updates_sqlite() {
        let (db, path) = setup();
        let now = ts(1_700_000_100);
        let mut store = LeaseStore::new(db.connection(), now).expect("store should load");
        let claimed = store
            .claim(
                db.connection(),
                LeaseClaim {
                    workspace_id: "ws-1".into(),
                    doc_id: "doc-1".into(),
                    section_id: "auth".into(),
                    agent_id: "claude-1".into(),
                    ttl_sec: 60,
                    mode: LeaseMode::Shared,
                    note: None,
                },
                now,
            )
            .expect("claim should succeed")
            .lease;
        let touched = store
            .record_activity(
                db.connection(),
                "ws-1",
                "doc-1",
                "auth",
                "claude-1",
                now + Duration::seconds(30),
            )
            .expect("activity update should succeed")
            .expect("lease should exist");

        assert!(touched.expires_at > claimed.expires_at);
        assert_eq!(touched.expires_at, now + Duration::seconds(90));

        let mut reloaded = LeaseStore::new(db.connection(), now + Duration::seconds(31))
            .expect("reload should succeed");
        let active = reloaded
            .active_leases_for_section(
                db.connection(),
                "ws-1",
                "doc-1",
                "auth",
                now + Duration::seconds(31),
            )
            .expect("active lease query should succeed");
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].expires_at, now + Duration::seconds(90));

        drop(db);
        cleanup(&path);
    }

    #[test]
    fn expired_leases_are_pruned_from_memory_and_sqlite() {
        let (db, path) = setup();
        let now = ts(1_700_000_200);
        let mut store = LeaseStore::new(db.connection(), now).expect("store should load");
        store
            .claim(
                db.connection(),
                LeaseClaim {
                    workspace_id: "ws-1".into(),
                    doc_id: "doc-1".into(),
                    section_id: "auth".into(),
                    agent_id: "claude-1".into(),
                    ttl_sec: 10,
                    mode: LeaseMode::Exclusive,
                    note: None,
                },
                now,
            )
            .expect("claim should succeed");

        let removed = store
            .prune_expired(db.connection(), now + Duration::seconds(11))
            .expect("prune should succeed");
        assert_eq!(removed, 1);
        assert_eq!(store.len(), 0);

        let rows: i64 = db
            .connection()
            .query_row("SELECT COUNT(*) FROM agent_leases", [], |row| row.get(0))
            .expect("count query should succeed");
        assert_eq!(rows, 0);

        drop(db);
        cleanup(&path);
    }

    #[test]
    fn claim_reports_conflicts_with_other_active_agents() {
        let (db, path) = setup();
        let now = ts(1_700_000_300);
        let mut store = LeaseStore::new(db.connection(), now).expect("store should load");
        store
            .claim(
                db.connection(),
                LeaseClaim {
                    workspace_id: "ws-1".into(),
                    doc_id: "doc-1".into(),
                    section_id: "auth".into(),
                    agent_id: "claude-1".into(),
                    ttl_sec: 300,
                    mode: LeaseMode::Exclusive,
                    note: None,
                },
                now,
            )
            .expect("first claim should succeed");
        let second = store
            .claim(
                db.connection(),
                LeaseClaim {
                    workspace_id: "ws-1".into(),
                    doc_id: "doc-1".into(),
                    section_id: "auth".into(),
                    agent_id: "copilot".into(),
                    ttl_sec: 300,
                    mode: LeaseMode::Shared,
                    note: Some("quick pass".into()),
                },
                now + Duration::seconds(1),
            )
            .expect("second claim should succeed");

        assert_eq!(second.conflicts.len(), 1);
        assert_eq!(second.conflicts[0].agent_id, "claude-1");
        assert_eq!(second.conflicts[0].section_id, "auth");

        drop(db);
        cleanup(&path);
    }

    #[test]
    fn claim_rejects_zero_ttl() {
        let (db, path) = setup();
        let now = ts(1_700_000_400);
        let mut store = LeaseStore::new(db.connection(), now).expect("store should load");
        let result = store.claim(
            db.connection(),
            LeaseClaim {
                workspace_id: "ws-1".into(),
                doc_id: "doc-1".into(),
                section_id: "auth".into(),
                agent_id: "claude-1".into(),
                ttl_sec: 0,
                mode: LeaseMode::Exclusive,
                note: None,
            },
            now,
        );

        assert!(result.is_err());

        drop(db);
        cleanup(&path);
    }
}
