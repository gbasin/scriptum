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
