// Outbox queue: relay sync with exponential backoff, 10k/1GiB bounds.
//
// Updates flow through a state machine:
//   pending → sent → acked   (happy path)
//   pending → sent → pending  (retry on failure, with backoff)
//   pending → dead            (after MAX_ATTEMPTS failures)
//
// Backpressure: if a workspace exceeds 10,000 pending updates or 1 GiB
// of payload, `enqueue()` returns `OutboxBackpressure`.

use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};

use crate::security::{decrypt_at_rest, encrypt_at_rest};

// ── Constants ───────────────────────────────────────────────────────

const BASE_DELAY_MS: u64 = 250;
const MAX_DELAY_MS: u64 = 30_000;
const MAX_ATTEMPTS: u32 = 8;
const MAX_PENDING_UPDATES: i64 = 10_000;
const MAX_PENDING_BYTES: i64 = 1_073_741_824; // 1 GiB

// ── Types ───────────────────────────────────────────────────────────

/// Update lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateState {
    Pending,
    Sent,
    Acked,
    Dead,
}

impl UpdateState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Sent => "sent",
            Self::Acked => "acked",
            Self::Dead => "dead",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "sent" => Some(Self::Sent),
            "acked" => Some(Self::Acked),
            "dead" => Some(Self::Dead),
            _ => None,
        }
    }
}

/// A queued CRDT update destined for the relay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxUpdate {
    pub id: i64,
    pub workspace_id: String,
    pub doc_id: String,
    pub client_update_id: String,
    pub payload: Vec<u8>,
    pub retry_count: u32,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub state: UpdateState,
    pub created_at: DateTime<Utc>,
}

/// Result of a backpressure check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackpressureStatus {
    pub pending_count: i64,
    pub pending_bytes: i64,
    pub is_over_limit: bool,
}

/// Backpressure error returned when a workspace exceeds queue bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxBackpressure {
    pub workspace_id: String,
    pub pending_count: i64,
    pub pending_bytes: i64,
}

impl std::fmt::Display for OutboxBackpressure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "OUTBOX_BACKPRESSURE: workspace {} has {} pending updates ({} bytes)",
            self.workspace_id, self.pending_count, self.pending_bytes
        )
    }
}

impl std::error::Error for OutboxBackpressure {}

// ── Backoff ─────────────────────────────────────────────────────────

/// Compute exponential backoff delay for a given attempt number (0-based).
pub fn backoff_delay(attempt: u32) -> Duration {
    let exp = attempt.min(7); // cap exponent to avoid overflow
    let delay_ms = BASE_DELAY_MS.saturating_mul(1u64 << exp).min(MAX_DELAY_MS);
    Duration::from_millis(delay_ms)
}

// ── Queue operations ────────────────────────────────────────────────

/// Outbox queue backed by the `outbox_updates` SQLite table.
pub struct OutboxQueue<'a> {
    conn: &'a Connection,
}

impl<'a> OutboxQueue<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Enqueue a new update. Returns the row ID.
    ///
    /// Fails with `OutboxBackpressure` if the workspace is over its bounds.
    pub fn enqueue(
        &self,
        workspace_id: &str,
        doc_id: &str,
        client_update_id: &str,
        payload: &[u8],
        now: DateTime<Utc>,
    ) -> Result<i64> {
        let status = self.check_backpressure(workspace_id)?;
        if status.is_over_limit {
            return Err(OutboxBackpressure {
                workspace_id: workspace_id.to_string(),
                pending_count: status.pending_count,
                pending_bytes: status.pending_bytes,
            }
            .into());
        }

        let encrypted_payload =
            encrypt_at_rest(payload).context("failed to encrypt outbox payload at rest")?;

        self.conn
            .execute(
                "INSERT INTO outbox_updates \
                 (workspace_id, doc_id, client_update_id, payload, state, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    workspace_id,
                    doc_id,
                    client_update_id,
                    encrypted_payload,
                    UpdateState::Pending.as_str(),
                    now.to_rfc3339(),
                ],
            )
            .context("failed to insert outbox update")?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Transition an update to `sent`.
    pub fn mark_sent(&self, id: i64) -> Result<bool> {
        let rows = self
            .conn
            .execute(
                "UPDATE outbox_updates SET state = ?1 WHERE id = ?2 AND state = ?3",
                params![UpdateState::Sent.as_str(), id, UpdateState::Pending.as_str()],
            )
            .context("failed to mark outbox update as sent")?;
        Ok(rows > 0)
    }

    /// Transition an update to `acked` (final success state).
    pub fn mark_acked(&self, id: i64) -> Result<bool> {
        let rows = self
            .conn
            .execute(
                "UPDATE outbox_updates SET state = ?1 WHERE id = ?2 AND state = ?3",
                params![UpdateState::Acked.as_str(), id, UpdateState::Sent.as_str()],
            )
            .context("failed to mark outbox update as acked")?;
        Ok(rows > 0)
    }

    /// Record a send failure. Increments retry_count and either schedules
    /// a retry (with exponential backoff) or marks the update as `dead`.
    pub fn mark_failed(&self, id: i64, now: DateTime<Utc>) -> Result<bool> {
        let (current_retry_count, current_state): (u32, String) = self
            .conn
            .query_row(
                "SELECT retry_count, state FROM outbox_updates WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .context("failed to read outbox update for failure handling")?;

        if current_state != UpdateState::Sent.as_str() {
            return Ok(false);
        }

        let new_retry_count = current_retry_count + 1;

        if new_retry_count >= MAX_ATTEMPTS {
            self.conn
                .execute(
                    "UPDATE outbox_updates SET state = ?1, retry_count = ?2 \
                     WHERE id = ?3",
                    params![UpdateState::Dead.as_str(), new_retry_count, id],
                )
                .context("failed to mark outbox update as dead")?;
        } else {
            let delay = backoff_delay(current_retry_count);
            let next_retry = now + chrono::Duration::from_std(delay).unwrap_or_default();
            self.conn
                .execute(
                    "UPDATE outbox_updates SET state = ?1, retry_count = ?2, \
                     next_retry_at = ?3 WHERE id = ?4",
                    params![
                        UpdateState::Pending.as_str(),
                        new_retry_count,
                        next_retry.to_rfc3339(),
                        id,
                    ],
                )
                .context("failed to schedule outbox update retry")?;
        }

        Ok(true)
    }

    /// Fetch updates that are ready to send: state = pending AND
    /// (next_retry_at IS NULL OR next_retry_at <= now).
    pub fn ready_to_send(&self, now: DateTime<Utc>) -> Result<Vec<OutboxUpdate>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, workspace_id, doc_id, client_update_id, payload, \
                 retry_count, next_retry_at, state, created_at \
                 FROM outbox_updates \
                 WHERE state = ?1 AND (next_retry_at IS NULL OR next_retry_at <= ?2) \
                 ORDER BY id ASC",
            )
            .context("failed to prepare ready_to_send query")?;

        let now_str = now.to_rfc3339();
        let rows = stmt
            .query_map(params![UpdateState::Pending.as_str(), now_str], row_to_update)
            .context("failed to query ready outbox updates")?;

        rows.collect::<std::result::Result<Vec<_>, _>>().context("failed to collect outbox updates")
    }

    /// Count and size of non-terminal updates for a workspace.
    pub fn check_backpressure(&self, workspace_id: &str) -> Result<BackpressureStatus> {
        let (count, bytes): (i64, i64) = self
            .conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(LENGTH(payload)), 0) \
                 FROM outbox_updates \
                 WHERE workspace_id = ?1 AND state IN ('pending', 'sent')",
                params![workspace_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .context("failed to check outbox backpressure")?;

        Ok(BackpressureStatus {
            pending_count: count,
            pending_bytes: bytes,
            is_over_limit: count >= MAX_PENDING_UPDATES || bytes >= MAX_PENDING_BYTES,
        })
    }
}

fn row_to_update(row: &rusqlite::Row<'_>) -> rusqlite::Result<OutboxUpdate> {
    let state_str: String = row.get(7)?;
    let next_retry_str: Option<String> = row.get(6)?;
    let created_str: String = row.get(8)?;
    let encrypted_payload: Vec<u8> = row.get(4)?;
    let payload = decrypt_at_rest(&encrypted_payload).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            4,
            rusqlite::types::Type::Blob,
            Box::new(std::io::Error::other(error.to_string())),
        )
    })?;

    Ok(OutboxUpdate {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        doc_id: row.get(2)?,
        client_update_id: row.get(3)?,
        payload,
        retry_count: row.get(5)?,
        next_retry_at: next_retry_str.and_then(|s| s.parse::<DateTime<Utc>>().ok()),
        state: UpdateState::parse(&state_str).unwrap_or(UpdateState::Pending),
        created_at: created_str.parse::<DateTime<Utc>>().unwrap_or_else(|_| Utc::now()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::meta_db::MetaDb;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn setup() -> (MetaDb, PathBuf) {
        let path = unique_temp_db_path("outbox");
        let db = MetaDb::open(&path).expect("meta db should open");
        (db, path)
    }

    fn unique_temp_db_path(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
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

    // ── Backoff ─────────────────────────────────────────────────────

    #[test]
    fn backoff_starts_at_250ms() {
        assert_eq!(backoff_delay(0), Duration::from_millis(250));
    }

    #[test]
    fn backoff_doubles_each_attempt() {
        assert_eq!(backoff_delay(1), Duration::from_millis(500));
        assert_eq!(backoff_delay(2), Duration::from_millis(1000));
        assert_eq!(backoff_delay(3), Duration::from_millis(2000));
    }

    #[test]
    fn backoff_caps_at_30_seconds() {
        assert_eq!(backoff_delay(7), Duration::from_millis(30_000));
        assert_eq!(backoff_delay(8), Duration::from_millis(30_000));
        assert_eq!(backoff_delay(100), Duration::from_millis(30_000));
    }

    // ── Enqueue + state transitions ─────────────────────────────────

    #[test]
    fn enqueue_and_read_back() {
        let (db, path) = setup();
        let q = OutboxQueue::new(db.connection());
        let now = Utc::now();

        let id =
            q.enqueue("ws-1", "doc-1", "upd-1", b"hello", now).expect("enqueue should succeed");

        let ready = q.ready_to_send(now).expect("ready_to_send should work");
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, id);
        assert_eq!(ready[0].workspace_id, "ws-1");
        assert_eq!(ready[0].doc_id, "doc-1");
        assert_eq!(ready[0].payload, b"hello");
        assert_eq!(ready[0].state, UpdateState::Pending);
        assert_eq!(ready[0].retry_count, 0);

        cleanup(&path);
    }

    #[test]
    fn happy_path_pending_to_sent_to_acked() {
        let (db, path) = setup();
        let q = OutboxQueue::new(db.connection());
        let now = Utc::now();

        let id = q.enqueue("ws-1", "doc-1", "upd-1", b"data", now).expect("enqueue should succeed");

        assert!(q.mark_sent(id).expect("mark_sent"));
        // Should no longer appear in ready_to_send.
        let ready = q.ready_to_send(now).expect("ready_to_send");
        assert!(ready.is_empty());

        assert!(q.mark_acked(id).expect("mark_acked"));

        cleanup(&path);
    }

    #[test]
    fn mark_sent_only_from_pending() {
        let (db, path) = setup();
        let q = OutboxQueue::new(db.connection());
        let now = Utc::now();

        let id = q.enqueue("ws-1", "doc-1", "upd-1", b"data", now).expect("enqueue");
        q.mark_sent(id).expect("mark_sent");
        q.mark_acked(id).expect("mark_acked");

        // Trying to mark an acked update as sent should return false.
        assert!(!q.mark_sent(id).expect("double mark_sent"));

        cleanup(&path);
    }

    #[test]
    fn mark_acked_only_from_sent() {
        let (db, path) = setup();
        let q = OutboxQueue::new(db.connection());
        let now = Utc::now();

        let id = q.enqueue("ws-1", "doc-1", "upd-1", b"data", now).expect("enqueue");

        // Can't ack a pending update.
        assert!(!q.mark_acked(id).expect("ack pending"));

        cleanup(&path);
    }

    // ── Retry with backoff ──────────────────────────────────────────

    #[test]
    fn failure_schedules_retry_with_backoff() {
        let (db, path) = setup();
        let q = OutboxQueue::new(db.connection());
        let now = Utc::now();

        let id = q.enqueue("ws-1", "doc-1", "upd-1", b"data", now).expect("enqueue");
        q.mark_sent(id).expect("mark_sent");
        q.mark_failed(id, now).expect("mark_failed");

        // Should NOT be ready immediately (next_retry_at is in the future).
        let ready = q.ready_to_send(now).expect("ready_to_send");
        assert!(ready.is_empty());

        // Should be ready after the backoff delay.
        let after_delay = now + chrono::Duration::milliseconds(300);
        let ready = q.ready_to_send(after_delay).expect("ready_to_send after delay");
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].retry_count, 1);

        cleanup(&path);
    }

    #[test]
    fn update_becomes_dead_after_max_attempts() {
        let (db, path) = setup();
        let q = OutboxQueue::new(db.connection());
        let mut now = Utc::now();

        let id = q.enqueue("ws-1", "doc-1", "upd-1", b"data", now).expect("enqueue");

        // Fail MAX_ATTEMPTS times.
        for i in 0..MAX_ATTEMPTS {
            // Advance time past the backoff.
            now += chrono::Duration::seconds(60);
            let ready = q.ready_to_send(now).expect("ready");
            assert!(!ready.is_empty(), "attempt {i}: should be ready");
            q.mark_sent(id).expect("mark_sent");
            q.mark_failed(id, now).expect("mark_failed");
        }

        // Should NOT appear in ready_to_send anymore (it's dead).
        now += chrono::Duration::seconds(60);
        let ready = q.ready_to_send(now).expect("ready after dead");
        assert!(ready.is_empty());

        cleanup(&path);
    }

    #[test]
    fn mark_failed_only_from_sent() {
        let (db, path) = setup();
        let q = OutboxQueue::new(db.connection());
        let now = Utc::now();

        let id = q.enqueue("ws-1", "doc-1", "upd-1", b"data", now).expect("enqueue");

        // Can't fail a pending update.
        assert!(!q.mark_failed(id, now).expect("fail pending"));

        cleanup(&path);
    }

    // ── Backpressure ────────────────────────────────────────────────

    #[test]
    fn backpressure_reports_zero_for_empty_workspace() {
        let (db, path) = setup();
        let q = OutboxQueue::new(db.connection());

        let status = q.check_backpressure("ws-1").expect("check_backpressure");
        assert_eq!(status.pending_count, 0);
        assert_eq!(status.pending_bytes, 0);
        assert!(!status.is_over_limit);

        cleanup(&path);
    }

    #[test]
    fn backpressure_counts_pending_and_sent() {
        let (db, path) = setup();
        let q = OutboxQueue::new(db.connection());
        let now = Utc::now();

        let id1 = q.enqueue("ws-1", "doc-1", "upd-1", b"aaa", now).expect("enqueue 1");
        q.enqueue("ws-1", "doc-1", "upd-2", b"bbbb", now).expect("enqueue 2");

        // Mark first as sent — should still count toward backpressure.
        q.mark_sent(id1).expect("mark_sent");

        let status = q.check_backpressure("ws-1").expect("check_backpressure");
        assert_eq!(status.pending_count, 2);
        assert!(
            status.pending_bytes >= 7,
            "encrypted payload accounting should be >= raw payload bytes"
        );
        assert!(!status.is_over_limit);

        cleanup(&path);
    }

    #[test]
    fn enqueue_persists_encrypted_payload_bytes() {
        let (db, path) = setup();
        let q = OutboxQueue::new(db.connection());
        let now = Utc::now();

        q.enqueue("ws-1", "doc-1", "upd-1", b"hello", now).expect("enqueue should succeed");

        let stored: Vec<u8> = db
            .connection()
            .query_row(
                "SELECT payload FROM outbox_updates WHERE workspace_id = 'ws-1' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .expect("payload row should load");
        assert_ne!(stored, b"hello");
        assert!(stored.len() > b"hello".len());

        cleanup(&path);
    }

    #[test]
    fn backpressure_excludes_acked_updates() {
        let (db, path) = setup();
        let q = OutboxQueue::new(db.connection());
        let now = Utc::now();

        let id = q.enqueue("ws-1", "doc-1", "upd-1", b"data", now).expect("enqueue");
        q.mark_sent(id).expect("mark_sent");
        q.mark_acked(id).expect("mark_acked");

        let status = q.check_backpressure("ws-1").expect("check_backpressure");
        assert_eq!(status.pending_count, 0);

        cleanup(&path);
    }

    #[test]
    fn backpressure_is_per_workspace() {
        let (db, path) = setup();
        let q = OutboxQueue::new(db.connection());
        let now = Utc::now();

        q.enqueue("ws-1", "doc-1", "upd-1", b"aaa", now).expect("enqueue ws-1");
        q.enqueue("ws-2", "doc-2", "upd-2", b"bbb", now).expect("enqueue ws-2");

        let s1 = q.check_backpressure("ws-1").expect("ws-1");
        assert_eq!(s1.pending_count, 1);

        let s2 = q.check_backpressure("ws-2").expect("ws-2");
        assert_eq!(s2.pending_count, 1);

        cleanup(&path);
    }

    // ── ready_to_send ordering ──────────────────────────────────────

    #[test]
    fn ready_to_send_returns_in_id_order() {
        let (db, path) = setup();
        let q = OutboxQueue::new(db.connection());
        let now = Utc::now();

        let id1 = q.enqueue("ws-1", "doc-1", "upd-1", b"a", now).expect("enqueue 1");
        let id2 = q.enqueue("ws-1", "doc-1", "upd-2", b"b", now).expect("enqueue 2");
        let id3 = q.enqueue("ws-1", "doc-1", "upd-3", b"c", now).expect("enqueue 3");

        let ready = q.ready_to_send(now).expect("ready_to_send");
        assert_eq!(ready.len(), 3);
        assert_eq!(ready[0].id, id1);
        assert_eq!(ready[1].id, id2);
        assert_eq!(ready[2].id, id3);

        cleanup(&path);
    }

    // ── UpdateState parsing ─────────────────────────────────────────

    #[test]
    fn update_state_round_trips() {
        for state in
            [UpdateState::Pending, UpdateState::Sent, UpdateState::Acked, UpdateState::Dead]
        {
            assert_eq!(UpdateState::parse(state.as_str()), Some(state));
        }
    }

    #[test]
    fn update_state_parse_returns_none_for_unknown() {
        assert_eq!(UpdateState::parse("unknown"), None);
        assert_eq!(UpdateState::parse(""), None);
    }
}
