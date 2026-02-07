use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use sqlx::{postgres::PgPool, Postgres, QueryBuilder};
use tokio::sync::RwLock;
use tracing::{info_span, Instrument};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PendingUpdate {
    pub workspace_id: Uuid,
    pub doc_id: Uuid,
    pub client_id: Uuid,
    pub client_update_id: Uuid,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct SequencedUpdate {
    pub workspace_id: Uuid,
    pub doc_id: Uuid,
    pub server_seq: i64,
    pub client_id: Uuid,
    pub client_update_id: Uuid,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SequencerKey {
    workspace_id: Uuid,
    doc_id: Uuid,
}

#[derive(Debug, sqlx::FromRow)]
struct MaxSeqRow {
    workspace_id: Uuid,
    doc_id: Uuid,
    max_server_seq: i64,
}

#[derive(Debug, Default)]
pub struct UpdateSequencer {
    counters: RwLock<HashMap<SequencerKey, Arc<AtomicI64>>>,
}

impl UpdateSequencer {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn next_server_seq(&self, workspace_id: Uuid, doc_id: Uuid) -> i64 {
        async move {
            let key = SequencerKey { workspace_id, doc_id };
            let counter = self.counter_for_key(key).await;
            counter.fetch_add(1, Ordering::SeqCst) + 1
        }
        .instrument(info_span!(
            "relay.sequencer.next_server_seq",
            workspace_id = %workspace_id,
            doc_id = %doc_id
        ))
        .await
    }

    pub async fn seed_counter(&self, workspace_id: Uuid, doc_id: Uuid, max_server_seq: i64) {
        async move {
            let key = SequencerKey { workspace_id, doc_id };
            let counter = self.counter_for_key(key).await;
            let mut current = counter.load(Ordering::SeqCst);

            while max_server_seq > current {
                match counter.compare_exchange(
                    current,
                    max_server_seq,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                ) {
                    Ok(_) => return,
                    Err(next_current) => current = next_current,
                }
            }
        }
        .instrument(info_span!(
            "relay.sequencer.seed_counter",
            workspace_id = %workspace_id,
            doc_id = %doc_id,
            max_server_seq
        ))
        .await
    }

    pub async fn sequence_update(&self, update: PendingUpdate) -> SequencedUpdate {
        async move {
            let server_seq = self.next_server_seq(update.workspace_id, update.doc_id).await;

            SequencedUpdate {
                workspace_id: update.workspace_id,
                doc_id: update.doc_id,
                server_seq,
                client_id: update.client_id,
                client_update_id: update.client_update_id,
                payload: update.payload,
            }
        }
        .instrument(info_span!(
            "relay.sequencer.sequence_update",
            workspace_id = %update.workspace_id,
            doc_id = %update.doc_id,
            client_id = %update.client_id,
            client_update_id = %update.client_update_id
        ))
        .await
    }

    pub async fn recover_from_max_server_seq(&self, pool: &PgPool) -> Result<()> {
        async {
            let rows = sqlx::query_as::<_, MaxSeqRow>(
                "
                WITH max_update_seq AS (
                    SELECT workspace_id, doc_id, MAX(server_seq) AS max_server_seq
                    FROM yjs_update_log
                    GROUP BY workspace_id, doc_id
                ),
                max_snapshot_seq AS (
                    SELECT workspace_id, doc_id, MAX(snapshot_seq) AS max_server_seq
                    FROM yjs_snapshots
                    GROUP BY workspace_id, doc_id
                )
                SELECT workspace_id, doc_id, MAX(max_server_seq) AS max_server_seq
                FROM (
                    SELECT workspace_id, doc_id, max_server_seq
                    FROM max_update_seq
                    UNION ALL
                    SELECT workspace_id, doc_id, max_server_seq
                    FROM max_snapshot_seq
                ) AS combined
                GROUP BY workspace_id, doc_id
                ",
            )
            .fetch_all(pool)
            .instrument(info_span!("relay.db.query", query = "recover_max_server_seq"))
            .await
            .context("failed to query max server_seq values")?;

            for row in rows {
                self.seed_counter(row.workspace_id, row.doc_id, row.max_server_seq).await;
            }

            Ok(())
        }
        .instrument(info_span!("relay.sequencer.recover_from_max_server_seq"))
        .await
    }

    pub async fn flush_batch_to_postgres(
        &self,
        pool: &PgPool,
        updates: &[SequencedUpdate],
    ) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }

        async {
            let mut builder = QueryBuilder::<Postgres>::new(
                "
                INSERT INTO yjs_update_log
                    (workspace_id, doc_id, server_seq, client_id, client_update_id, payload)
                ",
            );

            builder.push_values(updates, |mut row, update| {
                row.push_bind(update.workspace_id)
                    .push_bind(update.doc_id)
                    .push_bind(update.server_seq)
                    .push_bind(update.client_id)
                    .push_bind(update.client_update_id)
                    .push_bind(update.payload.as_slice());
            });

            builder.push(
                "
                ON CONFLICT (workspace_id, doc_id, client_id, client_update_id)
                DO NOTHING
                ",
            );

            builder
                .build()
                .execute(pool)
                .instrument(info_span!(
                    "relay.db.query",
                    query = "flush_batch_to_postgres",
                    update_count = updates.len()
                ))
                .await
                .context("failed to flush sequenced updates to postgres")?;

            Ok(())
        }
        .instrument(info_span!(
            "relay.sequencer.flush_batch_to_postgres",
            update_count = updates.len()
        ))
        .await
    }

    async fn counter_for_key(&self, key: SequencerKey) -> Arc<AtomicI64> {
        if let Some(existing) = self.counters.read().await.get(&key).cloned() {
            return existing;
        }

        let mut counters = self.counters.write().await;
        counters.entry(key).or_insert_with(|| Arc::new(AtomicI64::new(0))).clone()
    }
}

#[cfg(test)]
mod tests {
    use super::{PendingUpdate, UpdateSequencer};
    use uuid::Uuid;

    #[tokio::test]
    async fn assigns_monotonic_sequences_per_document() {
        let sequencer = UpdateSequencer::new();
        let workspace_id = Uuid::new_v4();
        let doc_a = Uuid::new_v4();
        let doc_b = Uuid::new_v4();

        assert_eq!(sequencer.next_server_seq(workspace_id, doc_a).await, 1);
        assert_eq!(sequencer.next_server_seq(workspace_id, doc_a).await, 2);
        assert_eq!(sequencer.next_server_seq(workspace_id, doc_b).await, 1);
    }

    #[tokio::test]
    async fn seed_counter_recovers_without_regression() {
        let sequencer = UpdateSequencer::new();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();

        sequencer.seed_counter(workspace_id, doc_id, 10).await;
        assert_eq!(sequencer.next_server_seq(workspace_id, doc_id).await, 11);

        sequencer.seed_counter(workspace_id, doc_id, 5).await;
        assert_eq!(sequencer.next_server_seq(workspace_id, doc_id).await, 12);
    }

    #[tokio::test]
    async fn sequence_update_assigns_server_seq() {
        let sequencer = UpdateSequencer::new();
        let pending = PendingUpdate {
            workspace_id: Uuid::new_v4(),
            doc_id: Uuid::new_v4(),
            client_id: Uuid::new_v4(),
            client_update_id: Uuid::new_v4(),
            payload: vec![1, 2, 3],
        };

        let first = sequencer.sequence_update(pending.clone()).await;
        let second = sequencer.sequence_update(pending).await;

        assert_eq!(first.server_seq, 1);
        assert_eq!(second.server_seq, 2);
    }
}
