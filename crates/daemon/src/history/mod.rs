use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use crate::engine::ydoc::YDoc;

pub const DEFAULT_MAX_REPLAY_OPS: usize = 1_000;

#[derive(Debug, Clone)]
pub struct SnapshotPoint {
    pub snapshot_seq: i64,
    pub captured_at: DateTime<Utc>,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct WalEntry {
    pub server_seq: i64,
    pub applied_at: DateTime<Utc>,
    pub payload: Vec<u8>,
}

pub struct ReplayResult {
    pub document: YDoc,
    pub base_snapshot_seq: i64,
    pub applied_ops: usize,
    pub capped: bool,
}

pub struct RestoreResult {
    /// CRDT update that transforms the current document into the scrubbed target state.
    pub restore_update: Vec<u8>,
    /// Materialized scrubbed document at the requested restore point.
    pub restored_document: YDoc,
    pub base_snapshot_seq: i64,
    pub applied_ops: usize,
    pub capped: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct ReplayEngine {
    max_ops_per_scrub: usize,
}

impl ReplayEngine {
    pub fn new(max_ops_per_scrub: usize) -> Self {
        Self { max_ops_per_scrub }
    }

    pub fn scrub_to_time(
        &self,
        snapshots: &[SnapshotPoint],
        wal_entries: &[WalEntry],
        target_time: DateTime<Utc>,
    ) -> Result<ReplayResult> {
        let base_snapshot = snapshots
            .iter()
            .filter(|snapshot| snapshot.captured_at <= target_time)
            .max_by_key(|snapshot| snapshot.captured_at);

        let (document, base_snapshot_seq) = match base_snapshot {
            Some(snapshot) => (
                YDoc::from_state(&snapshot.payload).with_context(|| {
                    format!("failed to load snapshot at seq {}", snapshot.snapshot_seq)
                })?,
                snapshot.snapshot_seq,
            ),
            None => (YDoc::new(), 0),
        };

        let mut replayable_entries = wal_entries
            .iter()
            .filter(|entry| entry.server_seq > base_snapshot_seq && entry.applied_at <= target_time)
            .collect::<Vec<_>>();
        replayable_entries.sort_by_key(|entry| entry.server_seq);

        let total_replayable = replayable_entries.len();
        let mut applied_ops = 0usize;
        for entry in replayable_entries.into_iter().take(self.max_ops_per_scrub) {
            document.apply_update(&entry.payload).with_context(|| {
                format!("failed to apply wal update at seq {}", entry.server_seq)
            })?;
            applied_ops = applied_ops.saturating_add(1);
        }

        Ok(ReplayResult {
            document,
            base_snapshot_seq,
            applied_ops,
            capped: total_replayable > self.max_ops_per_scrub,
        })
    }

    /// Build a non-destructive restore update for `current_document` at `target_time`.
    ///
    /// The returned `restore_update` is a normal CRDT update payload that can be
    /// appended as a new edit, preserving full history.
    pub fn restore_to_time(
        &self,
        current_document: &YDoc,
        snapshots: &[SnapshotPoint],
        wal_entries: &[WalEntry],
        target_time: DateTime<Utc>,
    ) -> Result<RestoreResult> {
        let replay = self.scrub_to_time(snapshots, wal_entries, target_time)?;

        // Clone current state so restore generation never mutates caller-owned state.
        let current_state = current_document.encode_state();
        let current_state_vector = current_document.encode_state_vector();
        let working_doc = YDoc::from_state(&current_state)
            .context("failed to clone current document for restore")?;

        let target_content = replay.document.get_text_string("content");
        let current_content = working_doc.get_text_string("content");
        if current_content != target_content {
            working_doc.replace_text(
                "content",
                0,
                working_doc.text_len("content"),
                &target_content,
            );
        }

        let restore_update = working_doc
            .encode_diff(&current_state_vector)
            .context("failed to encode restore update")?;

        Ok(RestoreResult {
            restore_update,
            restored_document: replay.document,
            base_snapshot_seq: replay.base_snapshot_seq,
            applied_ops: replay.applied_ops,
            capped: replay.capped,
        })
    }
}

impl Default for ReplayEngine {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_REPLAY_OPS)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::{ReplayEngine, SnapshotPoint, WalEntry};
    use crate::engine::ydoc::YDoc;

    #[test]
    fn scrub_uses_nearest_snapshot_before_target_time() {
        let mut source_doc = YDoc::with_client_id(1);
        let mut seq = 0i64;

        let update_1 = append_text_update(&mut source_doc, "a");
        seq += 1;
        let snapshot_1 = SnapshotPoint {
            snapshot_seq: seq,
            captured_at: t(1),
            payload: source_doc.encode_state(),
        };

        let update_2 = append_text_update(&mut source_doc, "b");
        seq += 1;
        let update_3 = append_text_update(&mut source_doc, "c");
        seq += 1;
        let snapshot_2 = SnapshotPoint {
            snapshot_seq: seq,
            captured_at: t(4),
            payload: source_doc.encode_state(),
        };

        let update_4 = append_text_update(&mut source_doc, "d");
        let wal_entries = vec![
            WalEntry { server_seq: 1, applied_at: t(0), payload: update_1 },
            WalEntry { server_seq: 2, applied_at: t(2), payload: update_2 },
            WalEntry { server_seq: 3, applied_at: t(3), payload: update_3 },
            WalEntry { server_seq: 4, applied_at: t(5), payload: update_4 },
        ];

        let engine = ReplayEngine::default();
        let result = engine
            .scrub_to_time(&[snapshot_2, snapshot_1], &wal_entries, t(3))
            .expect("scrub should succeed");

        assert_eq!(result.base_snapshot_seq, 1);
        assert_eq!(result.applied_ops, 2);
        assert!(!result.capped);
        assert_eq!(result.document.get_text_string("content"), "abc");
    }

    #[test]
    fn scrub_caps_replay_to_max_ops() {
        let mut source_doc = YDoc::with_client_id(2);
        let mut wal_entries = Vec::new();

        for seq in 1..=1_005 {
            let update = append_text_update(&mut source_doc, "x");
            wal_entries.push(WalEntry { server_seq: seq, applied_at: t(seq), payload: update });
        }

        let engine = ReplayEngine::default();
        let result =
            engine.scrub_to_time(&[], &wal_entries, t(2_000)).expect("scrub should succeed");

        assert_eq!(result.base_snapshot_seq, 0);
        assert_eq!(result.applied_ops, 1_000);
        assert!(result.capped);
        assert_eq!(result.document.get_text_string("content").len(), 1_000);
    }

    #[test]
    fn restore_to_time_generates_non_destructive_update() {
        let mut source_doc = YDoc::with_client_id(3);
        let mut seq = 0i64;

        let update_1 = append_text_update(&mut source_doc, "a");
        seq += 1;
        let snapshot_1 = SnapshotPoint {
            snapshot_seq: seq,
            captured_at: t(1),
            payload: source_doc.encode_state(),
        };

        let update_2 = append_text_update(&mut source_doc, "b");
        seq += 1;
        let update_3 = append_text_update(&mut source_doc, "c");
        seq += 1;
        let snapshot_2 = SnapshotPoint {
            snapshot_seq: seq,
            captured_at: t(4),
            payload: source_doc.encode_state(),
        };

        let update_4 = append_text_update(&mut source_doc, "d");
        let wal_entries = vec![
            WalEntry { server_seq: 1, applied_at: t(0), payload: update_1 },
            WalEntry { server_seq: 2, applied_at: t(2), payload: update_2 },
            WalEntry { server_seq: 3, applied_at: t(3), payload: update_3 },
            WalEntry { server_seq: 4, applied_at: t(5), payload: update_4 },
        ];

        // Current document includes updates through seq=4.
        let current_doc = source_doc;
        assert_eq!(current_doc.get_text_string("content"), "abcd");

        let engine = ReplayEngine::default();
        let restore = engine
            .restore_to_time(&current_doc, &[snapshot_2, snapshot_1], &wal_entries, t(3))
            .expect("restore should succeed");

        assert_eq!(restore.base_snapshot_seq, 1);
        assert_eq!(restore.applied_ops, 2);
        assert!(!restore.capped);
        assert_eq!(restore.restored_document.get_text_string("content"), "abc");
        assert_eq!(current_doc.get_text_string("content"), "abcd");

        let materialized =
            YDoc::from_state(&current_doc.encode_state()).expect("current state should clone");
        materialized.apply_update(&restore.restore_update).expect("restore update should apply");
        assert_eq!(materialized.get_text_string("content"), "abc");
    }

    fn append_text_update(doc: &mut YDoc, content: &str) -> Vec<u8> {
        let before = doc.encode_state_vector();
        let existing_len = doc.get_text_string("content").len() as u32;
        doc.insert_text("content", existing_len, content);
        doc.encode_diff(&before).expect("diff should encode after local edit")
    }

    fn t(minute: i64) -> chrono::DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + minute * 60, 0)
            .single()
            .expect("timestamp should be representable")
    }
}
