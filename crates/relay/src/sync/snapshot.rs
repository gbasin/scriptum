use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use sqlx::postgres::PgPool;
use tracing::{info_span, Instrument};
use uuid::Uuid;

pub const SNAPSHOT_INTERVAL_UPDATES: i64 = 1_000;
pub const SNAPSHOT_INTERVAL_MINUTES: i64 = 10;
pub const SNAPSHOT_RETAIN_COUNT: usize = 2;
pub const DEFAULT_LARGE_SNAPSHOT_THRESHOLD_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotPolicy {
    pub interval_updates: i64,
    pub interval: Duration,
}

impl Default for SnapshotPolicy {
    fn default() -> Self {
        Self {
            interval_updates: SNAPSHOT_INTERVAL_UPDATES,
            interval: Duration::from_secs((SNAPSHOT_INTERVAL_MINUTES * 60) as u64),
        }
    }
}

impl SnapshotPolicy {
    pub fn should_snapshot(
        &self,
        last_snapshot_seq: i64,
        current_seq: i64,
        last_snapshot_at: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> bool {
        if current_seq <= last_snapshot_seq {
            return false;
        }

        let updates_since_snapshot = current_seq.saturating_sub(last_snapshot_seq);
        if updates_since_snapshot >= self.interval_updates {
            return true;
        }

        let Some(interval) = ChronoDuration::from_std(self.interval).ok() else {
            return false;
        };

        now.signed_duration_since(last_snapshot_at) >= interval
    }
}

#[derive(Debug, Clone)]
pub struct SnapshotCompactorConfig {
    pub policy: SnapshotPolicy,
    pub retain_snapshots: usize,
    pub large_snapshot_threshold_bytes: usize,
}

impl Default for SnapshotCompactorConfig {
    fn default() -> Self {
        Self {
            policy: SnapshotPolicy::default(),
            retain_snapshots: SNAPSHOT_RETAIN_COUNT,
            large_snapshot_threshold_bytes: DEFAULT_LARGE_SNAPSHOT_THRESHOLD_BYTES,
        }
    }
}

pub type SnapshotStoreFuture<'a> = Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;

pub trait SnapshotObjectStore: Send + Sync {
    fn put_snapshot<'a>(&'a self, key: &'a str, payload: &'a [u8]) -> SnapshotStoreFuture<'a>;
}

#[derive(Debug, Clone)]
pub struct SnapshotCandidate {
    pub workspace_id: Uuid,
    pub doc_id: Uuid,
    pub snapshot_seq: i64,
    pub payload: Vec<u8>,
    pub now: DateTime<Utc>,
}

impl SnapshotCandidate {
    pub fn new(workspace_id: Uuid, doc_id: Uuid, snapshot_seq: i64, payload: Vec<u8>) -> Self {
        Self { workspace_id, doc_id, snapshot_seq, payload, now: Utc::now() }
    }

    pub fn with_now(
        workspace_id: Uuid,
        doc_id: Uuid,
        snapshot_seq: i64,
        payload: Vec<u8>,
        now: DateTime<Utc>,
    ) -> Self {
        Self { workspace_id, doc_id, snapshot_seq, payload, now }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotSkipReason {
    StaleSequence,
    PolicyNotSatisfied,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotPersistOutcome {
    pub snapshot_seq: i64,
    pub uploaded_to_object_store: bool,
    pub deleted_snapshot_rows: u64,
    pub deleted_update_rows: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotWriteResult {
    Skipped(SnapshotSkipReason),
    Persisted(SnapshotPersistOutcome),
}

#[derive(Default)]
pub struct SnapshotCompactor {
    config: SnapshotCompactorConfig,
    object_store: Option<Arc<dyn SnapshotObjectStore>>,
}

impl SnapshotCompactor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(config: SnapshotCompactorConfig) -> Self {
        Self { config, object_store: None }
    }

    pub fn with_object_store(mut self, object_store: Arc<dyn SnapshotObjectStore>) -> Self {
        self.object_store = Some(object_store);
        self
    }

    pub async fn maybe_snapshot_and_compact(
        &self,
        pool: &PgPool,
        candidate: SnapshotCandidate,
    ) -> Result<SnapshotWriteResult> {
        async {
            let latest_snapshot = sqlx::query_as::<_, SnapshotMetaRow>(
                "
                SELECT snapshot_seq, created_at
                FROM yjs_snapshots
                WHERE workspace_id = $1
                  AND doc_id = $2
                ORDER BY snapshot_seq DESC
                LIMIT 1
                ",
            )
            .bind(candidate.workspace_id)
            .bind(candidate.doc_id)
            .fetch_optional(pool)
            .instrument(info_span!("relay.db.query", query = "fetch_latest_snapshot_metadata"))
            .await
            .context("failed to fetch latest snapshot metadata")?;

            if let Some(snapshot) = latest_snapshot {
                if candidate.snapshot_seq <= snapshot.snapshot_seq {
                    return Ok(SnapshotWriteResult::Skipped(SnapshotSkipReason::StaleSequence));
                }

                if !self.config.policy.should_snapshot(
                    snapshot.snapshot_seq,
                    candidate.snapshot_seq,
                    snapshot.created_at,
                    candidate.now,
                ) {
                    return Ok(SnapshotWriteResult::Skipped(
                        SnapshotSkipReason::PolicyNotSatisfied,
                    ));
                }
            }

            let uploaded_to_object_store =
                self.should_upload_to_object_store(candidate.payload.len());
            if uploaded_to_object_store {
                if let Some(object_store) = &self.object_store {
                    let key = snapshot_object_storage_key(
                        candidate.workspace_id,
                        candidate.doc_id,
                        candidate.snapshot_seq,
                    );
                    object_store
                        .put_snapshot(&key, &candidate.payload)
                        .instrument(info_span!(
                            "relay.object_store.put_snapshot",
                            key = %key,
                            payload_bytes = candidate.payload.len()
                        ))
                        .await
                        .with_context(|| {
                            format!("failed to store snapshot in object storage: {key}")
                        })?;
                }
            }

            let mut tx = pool
                .begin()
                .instrument(info_span!("relay.db.query", query = "begin_snapshot_compaction_tx"))
                .await
                .context("failed to open transaction for snapshot compaction")?;

            sqlx::query(
                "
                INSERT INTO yjs_snapshots (workspace_id, doc_id, snapshot_seq, payload)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (workspace_id, doc_id, snapshot_seq) DO NOTHING
                ",
            )
            .bind(candidate.workspace_id)
            .bind(candidate.doc_id)
            .bind(candidate.snapshot_seq)
            .bind(candidate.payload.as_slice())
            .execute(&mut *tx)
            .instrument(info_span!("relay.db.query", query = "insert_yjs_snapshot"))
            .await
            .context("failed to persist yjs snapshot")?;

            let retain_limit =
                i64::try_from(self.config.retain_snapshots.max(1)).unwrap_or(i64::MAX);
            let retained_snapshots = sqlx::query_as::<_, SnapshotSeqRow>(
                "
                SELECT snapshot_seq
                FROM yjs_snapshots
                WHERE workspace_id = $1
                  AND doc_id = $2
                ORDER BY snapshot_seq DESC
                LIMIT $3
                ",
            )
            .bind(candidate.workspace_id)
            .bind(candidate.doc_id)
            .bind(retain_limit)
            .fetch_all(&mut *tx)
            .instrument(info_span!("relay.db.query", query = "fetch_retained_snapshots"))
            .await
            .context("failed to load retained snapshots for compaction")?;

            let snapshot_seqs =
                retained_snapshots.into_iter().map(|row| row.snapshot_seq).collect::<Vec<_>>();
            let plan = build_compaction_plan(&snapshot_seqs, self.config.retain_snapshots);

            let deleted_snapshot_rows =
                if let Some(delete_below_seq) = plan.delete_snapshots_below_seq {
                    sqlx::query(
                        "
                    DELETE FROM yjs_snapshots
                    WHERE workspace_id = $1
                      AND doc_id = $2
                      AND snapshot_seq < $3
                    ",
                    )
                    .bind(candidate.workspace_id)
                    .bind(candidate.doc_id)
                    .bind(delete_below_seq)
                    .execute(&mut *tx)
                    .instrument(info_span!("relay.db.query", query = "delete_compacted_snapshots"))
                    .await
                    .context("failed to compact old snapshots")?
                    .rows_affected()
                } else {
                    0
                };

            let deleted_update_rows =
                if let Some(delete_to_seq) = plan.delete_updates_at_or_below_seq {
                    sqlx::query(
                        "
                    DELETE FROM yjs_update_log
                    WHERE workspace_id = $1
                      AND doc_id = $2
                      AND server_seq <= $3
                    ",
                    )
                    .bind(candidate.workspace_id)
                    .bind(candidate.doc_id)
                    .bind(delete_to_seq)
                    .execute(&mut *tx)
                    .instrument(info_span!("relay.db.query", query = "delete_compacted_updates"))
                    .await
                    .context("failed to compact old yjs updates")?
                    .rows_affected()
                } else {
                    0
                };

            tx.commit()
                .instrument(info_span!("relay.db.query", query = "commit_snapshot_compaction_tx"))
                .await
                .context("failed to commit snapshot compaction transaction")?;

            Ok(SnapshotWriteResult::Persisted(SnapshotPersistOutcome {
                snapshot_seq: candidate.snapshot_seq,
                uploaded_to_object_store,
                deleted_snapshot_rows,
                deleted_update_rows,
            }))
        }
        .instrument(info_span!(
            "relay.snapshot.maybe_snapshot_and_compact",
            workspace_id = %candidate.workspace_id,
            doc_id = %candidate.doc_id,
            snapshot_seq = candidate.snapshot_seq,
            payload_bytes = candidate.payload.len()
        ))
        .await
    }

    fn should_upload_to_object_store(&self, payload_len: usize) -> bool {
        self.object_store.is_some() && payload_len >= self.config.large_snapshot_threshold_bytes
    }
}

pub fn snapshot_object_storage_key(workspace_id: Uuid, doc_id: Uuid, snapshot_seq: i64) -> String {
    format!("{workspace_id}/{doc_id}/{snapshot_seq}.snap")
}

#[derive(Debug, sqlx::FromRow)]
struct SnapshotMetaRow {
    snapshot_seq: i64,
    created_at: DateTime<Utc>,
}

#[derive(Debug, sqlx::FromRow)]
struct SnapshotSeqRow {
    snapshot_seq: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompactionPlan {
    delete_snapshots_below_seq: Option<i64>,
    delete_updates_at_or_below_seq: Option<i64>,
}

fn build_compaction_plan(snapshot_seqs: &[i64], retain_snapshots: usize) -> CompactionPlan {
    if snapshot_seqs.is_empty() {
        return CompactionPlan {
            delete_snapshots_below_seq: None,
            delete_updates_at_or_below_seq: None,
        };
    }

    let mut sorted = snapshot_seqs.to_vec();
    sorted.sort_unstable_by(|lhs, rhs| rhs.cmp(lhs));
    let retain_count = retain_snapshots.max(1);
    let oldest_retained_index = (retain_count - 1).min(sorted.len().saturating_sub(1));
    let oldest_retained_seq = sorted[oldest_retained_index];

    CompactionPlan {
        delete_snapshots_below_seq: Some(oldest_retained_seq),
        delete_updates_at_or_below_seq: if sorted.len() >= retain_count {
            Some(oldest_retained_seq)
        } else {
            None
        },
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use chrono::Utc;
    use uuid::Uuid;

    use super::{
        build_compaction_plan, snapshot_object_storage_key, SnapshotCompactor,
        SnapshotCompactorConfig, SnapshotObjectStore, SnapshotPolicy, SnapshotStoreFuture,
    };

    #[derive(Default)]
    struct NoopObjectStore;

    impl SnapshotObjectStore for NoopObjectStore {
        fn put_snapshot<'a>(
            &'a self,
            _key: &'a str,
            _payload: &'a [u8],
        ) -> SnapshotStoreFuture<'a> {
            Box::pin(async { Ok(()) })
        }
    }

    #[test]
    fn snapshot_policy_triggers_on_update_interval() {
        let now = Utc::now();
        let policy = SnapshotPolicy { interval_updates: 1_000, interval: Duration::from_secs(600) };

        assert!(policy.should_snapshot(100, 1_100, now, now));
    }

    #[test]
    fn snapshot_policy_triggers_on_time_interval() {
        let now = Utc::now();
        let policy = SnapshotPolicy { interval_updates: 1_000, interval: Duration::from_secs(600) };

        assert!(policy.should_snapshot(100, 101, now - chrono::Duration::minutes(10), now));
    }

    #[test]
    fn snapshot_policy_skips_when_thresholds_are_not_met() {
        let now = Utc::now();
        let policy = SnapshotPolicy { interval_updates: 1_000, interval: Duration::from_secs(600) };

        assert!(!policy.should_snapshot(100, 999, now - chrono::Duration::minutes(9), now));
    }

    #[test]
    fn snapshot_policy_skips_when_candidate_is_not_newer() {
        let now = Utc::now();
        let policy = SnapshotPolicy { interval_updates: 1_000, interval: Duration::from_secs(600) };

        assert!(!policy.should_snapshot(100, 100, now - chrono::Duration::minutes(10), now));
    }

    #[test]
    fn compaction_plan_retain_latest_two_and_compact_updates() {
        let plan = build_compaction_plan(&[3_000, 2_000, 1_000], 2);

        assert_eq!(plan.delete_snapshots_below_seq, Some(2_000));
        assert_eq!(plan.delete_updates_at_or_below_seq, Some(2_000));
    }

    #[test]
    fn compaction_plan_keeps_all_updates_until_two_snapshots_exist() {
        let plan = build_compaction_plan(&[1_000], 2);

        assert_eq!(plan.delete_snapshots_below_seq, Some(1_000));
        assert_eq!(plan.delete_updates_at_or_below_seq, None);
    }

    #[test]
    fn object_storage_upload_is_size_gated() {
        let compactor = SnapshotCompactor::with_config(SnapshotCompactorConfig {
            large_snapshot_threshold_bytes: 16,
            ..SnapshotCompactorConfig::default()
        })
        .with_object_store(Arc::new(NoopObjectStore));

        assert!(!compactor.should_upload_to_object_store(15));
        assert!(compactor.should_upload_to_object_store(16));
    }

    #[test]
    fn object_storage_key_matches_expected_layout() {
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();

        assert_eq!(
            snapshot_object_storage_key(workspace_id, doc_id, 42),
            format!("{workspace_id}/{doc_id}/42.snap")
        );
    }
}
