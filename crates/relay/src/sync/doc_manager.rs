use std::collections::{hash_map::Entry, HashMap};
use std::time::{Duration, Instant};

use sqlx::postgres::PgPool;
use tokio::sync::RwLock;
use uuid::Uuid;
use yrs::updates::decoder::Decode;
use yrs::{Doc, GetString, Transact, Update};

const DEFAULT_INACTIVE_AFTER: Duration = Duration::from_secs(300);

#[derive(Debug, Clone)]
pub struct StoredSnapshot {
    pub snapshot_seq: i64,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct StoredUpdate {
    pub server_seq: i64,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct DocHistory {
    pub snapshot: Option<StoredSnapshot>,
    pub updates: Vec<StoredUpdate>,
}

#[derive(Debug)]
pub enum DocManagerError {
    SnapshotQuery { source: sqlx::Error },

    UpdateQuery { source: sqlx::Error },

    InvalidSnapshotPayload { workspace_id: Uuid, doc_id: Uuid, snapshot_seq: i64 },

    InvalidUpdatePayload { workspace_id: Uuid, doc_id: Uuid, server_seq: i64 },

    DocumentNotLoaded { workspace_id: Uuid, doc_id: Uuid },

    BaseServerSeqAhead { base_server_seq: i64, head_server_seq: i64 },

    NonMonotonicServerSeq { server_seq: i64, head_server_seq: i64 },
}

impl std::fmt::Display for DocManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SnapshotQuery { source } => {
                write!(f, "failed to load latest snapshot from postgres: {source}")
            }
            Self::UpdateQuery { source } => {
                write!(f, "failed to load update rows from postgres: {source}")
            }
            Self::InvalidSnapshotPayload {
                workspace_id,
                doc_id,
                snapshot_seq,
            } => write!(
                f,
                "invalid snapshot payload for workspace {workspace_id} doc {doc_id} at snapshot_seq {snapshot_seq}"
            ),
            Self::InvalidUpdatePayload {
                workspace_id,
                doc_id,
                server_seq,
            } => write!(
                f,
                "invalid update payload for workspace {workspace_id} doc {doc_id} at server_seq {server_seq}"
            ),
            Self::DocumentNotLoaded { workspace_id, doc_id } => {
                write!(f, "document workspace {workspace_id} doc {doc_id} is not loaded")
            }
            Self::BaseServerSeqAhead {
                base_server_seq,
                head_server_seq,
            } => write!(
                f,
                "base_server_seq {base_server_seq} exceeds current head {head_server_seq}"
            ),
            Self::NonMonotonicServerSeq {
                server_seq,
                head_server_seq,
            } => write!(
                f,
                "server_seq {server_seq} must be greater than current head {head_server_seq}"
            ),
        }
    }
}

impl std::error::Error for DocManagerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::SnapshotQuery { source } => Some(source),
            Self::UpdateQuery { source } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct SnapshotRow {
    snapshot_seq: i64,
    payload: Vec<u8>,
}

#[derive(Debug, sqlx::FromRow)]
struct UpdateRow {
    server_seq: i64,
    payload: Vec<u8>,
}

#[derive(Debug)]
struct LoadedDoc {
    doc: Doc,
    head_server_seq: i64,
    subscribers: usize,
    last_touched: Instant,
}

#[derive(Debug)]
struct HydratedDoc {
    doc: Doc,
    head_server_seq: i64,
}

pub struct DocManager {
    docs: RwLock<HashMap<(Uuid, Uuid), LoadedDoc>>,
    inactive_after: Duration,
}

impl Default for DocManager {
    fn default() -> Self {
        Self::new(DEFAULT_INACTIVE_AFTER)
    }
}

impl DocManager {
    pub fn new(inactive_after: Duration) -> Self {
        Self { docs: RwLock::new(HashMap::new()), inactive_after }
    }

    pub async fn subscribe(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        history: DocHistory,
    ) -> Result<i64, DocManagerError> {
        let key = (workspace_id, doc_id);
        {
            let mut docs = self.docs.write().await;
            if let Some(existing) = docs.get_mut(&key) {
                existing.subscribers += 1;
                existing.last_touched = Instant::now();
                return Ok(existing.head_server_seq);
            }
        }

        let hydrated = hydrate_document(workspace_id, doc_id, history)?;
        let mut docs = self.docs.write().await;
        let now = Instant::now();

        match docs.entry(key) {
            Entry::Occupied(mut occupied) => {
                let existing = occupied.get_mut();
                existing.subscribers += 1;
                existing.last_touched = now;
                Ok(existing.head_server_seq)
            }
            Entry::Vacant(vacant) => {
                let head_server_seq = hydrated.head_server_seq;
                vacant.insert(LoadedDoc {
                    doc: hydrated.doc,
                    head_server_seq,
                    subscribers: 1,
                    last_touched: now,
                });
                Ok(head_server_seq)
            }
        }
    }

    pub async fn unsubscribe(&self, workspace_id: Uuid, doc_id: Uuid) -> bool {
        let mut docs = self.docs.write().await;
        if let Some(loaded) = docs.get_mut(&(workspace_id, doc_id)) {
            loaded.subscribers = loaded.subscribers.saturating_sub(1);
            loaded.last_touched = Instant::now();
            true
        } else {
            false
        }
    }

    pub async fn apply_sequenced_update(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        base_server_seq: i64,
        server_seq: i64,
        payload: &[u8],
    ) -> Result<(), DocManagerError> {
        let mut docs = self.docs.write().await;
        let loaded = docs
            .get_mut(&(workspace_id, doc_id))
            .ok_or(DocManagerError::DocumentNotLoaded { workspace_id, doc_id })?;

        if base_server_seq > loaded.head_server_seq {
            return Err(DocManagerError::BaseServerSeqAhead {
                base_server_seq,
                head_server_seq: loaded.head_server_seq,
            });
        }
        if server_seq <= loaded.head_server_seq {
            return Err(DocManagerError::NonMonotonicServerSeq {
                server_seq,
                head_server_seq: loaded.head_server_seq,
            });
        }

        apply_update_payload(&loaded.doc, payload).map_err(|_| {
            DocManagerError::InvalidUpdatePayload { workspace_id, doc_id, server_seq }
        })?;

        loaded.head_server_seq = server_seq;
        loaded.last_touched = Instant::now();
        Ok(())
    }

    pub async fn unload_inactive(&self) -> Vec<(Uuid, Uuid)> {
        let now = Instant::now();
        let mut docs = self.docs.write().await;
        let mut unloaded = Vec::new();

        docs.retain(|(workspace_id, doc_id), loaded| {
            let should_unload = loaded.subscribers == 0
                && now.duration_since(loaded.last_touched) >= self.inactive_after;
            if should_unload {
                unloaded.push((*workspace_id, *doc_id));
                false
            } else {
                true
            }
        });

        unloaded
    }

    pub async fn head_server_seq(&self, workspace_id: Uuid, doc_id: Uuid) -> Option<i64> {
        self.docs.read().await.get(&(workspace_id, doc_id)).map(|loaded| loaded.head_server_seq)
    }

    pub async fn text_content(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        text_name: &str,
    ) -> Option<String> {
        let docs = self.docs.read().await;
        let loaded = docs.get(&(workspace_id, doc_id))?;
        let text = loaded.doc.get_or_insert_text(text_name);
        let txn = loaded.doc.transact();
        let content = text.get_string(&txn);
        Some(content)
    }
}

pub async fn load_doc_history(
    pool: &PgPool,
    workspace_id: Uuid,
    doc_id: Uuid,
) -> Result<DocHistory, DocManagerError> {
    let snapshot = sqlx::query_as::<_, SnapshotRow>(
        "
        SELECT snapshot_seq, payload
        FROM yjs_snapshots
        WHERE workspace_id = $1 AND doc_id = $2
        ORDER BY snapshot_seq DESC
        LIMIT 1
        ",
    )
    .bind(workspace_id)
    .bind(doc_id)
    .fetch_optional(pool)
    .await
    .map_err(|source| DocManagerError::SnapshotQuery { source })?
    .map(|row| StoredSnapshot { snapshot_seq: row.snapshot_seq, payload: row.payload });

    let min_server_seq = snapshot.as_ref().map(|entry| entry.snapshot_seq).unwrap_or(0);
    let updates = sqlx::query_as::<_, UpdateRow>(
        "
        SELECT server_seq, payload
        FROM yjs_update_log
        WHERE workspace_id = $1
          AND doc_id = $2
          AND server_seq > $3
        ORDER BY server_seq ASC
        ",
    )
    .bind(workspace_id)
    .bind(doc_id)
    .bind(min_server_seq)
    .fetch_all(pool)
    .await
    .map_err(|source| DocManagerError::UpdateQuery { source })?
    .into_iter()
    .map(|row| StoredUpdate { server_seq: row.server_seq, payload: row.payload })
    .collect();

    Ok(DocHistory { snapshot, updates })
}

fn hydrate_document(
    workspace_id: Uuid,
    doc_id: Uuid,
    history: DocHistory,
) -> Result<HydratedDoc, DocManagerError> {
    let doc = Doc::new();
    let mut head_server_seq = 0;

    if let Some(snapshot) = history.snapshot {
        apply_update_payload(&doc, &snapshot.payload).map_err(|_| {
            DocManagerError::InvalidSnapshotPayload {
                workspace_id,
                doc_id,
                snapshot_seq: snapshot.snapshot_seq,
            }
        })?;
        head_server_seq = snapshot.snapshot_seq;
    }

    let mut updates = history.updates;
    updates.sort_by_key(|update| update.server_seq);
    for update in updates {
        if update.server_seq <= head_server_seq {
            continue;
        }

        apply_update_payload(&doc, &update.payload).map_err(|_| {
            DocManagerError::InvalidUpdatePayload {
                workspace_id,
                doc_id,
                server_seq: update.server_seq,
            }
        })?;
        head_server_seq = update.server_seq;
    }

    Ok(HydratedDoc { doc, head_server_seq })
}

fn apply_update_payload(doc: &Doc, payload: &[u8]) -> Result<(), ()> {
    let decoded = Update::decode_v1(payload).map_err(|_| ())?;
    doc.transact_mut().apply_update(decoded).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::{DocHistory, DocManager, DocManagerError, StoredSnapshot, StoredUpdate};
    use std::time::Duration;
    use uuid::Uuid;
    use yrs::{Doc, ReadTxn, StateVector, Text, Transact};

    fn doc_with_content(content: &str, client_id: u64) -> Doc {
        let options = yrs::Options { client_id, ..Default::default() };
        let doc = Doc::with_options(options);
        let text = doc.get_or_insert_text("content");
        let mut txn = doc.transact_mut();
        text.insert(&mut txn, 0, content);
        drop(txn);
        doc
    }

    fn encode_full_state(doc: &Doc) -> Vec<u8> {
        doc.transact().encode_state_as_update_v1(&StateVector::default())
    }

    fn encode_insert_update(base_content: &str, insert_at: u32, inserted_text: &str) -> Vec<u8> {
        let base_doc = doc_with_content(base_content, 7);
        let updated_doc = doc_with_content(base_content, 7);
        {
            let text = updated_doc.get_or_insert_text("content");
            let mut txn = updated_doc.transact_mut();
            text.insert(&mut txn, insert_at, inserted_text);
        }

        let base_state_vector = base_doc.transact().state_vector();
        let diff = updated_doc.transact().encode_diff_v1(&base_state_vector);
        diff
    }

    #[tokio::test]
    async fn subscribe_hydrates_from_snapshot_and_update_log() {
        let manager = DocManager::new(Duration::from_secs(60));
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        let snapshot_doc = doc_with_content("hello", 7);
        let update_payload = encode_insert_update("hello", 5, " world");
        let history = DocHistory {
            snapshot: Some(StoredSnapshot {
                snapshot_seq: 10,
                payload: encode_full_state(&snapshot_doc),
            }),
            updates: vec![StoredUpdate { server_seq: 11, payload: update_payload }],
        };

        let head_server_seq =
            manager.subscribe(workspace_id, doc_id, history).await.expect("subscribe should load");

        assert_eq!(head_server_seq, 11);
        assert_eq!(
            manager.text_content(workspace_id, doc_id, "content").await.as_deref(),
            Some("hello world")
        );
        assert_eq!(manager.head_server_seq(workspace_id, doc_id).await, Some(11));
    }

    #[tokio::test]
    async fn apply_sequenced_update_rejects_invalid_payload() {
        let manager = DocManager::new(Duration::from_secs(60));
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        manager
            .subscribe(workspace_id, doc_id, DocHistory::default())
            .await
            .expect("subscribe should initialize an empty doc");

        let error = manager
            .apply_sequenced_update(workspace_id, doc_id, 0, 1, b"not-a-valid-yjs-update")
            .await
            .expect_err("invalid update payload should be rejected");

        assert!(matches!(
            error,
            DocManagerError::InvalidUpdatePayload { workspace_id: ws, doc_id: doc, server_seq: 1 }
                if ws == workspace_id && doc == doc_id
        ));
        assert_eq!(manager.head_server_seq(workspace_id, doc_id).await, Some(0));
    }

    #[tokio::test]
    async fn unload_inactive_only_removes_docs_without_subscribers() {
        let manager = DocManager::new(Duration::ZERO);
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        manager
            .subscribe(workspace_id, doc_id, DocHistory::default())
            .await
            .expect("subscribe should initialize an empty doc");

        assert!(
            manager.unload_inactive().await.is_empty(),
            "active subscribers should prevent unload"
        );

        assert!(manager.unsubscribe(workspace_id, doc_id).await);
        let unloaded = manager.unload_inactive().await;

        assert_eq!(unloaded, vec![(workspace_id, doc_id)]);
        assert_eq!(manager.head_server_seq(workspace_id, doc_id).await, None);
    }

    #[tokio::test]
    async fn apply_sequenced_update_rejects_base_seq_ahead_of_head() {
        let manager = DocManager::new(Duration::from_secs(60));
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        manager
            .subscribe(workspace_id, doc_id, DocHistory::default())
            .await
            .expect("subscribe should initialize an empty doc");

        let payload = encode_insert_update("", 0, "hello");
        let error = manager
            .apply_sequenced_update(workspace_id, doc_id, 1, 1, &payload)
            .await
            .expect_err("base server seq ahead of head should be rejected");

        assert!(matches!(
            error,
            DocManagerError::BaseServerSeqAhead { base_server_seq: 1, head_server_seq: 0 }
        ));
    }
}
