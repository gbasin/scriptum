use crate::auth::middleware::WorkspaceRole;
use crate::awareness::AwarenessStore;
use crate::db::pool::{check_pool_health, create_pg_pool, PoolConfig};
use crate::metrics;
use anyhow::Context;
use chrono::{Duration, Utc};
use scriptum_common::protocol::ws::WsMessage;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    env,
    sync::Arc,
};
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

pub(crate) const HEARTBEAT_INTERVAL_MS: u32 = 15_000;
pub(crate) const HEARTBEAT_TIMEOUT_MS: u64 = 10_000;
pub(crate) const MAX_FRAME_BYTES: u32 = 262_144;
pub(crate) const SESSION_TOKEN_TTL_MINUTES: i64 = 15;
pub(crate) const RESUME_TOKEN_TTL_MINUTES: i64 = 10;

#[derive(Clone)]
pub(crate) struct SyncSessionRouterState {
    pub(crate) session_store: Arc<SyncSessionStore>,
    pub(crate) doc_store: Arc<DocSyncStore>,
    pub(crate) awareness_store: Arc<AwarenessStore>,
    pub(crate) membership_store: WorkspaceMembershipStore,
    pub(crate) ws_base_url: Arc<str>,
}

#[derive(Clone)]
pub enum WorkspaceMembershipStore {
    Postgres(sqlx::PgPool),
    #[cfg_attr(not(test), allow(dead_code))]
    Memory(Arc<RwLock<HashMap<(Uuid, Uuid), WorkspaceRole>>>),
}

impl WorkspaceMembershipStore {
    pub async fn from_env() -> anyhow::Result<Self> {
        let database_url = env::var("SCRIPTUM_RELAY_DATABASE_URL")
            .context("SCRIPTUM_RELAY_DATABASE_URL must be set for WebSocket RBAC")?;
        let pool = create_pg_pool(&database_url, PoolConfig::from_env())
            .await
            .context("failed to initialize relay PostgreSQL pool for websocket RBAC")?;
        check_pool_health(&pool)
            .await
            .context("relay PostgreSQL health check failed for websocket RBAC")?;

        Ok(Self::Postgres(pool))
    }

    pub(crate) async fn role_for_user(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
    ) -> anyhow::Result<Option<WorkspaceRole>> {
        match self {
            Self::Postgres(pool) => {
                let role = sqlx::query_scalar::<_, String>(
                    r#"
                    SELECT wm.role
                    FROM workspace_members AS wm
                    INNER JOIN workspaces AS w
                        ON w.id = wm.workspace_id
                    WHERE wm.workspace_id = $1
                      AND wm.user_id = $2
                      AND wm.status = 'active'
                      AND w.deleted_at IS NULL
                    "#,
                )
                .bind(workspace_id)
                .bind(user_id)
                .fetch_optional(pool)
                .await
                .context("failed to query workspace role for websocket session")?
                .map(|role| {
                    WorkspaceRole::from_db_value(&role).ok_or_else(|| {
                        anyhow::anyhow!("invalid workspace role '{role}' in database")
                    })
                })
                .transpose()?;

                Ok(role)
            }
            Self::Memory(store) => Ok(store.read().await.get(&(workspace_id, user_id)).copied()),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_tests() -> Self {
        Self::Memory(Arc::new(RwLock::new(HashMap::new())))
    }

    #[cfg(test)]
    pub(crate) async fn grant_for_tests(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
        role: WorkspaceRole,
    ) {
        if let Self::Memory(store) = self {
            store.write().await.insert((workspace_id, user_id), role);
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SyncSessionStore {
    sessions: Arc<RwLock<HashMap<Uuid, SyncSessionRecord>>>,
}

#[derive(Debug, Clone, Default)]
pub struct DocSyncStore {
    docs: Arc<RwLock<HashMap<(Uuid, Uuid), DocSyncState>>>,
}

#[derive(Debug, Clone, Default)]
struct DocSyncState {
    snapshot: Option<DocSnapshotState>,
    updates: Vec<DocUpdateState>,
    dedupe: HashMap<(Uuid, Uuid), i64>,
    head_server_seq: i64,
}

#[derive(Debug, Clone)]
struct DocSnapshotState {
    snapshot_seq: i64,
    payload_b64: String,
}

#[derive(Debug, Clone)]
struct DocUpdateState {
    server_seq: i64,
    client_id: Uuid,
    client_update_id: Uuid,
    payload_b64: String,
    actor_user_id: Option<Uuid>,
    actor_agent_id: Option<String>,
}

pub(crate) enum ApplyClientUpdateResult {
    Applied { server_seq: i64, broadcast_base_server_seq: i64 },
    Duplicate { server_seq: i64 },
    RejectedBaseSeq { server_seq: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UpdateAttribution {
    pub(crate) user_id: Option<Uuid>,
    pub(crate) agent_id: Option<String>,
}

#[derive(Debug, Clone)]
struct SyncSessionRecord {
    workspace_id: Uuid,
    client_id: Uuid,
    device_id: Uuid,
    session_token: String,
    resume_token: String,
    expires_at: chrono::DateTime<Utc>,
    resume_expires_at: chrono::DateTime<Utc>,
    active_connections: usize,
    subscriptions: HashSet<Uuid>,
    outbound: Option<mpsc::UnboundedSender<WsMessage>>,
    actor_user_id: Option<Uuid>,
    actor_agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSyncSessionRequest {
    pub protocol: String,
    pub client_id: Uuid,
    pub device_id: Uuid,
    #[serde(default)]
    pub resume_token: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateSyncSessionResponse {
    pub session_id: Uuid,
    pub session_token: String,
    pub ws_url: String,
    pub heartbeat_interval_ms: u32,
    pub max_frame_bytes: u32,
    pub resume_token: String,
    pub resume_expires_at: String,
}


#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionTokenValidation {
    Valid { resume_accepted: bool, resume_token: String, resume_expires_at: chrono::DateTime<Utc> },
    Invalid,
    Expired,
}

impl DocSyncStore {
    pub async fn set_snapshot(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        snapshot_seq: i64,
        payload_b64: String,
    ) {
        let mut guard = self.docs.write().await;
        let state = guard.entry((workspace_id, doc_id)).or_default();
        state.snapshot = Some(DocSnapshotState { snapshot_seq, payload_b64 });
        state.head_server_seq = state.head_server_seq.max(snapshot_seq);
    }

    pub async fn append_update(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        server_seq: i64,
        client_id: Uuid,
        client_update_id: Uuid,
        payload_b64: String,
    ) {
        let mut guard = self.docs.write().await;
        {
            let state = guard.entry((workspace_id, doc_id)).or_default();
            let previous_head_server_seq = state.head_server_seq;
            if server_seq > previous_head_server_seq.saturating_add(1) {
                metrics::increment_sequence_gap_count();
            }

            state.updates.push(DocUpdateState {
                server_seq,
                client_id,
                client_update_id,
                payload_b64,
                actor_user_id: None,
                actor_agent_id: None,
            });
            state.dedupe.insert((client_id, client_update_id), server_seq);
            state.head_server_seq = state.head_server_seq.max(server_seq);
            state.updates.sort_by_key(|update| update.server_seq);
        }

        let outbox_depth = workspace_outbox_depth(&guard, workspace_id);
        metrics::set_outbox_depth_for_workspace(workspace_id, outbox_depth);
    }

    pub(crate) async fn apply_client_update(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        client_id: Uuid,
        client_update_id: Uuid,
        base_server_seq: i64,
        payload_b64: String,
        attribution: UpdateAttribution,
    ) -> ApplyClientUpdateResult {
        let mut guard = self.docs.write().await;
        let result = {
            let state = guard.entry((workspace_id, doc_id)).or_default();

            if let Some(existing_server_seq) =
                state.dedupe.get(&(client_id, client_update_id)).copied()
            {
                return ApplyClientUpdateResult::Duplicate { server_seq: existing_server_seq };
            }

            if base_server_seq > state.head_server_seq {
                return ApplyClientUpdateResult::RejectedBaseSeq {
                    server_seq: state.head_server_seq,
                };
            }

            let next_server_seq = state.head_server_seq.saturating_add(1);
            state.head_server_seq = next_server_seq;
            state.dedupe.insert((client_id, client_update_id), next_server_seq);
            state.updates.push(DocUpdateState {
                server_seq: next_server_seq,
                client_id,
                client_update_id,
                payload_b64,
                actor_user_id: attribution.user_id,
                actor_agent_id: attribution.agent_id,
            });
            state.updates.sort_by_key(|update| update.server_seq);

            ApplyClientUpdateResult::Applied {
                server_seq: next_server_seq,
                broadcast_base_server_seq: next_server_seq.saturating_sub(1),
            }
        };

        let outbox_depth = workspace_outbox_depth(&guard, workspace_id);
        metrics::set_outbox_depth_for_workspace(workspace_id, outbox_depth);
        result
    }

    pub(crate) async fn build_state_sync_messages(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        last_server_seq: Option<i64>,
    ) -> Vec<WsMessage> {
        let state = self.docs.read().await.get(&(workspace_id, doc_id)).cloned();
        let Some(state) = state else {
            return Vec::new();
        };

        let mut messages = Vec::new();
        let mut cursor_seq = last_server_seq.unwrap_or(0);

        if let Some(snapshot) = state.snapshot {
            if last_server_seq.is_none()
                || last_server_seq.is_some_and(|seq| seq < snapshot.snapshot_seq)
            {
                cursor_seq = snapshot.snapshot_seq;
                messages.push(WsMessage::Snapshot {
                    doc_id,
                    snapshot_seq: snapshot.snapshot_seq,
                    payload_b64: snapshot.payload_b64,
                });
            }
        }

        for update in state.updates.into_iter().filter(|update| update.server_seq > cursor_seq) {
            messages.push(WsMessage::YjsUpdate {
                doc_id,
                client_id: update.client_id,
                client_update_id: update.client_update_id,
                base_server_seq: update.server_seq.saturating_sub(1),
                payload_b64: update.payload_b64,
            });
        }

        messages
    }

    pub(crate) async fn update_attribution(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        client_id: Uuid,
        client_update_id: Uuid,
    ) -> Option<UpdateAttribution> {
        let guard = self.docs.read().await;
        let state = guard.get(&(workspace_id, doc_id))?;
        let update = state.updates.iter().find(|entry| {
            entry.client_id == client_id && entry.client_update_id == client_update_id
        })?;

        Some(UpdateAttribution {
            user_id: update.actor_user_id,
            agent_id: update.actor_agent_id.clone(),
        })
    }
}

fn workspace_outbox_depth(docs: &HashMap<(Uuid, Uuid), DocSyncState>, workspace_id: Uuid) -> i64 {
    docs.iter()
        .filter(|((doc_workspace_id, _), _)| *doc_workspace_id == workspace_id)
        .map(|(_, state)| state.updates.len() as i64)
        .sum()
}

impl SyncSessionStore {
    pub(crate) async fn create_session(
        &self,
        session_id: Uuid,
        workspace_id: Uuid,
        client_id: Uuid,
        device_id: Uuid,
        session_token: String,
        resume_token: String,
        expires_at: chrono::DateTime<Utc>,
        resume_expires_at: chrono::DateTime<Utc>,
    ) {
        self.create_session_with_actor(
            session_id,
            workspace_id,
            client_id,
            device_id,
            session_token,
            resume_token,
            expires_at,
            resume_expires_at,
            None,
            None,
        )
        .await;
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn create_session_with_actor(
        &self,
        session_id: Uuid,
        workspace_id: Uuid,
        client_id: Uuid,
        device_id: Uuid,
        session_token: String,
        resume_token: String,
        expires_at: chrono::DateTime<Utc>,
        resume_expires_at: chrono::DateTime<Utc>,
        actor_user_id: Option<Uuid>,
        actor_agent_id: Option<String>,
    ) {
        let mut guard = self.sessions.write().await;
        guard.insert(
            session_id,
            SyncSessionRecord {
                workspace_id,
                client_id,
                device_id,
                session_token,
                resume_token,
                expires_at,
                resume_expires_at,
                active_connections: 0,
                subscriptions: HashSet::new(),
                outbound: None,
                actor_user_id,
                actor_agent_id,
            },
        );
    }

    pub(crate) async fn session_exists(&self, session_id: Uuid) -> bool {
        self.sessions.read().await.contains_key(&session_id)
    }

    pub(crate) async fn validate_session_token(
        &self,
        session_id: Uuid,
        session_token: &str,
        resume_token: Option<&str>,
    ) -> SessionTokenValidation {
        let mut guard = self.sessions.write().await;
        let Some(session) = guard.get_mut(&session_id) else {
            return SessionTokenValidation::Invalid;
        };

        if session.session_token != session_token {
            return SessionTokenValidation::Invalid;
        }

        if Utc::now() > session.expires_at {
            return SessionTokenValidation::Expired;
        }

        let now = Utc::now();
        let resume_accepted = match resume_token {
            Some(token) if token == session.resume_token => now <= session.resume_expires_at,
            _ => false,
        };
        let next_resume_token = Uuid::new_v4().to_string();
        let next_resume_expires_at = now + Duration::minutes(RESUME_TOKEN_TTL_MINUTES);
        session.resume_token = next_resume_token.clone();
        session.resume_expires_at = next_resume_expires_at;

        SessionTokenValidation::Valid {
            resume_accepted,
            resume_token: next_resume_token,
            resume_expires_at: next_resume_expires_at,
        }
    }

    pub(crate) async fn mark_connected(&self, session_id: Uuid) -> bool {
        let mut guard = self.sessions.write().await;
        match guard.get_mut(&session_id) {
            Some(session) => {
                session.active_connections += 1;
                true
            }
            None => false,
        }
    }

    pub(crate) async fn mark_disconnected(&self, session_id: Uuid) {
        let mut guard = self.sessions.write().await;
        if let Some(session) = guard.get_mut(&session_id) {
            session.active_connections = session.active_connections.saturating_sub(1);
            if session.active_connections == 0 {
                session.subscriptions.clear();
                session.outbound = None;
            }
        }
    }

    pub(crate) async fn register_outbound(
        &self,
        session_id: Uuid,
        sender: mpsc::UnboundedSender<WsMessage>,
    ) -> bool {
        let mut guard = self.sessions.write().await;
        match guard.get_mut(&session_id) {
            Some(session) => {
                session.outbound = Some(sender);
                true
            }
            None => false,
        }
    }

    pub(crate) async fn track_subscription(&self, session_id: Uuid, doc_id: Uuid) -> bool {
        let mut guard = self.sessions.write().await;
        match guard.get_mut(&session_id) {
            Some(session) => {
                session.subscriptions.insert(doc_id);
                true
            }
            None => false,
        }
    }

    pub(crate) async fn session_is_subscribed(&self, session_id: Uuid, doc_id: Uuid) -> bool {
        self.sessions
            .read()
            .await
            .get(&session_id)
            .map(|session| session.subscriptions.contains(&doc_id))
            .unwrap_or(false)
    }

    pub(crate) async fn attribution_for_session(
        &self,
        session_id: Uuid,
    ) -> Option<UpdateAttribution> {
        self.sessions.read().await.get(&session_id).map(|session| UpdateAttribution {
            user_id: session.actor_user_id,
            agent_id: session.actor_agent_id.clone(),
        })
    }

    pub(crate) async fn broadcast_to_subscribers(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        message: WsMessage,
    ) -> usize {
        let mut recipients = Vec::new();
        {
            let guard = self.sessions.read().await;
            for (session_id, session) in guard.iter() {
                if session.workspace_id == workspace_id && session.subscriptions.contains(&doc_id) {
                    if let Some(sender) = session.outbound.clone() {
                        recipients.push((*session_id, sender));
                    }
                }
            }
        }

        let mut sent_count = 0;
        for (_session_id, recipient) in recipients {
            if recipient.send(message.clone()).is_ok() {
                sent_count += 1;
            }
        }

        sent_count
    }

    /// Broadcast to all doc subscribers except the sender session.
    pub(crate) async fn broadcast_to_subscribers_excluding(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        message: WsMessage,
        exclude_session: Uuid,
    ) -> usize {
        let mut recipients = Vec::new();
        {
            let guard = self.sessions.read().await;
            for (session_id, session) in guard.iter() {
                if *session_id == exclude_session {
                    continue;
                }
                if session.workspace_id == workspace_id && session.subscriptions.contains(&doc_id) {
                    if let Some(sender) = session.outbound.clone() {
                        recipients.push((*session_id, sender));
                    }
                }
            }
        }

        let mut sent_count = 0;
        for (_session_id, recipient) in recipients {
            if recipient.send(message.clone()).is_ok() {
                sent_count += 1;
            }
        }

        sent_count
    }

    pub(crate) async fn active_connections(&self, session_id: Uuid) -> Option<usize> {
        self.sessions.read().await.get(&session_id).map(|session| session.active_connections)
    }

    pub(crate) async fn workspace_for_session(&self, session_id: Uuid) -> Option<Uuid> {
        self.sessions.read().await.get(&session_id).map(|session| session.workspace_id)
    }

    pub(crate) async fn actor_user_for_session(&self, session_id: Uuid) -> Option<Uuid> {
        self.sessions.read().await.get(&session_id).and_then(|session| session.actor_user_id)
    }

    pub(crate) async fn token_for_session(&self, session_id: Uuid) -> Option<String> {
        self.sessions.read().await.get(&session_id).map(|session| session.session_token.clone())
    }

    pub(crate) async fn subscriptions_for_session(&self, session_id: Uuid) -> Option<Vec<Uuid>> {
        self.sessions.read().await.get(&session_id).map(|session| {
            let mut subscriptions = session.subscriptions.iter().copied().collect::<Vec<_>>();
            subscriptions.sort();
            subscriptions
        })
    }
}
