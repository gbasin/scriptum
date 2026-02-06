use crate::auth::{
    jwt::JwtAccessTokenService,
    middleware::{require_bearer_auth, AuthenticatedUser, WorkspaceRole},
};
use crate::db::pool::{check_pool_health, create_pg_pool, PoolConfig};
use crate::error::{ErrorCode, RelayError};
use crate::protocol;
use anyhow::Context;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Extension, Path, State,
    },
    http::StatusCode,
    middleware,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{Duration, Utc};
use scriptum_common::protocol::ws::WsMessage;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    env,
    sync::Arc,
};
use tokio::sync::{mpsc, RwLock};
use tracing::error;
use uuid::Uuid;

const HEARTBEAT_INTERVAL_MS: u32 = 15_000;
const MAX_FRAME_BYTES: u32 = 262_144;
const SESSION_TOKEN_TTL_MINUTES: i64 = 15;
const RESUME_TOKEN_TTL_MINUTES: i64 = 10;

#[derive(Clone)]
pub struct SyncSessionRouterState {
    session_store: Arc<SyncSessionStore>,
    doc_store: Arc<DocSyncStore>,
    membership_store: WorkspaceMembershipStore,
    ws_base_url: Arc<str>,
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

    async fn role_for_user(
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

enum ApplyClientUpdateResult {
    Applied { server_seq: i64, broadcast_base_server_seq: i64 },
    Duplicate { server_seq: i64 },
    RejectedBaseSeq { server_seq: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpdateAttribution {
    user_id: Option<Uuid>,
    agent_id: Option<String>,
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

pub fn router(
    jwt_service: Arc<JwtAccessTokenService>,
    session_store: Arc<SyncSessionStore>,
    doc_store: Arc<DocSyncStore>,
    membership_store: WorkspaceMembershipStore,
    ws_base_url: String,
) -> Router {
    let state = SyncSessionRouterState {
        session_store,
        doc_store,
        membership_store,
        ws_base_url: Arc::<str>::from(ws_base_url),
    };
    let auth_layer = middleware::from_fn_with_state(jwt_service, require_bearer_auth);

    Router::new()
        .route(
            "/v1/workspaces/{workspace_id}/sync-sessions",
            post(create_sync_session).route_layer(auth_layer),
        )
        .route("/v1/ws/{session_id}", get(ws_upgrade))
        .with_state(state)
}

pub async fn create_sync_session(
    Path(workspace_id): Path<Uuid>,
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<SyncSessionRouterState>,
    Json(payload): Json<CreateSyncSessionRequest>,
) -> impl IntoResponse {
    if let Err(upgrade_error) = protocol::require_supported(&payload.protocol) {
        return upgrade_error.into_response();
    }

    if workspace_id != user.workspace_id {
        return RelayError::new(ErrorCode::AuthForbidden, "workspace mismatch").into_response();
    }

    let role = match state.membership_store.role_for_user(workspace_id, user.user_id).await {
        Ok(Some(role)) => role,
        Ok(None) => {
            return RelayError::new(ErrorCode::AuthForbidden, "caller lacks workspace access")
                .into_response();
        }
        Err(error) => {
            error!(error = ?error, user_id = %user.user_id, workspace_id = %workspace_id, "failed to evaluate workspace membership");
            return RelayError::from_code(ErrorCode::InternalError).into_response();
        }
    };

    if !role.allows(WorkspaceRole::Viewer) {
        return RelayError::new(ErrorCode::AuthForbidden, "caller lacks required role")
            .into_response();
    }

    let session_id = Uuid::new_v4();
    let session_token = Uuid::new_v4().to_string();
    let session_expires_at = Utc::now() + Duration::minutes(SESSION_TOKEN_TTL_MINUTES);
    let resume_token = payload
        .resume_token
        .filter(|token| !token.is_empty())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let resume_expires_at = Utc::now() + Duration::minutes(RESUME_TOKEN_TTL_MINUTES);
    let resume_expires_at_rfc3339 = resume_expires_at.to_rfc3339();
    let ws_url = format!("{}/v1/ws/{}", state.ws_base_url, session_id);

    state
        .session_store
        .create_session_with_actor(
            session_id,
            workspace_id,
            payload.client_id,
            payload.device_id,
            session_token.clone(),
            resume_token.clone(),
            session_expires_at,
            resume_expires_at,
            Some(user.user_id),
            None,
        )
        .await;

    (
        StatusCode::CREATED,
        Json(CreateSyncSessionResponse {
            session_id,
            session_token,
            ws_url,
            heartbeat_interval_ms: HEARTBEAT_INTERVAL_MS,
            max_frame_bytes: MAX_FRAME_BYTES,
            resume_token,
            resume_expires_at: resume_expires_at_rfc3339,
        }),
    )
        .into_response()
}

pub async fn ws_upgrade(
    Path(session_id): Path<Uuid>,
    State(state): State<SyncSessionRouterState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if !state.session_store.session_exists(session_id).await {
        return RelayError::from_code(ErrorCode::NotFound).into_response();
    }

    let session_store = state.session_store.clone();
    let doc_store = state.doc_store.clone();
    let membership_store = state.membership_store.clone();
    ws.max_frame_size(MAX_FRAME_BYTES as usize).on_upgrade(move |socket| async move {
        handle_socket(session_store, doc_store, membership_store, session_id, socket).await;
    })
}

async fn handle_socket(
    session_store: Arc<SyncSessionStore>,
    doc_store: Arc<DocSyncStore>,
    membership_store: WorkspaceMembershipStore,
    session_id: Uuid,
    mut socket: WebSocket,
) {
    if !session_store.mark_connected(session_id).await {
        return;
    }

    let hello = match socket.recv().await {
        Some(Ok(Message::Text(raw_message))) => {
            match serde_json::from_str::<WsMessage>(&raw_message) {
                Ok(WsMessage::Hello { session_token, resume_token }) => match handle_hello_message(
                    &session_store,
                    session_id,
                    session_token,
                    resume_token,
                )
                .await
                {
                    Ok(hello_ack) => hello_ack,
                    Err(error_message) => {
                        let _ = send_ws_message(&mut socket, &error_message).await;
                        let _ = socket.send(Message::Close(None)).await;
                        session_store.mark_disconnected(session_id).await;
                        return;
                    }
                },
                _ => {
                    let _ = send_ws_message(
                        &mut socket,
                        &WsMessage::Error {
                            code: "SYNC_HELLO_REQUIRED".to_string(),
                            message: "first WebSocket message must be a hello frame".to_string(),
                            retryable: false,
                            doc_id: None,
                        },
                    )
                    .await;
                    let _ = socket.send(Message::Close(None)).await;
                    session_store.mark_disconnected(session_id).await;
                    return;
                }
            }
        }
        _ => {
            session_store.mark_disconnected(session_id).await;
            return;
        }
    };

    if send_ws_message(&mut socket, &hello).await.is_err() {
        session_store.mark_disconnected(session_id).await;
        return;
    }

    let (outbound_sender, mut outbound_receiver) = mpsc::unbounded_channel::<WsMessage>();
    if !session_store.register_outbound(session_id, outbound_sender).await {
        session_store.mark_disconnected(session_id).await;
        return;
    }

    loop {
        tokio::select! {
            maybe_outbound = outbound_receiver.recv() => {
                match maybe_outbound {
                    Some(outbound_message) => {
                        if send_ws_message(&mut socket, &outbound_message).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
            maybe_message = socket.recv() => {
                let Some(message) = maybe_message else {
                    break;
                };

                match message {
                    Ok(Message::Text(raw_message)) => {
                        let inbound = match serde_json::from_str::<WsMessage>(&raw_message) {
                            Ok(message) => message,
                            Err(_) => {
                                if send_ws_message(
                                    &mut socket,
                                    &WsMessage::Error {
                                        code: "SYNC_INVALID_MESSAGE".to_string(),
                                        message: "invalid websocket frame payload".to_string(),
                                        retryable: false,
                                        doc_id: None,
                                    },
                                )
                                .await
                                .is_err()
                                {
                                    break;
                                }
                                continue;
                            }
                        };

                        match inbound {
                            WsMessage::Subscribe { doc_id, last_server_seq } => {
                                match handle_subscribe_message(
                                    &session_store,
                                    &doc_store,
                                    &membership_store,
                                    session_id,
                                    doc_id,
                                    last_server_seq,
                                )
                                .await
                                {
                                    Ok(outbound_messages) => {
                                        let mut send_failed = false;
                                        for outbound in outbound_messages {
                                            if send_ws_message(&mut socket, &outbound).await.is_err() {
                                                send_failed = true;
                                                break;
                                            }
                                        }

                                        if send_failed {
                                            break;
                                        }
                                    }
                                    Err(error_message) => {
                                        if send_ws_message(&mut socket, &error_message).await.is_err() {
                                            break;
                                        }
                                    }
                                }
                            }
                            WsMessage::YjsUpdate {
                                doc_id,
                                client_id,
                                client_update_id,
                                base_server_seq,
                                payload_b64,
                            } => {
                                match handle_yjs_update_message(
                                    &session_store,
                                    &doc_store,
                                    session_id,
                                    doc_id,
                                    client_id,
                                    client_update_id,
                                    base_server_seq,
                                    payload_b64,
                                )
                                .await
                                {
                                    Ok(result) => {
                                        if send_ws_message(&mut socket, &result.ack).await.is_err() {
                                            break;
                                        }
                                        if let Some(broadcast_message) = result.broadcast {
                                            let _ = session_store
                                                .broadcast_to_subscribers(
                                                    result.workspace_id,
                                                    doc_id,
                                                    broadcast_message,
                                                )
                                                .await;
                                        }
                                    }
                                    Err(error_message) => {
                                        if send_ws_message(&mut socket, &error_message).await.is_err() {
                                            break;
                                        }
                                    }
                                }
                            }
                            _ => {
                                if send_ws_message(
                                    &mut socket,
                                    &WsMessage::Error {
                                        code: "SYNC_UNSUPPORTED_MESSAGE".to_string(),
                                        message: "message type is not supported by this relay build"
                                            .to_string(),
                                        retryable: true,
                                        doc_id: None,
                                    },
                                )
                                .await
                                .is_err()
                                {
                                    break;
                                }
                            }
                        }
                    }
                    Ok(Message::Ping(payload)) => {
                        if socket.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        }
    }

    session_store.mark_disconnected(session_id).await;
}

async fn send_ws_message(socket: &mut WebSocket, message: &WsMessage) -> Result<(), ()> {
    let encoded = serde_json::to_string(message).map_err(|_| ())?;
    socket.send(Message::Text(encoded.into())).await.map_err(|_| ())
}

async fn handle_hello_message(
    session_store: &SyncSessionStore,
    session_id: Uuid,
    session_token: String,
    resume_token: Option<String>,
) -> Result<WsMessage, WsMessage> {
    match session_store
        .validate_session_token(session_id, &session_token, resume_token.as_deref())
        .await
    {
        SessionTokenValidation::Valid { resume_accepted } => {
            Ok(WsMessage::HelloAck { server_time: Utc::now().to_rfc3339(), resume_accepted })
        }
        SessionTokenValidation::Invalid => Err(WsMessage::Error {
            code: "SYNC_TOKEN_INVALID".to_string(),
            message: "invalid session token".to_string(),
            retryable: false,
            doc_id: None,
        }),
        SessionTokenValidation::Expired => Err(WsMessage::Error {
            code: "SYNC_TOKEN_EXPIRED".to_string(),
            message: "session token expired".to_string(),
            retryable: false,
            doc_id: None,
        }),
    }
}

async fn handle_subscribe_message(
    session_store: &SyncSessionStore,
    doc_store: &DocSyncStore,
    membership_store: &WorkspaceMembershipStore,
    session_id: Uuid,
    doc_id: Uuid,
    last_server_seq: Option<i64>,
) -> Result<Vec<WsMessage>, WsMessage> {
    if let Some(sequence) = last_server_seq {
        if sequence < 0 {
            return Err(WsMessage::Error {
                code: "SYNC_INVALID_LAST_SERVER_SEQ".to_string(),
                message: "last_server_seq must be >= 0".to_string(),
                retryable: false,
                doc_id: Some(doc_id),
            });
        }
    }

    let Some(workspace_id) = session_store.workspace_for_session(session_id).await else {
        return Err(WsMessage::Error {
            code: "SYNC_SESSION_INVALID".to_string(),
            message: "session is not available".to_string(),
            retryable: false,
            doc_id: Some(doc_id),
        });
    };

    if let Some(actor_user_id) = session_store.actor_user_for_session(session_id).await {
        let role = match membership_store.role_for_user(workspace_id, actor_user_id).await {
            Ok(Some(role)) => role,
            Ok(None) => {
                return Err(WsMessage::Error {
                    code: ErrorCode::AuthForbidden.as_str().to_string(),
                    message: "caller lacks workspace access".to_string(),
                    retryable: false,
                    doc_id: Some(doc_id),
                });
            }
            Err(error) => {
                error!(
                    error = ?error,
                    session_id = %session_id,
                    actor_user_id = %actor_user_id,
                    workspace_id = %workspace_id,
                    "failed to evaluate websocket subscribe permissions",
                );
                return Err(WsMessage::Error {
                    code: ErrorCode::InternalError.as_str().to_string(),
                    message: ErrorCode::InternalError.default_message().to_string(),
                    retryable: true,
                    doc_id: Some(doc_id),
                });
            }
        };

        if !role.allows(WorkspaceRole::Viewer) {
            return Err(WsMessage::Error {
                code: ErrorCode::AuthForbidden.as_str().to_string(),
                message: "caller lacks required role".to_string(),
                retryable: false,
                doc_id: Some(doc_id),
            });
        }
    }

    if !session_store.track_subscription(session_id, doc_id).await {
        return Err(WsMessage::Error {
            code: "SYNC_SESSION_INVALID".to_string(),
            message: "session is not available".to_string(),
            retryable: false,
            doc_id: Some(doc_id),
        });
    }

    Ok(doc_store.build_state_sync_messages(workspace_id, doc_id, last_server_seq).await)
}

#[derive(Debug)]
struct YjsUpdateHandlingResult {
    workspace_id: Uuid,
    ack: WsMessage,
    broadcast: Option<WsMessage>,
}

#[allow(clippy::too_many_arguments)]
async fn handle_yjs_update_message(
    session_store: &SyncSessionStore,
    doc_store: &DocSyncStore,
    session_id: Uuid,
    doc_id: Uuid,
    client_id: Uuid,
    client_update_id: Uuid,
    base_server_seq: i64,
    payload_b64: String,
) -> Result<YjsUpdateHandlingResult, WsMessage> {
    if base_server_seq < 0 {
        return Err(WsMessage::Error {
            code: "SYNC_INVALID_BASE_SERVER_SEQ".to_string(),
            message: "base_server_seq must be >= 0".to_string(),
            retryable: false,
            doc_id: Some(doc_id),
        });
    }

    let Some(workspace_id) = session_store.workspace_for_session(session_id).await else {
        return Err(WsMessage::Error {
            code: "SYNC_SESSION_INVALID".to_string(),
            message: "session is not available".to_string(),
            retryable: false,
            doc_id: Some(doc_id),
        });
    };

    if !session_store.session_is_subscribed(session_id, doc_id).await {
        return Err(WsMessage::Error {
            code: "SYNC_DOC_NOT_SUBSCRIBED".to_string(),
            message: "subscribe before sending yjs_update".to_string(),
            retryable: false,
            doc_id: Some(doc_id),
        });
    }

    let Some(attribution) = session_store.attribution_for_session(session_id).await else {
        return Err(WsMessage::Error {
            code: "SYNC_SESSION_INVALID".to_string(),
            message: "session is not available".to_string(),
            retryable: false,
            doc_id: Some(doc_id),
        });
    };

    let apply_result = doc_store
        .apply_client_update(
            workspace_id,
            doc_id,
            client_id,
            client_update_id,
            base_server_seq,
            payload_b64.clone(),
            attribution,
        )
        .await;

    match apply_result {
        ApplyClientUpdateResult::Applied { server_seq, broadcast_base_server_seq } => {
            Ok(YjsUpdateHandlingResult {
                workspace_id,
                ack: WsMessage::Ack { doc_id, client_update_id, server_seq, applied: true },
                broadcast: Some(WsMessage::YjsUpdate {
                    doc_id,
                    client_id,
                    client_update_id,
                    base_server_seq: broadcast_base_server_seq,
                    payload_b64,
                }),
            })
        }
        ApplyClientUpdateResult::Duplicate { server_seq } => Ok(YjsUpdateHandlingResult {
            workspace_id,
            ack: WsMessage::Ack { doc_id, client_update_id, server_seq, applied: false },
            broadcast: None,
        }),
        ApplyClientUpdateResult::RejectedBaseSeq { server_seq } => Err(WsMessage::Error {
            code: "SYNC_BASE_SERVER_SEQ_MISMATCH".to_string(),
            message: format!("base_server_seq exceeds head server sequence ({server_seq})"),
            retryable: true,
            doc_id: Some(doc_id),
        }),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionTokenValidation {
    Valid { resume_accepted: bool },
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
        let state = guard.entry((workspace_id, doc_id)).or_default();
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

    async fn apply_client_update(
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
        let state = guard.entry((workspace_id, doc_id)).or_default();

        if let Some(existing_server_seq) = state.dedupe.get(&(client_id, client_update_id)).copied()
        {
            return ApplyClientUpdateResult::Duplicate { server_seq: existing_server_seq };
        }

        if base_server_seq > state.head_server_seq {
            return ApplyClientUpdateResult::RejectedBaseSeq { server_seq: state.head_server_seq };
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
    }

    async fn build_state_sync_messages(
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

    async fn update_attribution(
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

impl SyncSessionStore {
    async fn create_session(
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
    async fn create_session_with_actor(
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

    async fn session_exists(&self, session_id: Uuid) -> bool {
        self.sessions.read().await.contains_key(&session_id)
    }

    async fn validate_session_token(
        &self,
        session_id: Uuid,
        session_token: &str,
        resume_token: Option<&str>,
    ) -> SessionTokenValidation {
        let Some(session) = self.sessions.read().await.get(&session_id).cloned() else {
            return SessionTokenValidation::Invalid;
        };

        if session.session_token != session_token {
            return SessionTokenValidation::Invalid;
        }

        if Utc::now() > session.expires_at {
            return SessionTokenValidation::Expired;
        }

        let resume_accepted = match resume_token {
            Some(token) if token == session.resume_token => Utc::now() <= session.resume_expires_at,
            _ => false,
        };

        SessionTokenValidation::Valid { resume_accepted }
    }

    async fn mark_connected(&self, session_id: Uuid) -> bool {
        let mut guard = self.sessions.write().await;
        match guard.get_mut(&session_id) {
            Some(session) => {
                session.active_connections += 1;
                true
            }
            None => false,
        }
    }

    async fn mark_disconnected(&self, session_id: Uuid) {
        let mut guard = self.sessions.write().await;
        if let Some(session) = guard.get_mut(&session_id) {
            session.active_connections = session.active_connections.saturating_sub(1);
            if session.active_connections == 0 {
                session.subscriptions.clear();
                session.outbound = None;
            }
        }
    }

    async fn register_outbound(
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

    async fn track_subscription(&self, session_id: Uuid, doc_id: Uuid) -> bool {
        let mut guard = self.sessions.write().await;
        match guard.get_mut(&session_id) {
            Some(session) => {
                session.subscriptions.insert(doc_id);
                true
            }
            None => false,
        }
    }

    async fn session_is_subscribed(&self, session_id: Uuid, doc_id: Uuid) -> bool {
        self.sessions
            .read()
            .await
            .get(&session_id)
            .map(|session| session.subscriptions.contains(&doc_id))
            .unwrap_or(false)
    }

    async fn attribution_for_session(&self, session_id: Uuid) -> Option<UpdateAttribution> {
        self.sessions.read().await.get(&session_id).map(|session| UpdateAttribution {
            user_id: session.actor_user_id,
            agent_id: session.actor_agent_id.clone(),
        })
    }

    async fn broadcast_to_subscribers(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        message: WsMessage,
    ) -> usize {
        let mut recipients = Vec::new();
        {
            let guard = self.sessions.read().await;
            for session in guard.values() {
                if session.workspace_id == workspace_id && session.subscriptions.contains(&doc_id) {
                    if let Some(sender) = session.outbound.clone() {
                        recipients.push(sender);
                    }
                }
            }
        }

        let mut sent_count = 0;
        for recipient in recipients {
            if recipient.send(message.clone()).is_ok() {
                sent_count += 1;
            }
        }

        sent_count
    }

    async fn active_connections(&self, session_id: Uuid) -> Option<usize> {
        self.sessions.read().await.get(&session_id).map(|session| session.active_connections)
    }

    async fn workspace_for_session(&self, session_id: Uuid) -> Option<Uuid> {
        self.sessions.read().await.get(&session_id).map(|session| session.workspace_id)
    }

    async fn actor_user_for_session(&self, session_id: Uuid) -> Option<Uuid> {
        self.sessions.read().await.get(&session_id).and_then(|session| session.actor_user_id)
    }

    async fn token_for_session(&self, session_id: Uuid) -> Option<String> {
        self.sessions.read().await.get(&session_id).map(|session| session.session_token.clone())
    }

    async fn subscriptions_for_session(&self, session_id: Uuid) -> Option<Vec<Uuid>> {
        self.sessions.read().await.get(&session_id).map(|session| {
            let mut subscriptions = session.subscriptions.iter().copied().collect::<Vec<_>>();
            subscriptions.sort();
            subscriptions
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        handle_hello_message, handle_subscribe_message, handle_yjs_update_message, router,
        CreateSyncSessionResponse, DocSyncStore, SessionTokenValidation, SyncSessionStore,
        WorkspaceMembershipStore, HEARTBEAT_INTERVAL_MS, MAX_FRAME_BYTES,
    };
    use crate::auth::{jwt::JwtAccessTokenService, middleware::WorkspaceRole};
    use axum::{
        body::{to_bytes, Body},
        http::{header::AUTHORIZATION, Method, Request, StatusCode},
    };
    use chrono::{Duration, Utc};
    use scriptum_common::protocol::ws::WsMessage;
    use std::sync::Arc;
    use tower::ServiceExt;
    use uuid::Uuid;

    const TEST_SECRET: &str = "scriptum_test_secret_that_is_definitely_long_enough";

    async fn response_body(response: axum::response::Response) -> String {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should be readable");
        String::from_utf8(bytes.to_vec()).expect("response body should be valid utf8")
    }

    #[tokio::test]
    async fn create_sync_session_requires_matching_workspace() {
        let jwt_service = Arc::new(
            JwtAccessTokenService::new(TEST_SECRET).expect("jwt service should initialize"),
        );
        let session_store = Arc::new(SyncSessionStore::default());
        let membership_store = WorkspaceMembershipStore::for_tests();
        let app = router(
            jwt_service.clone(),
            session_store,
            Arc::new(DocSyncStore::default()),
            membership_store,
            "ws://localhost:8080".to_string(),
        );

        let token = jwt_service
            .issue_workspace_token(Uuid::new_v4(), Uuid::new_v4())
            .expect("access token should be created");
        let payload = r#"{"protocol":"scriptum-sync.v1","client_id":"11111111-1111-1111-1111-111111111111","device_id":"22222222-2222-2222-2222-222222222222"}"#;

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/workspaces/33333333-3333-3333-3333-333333333333/sync-sessions")
                    .header("content-type", "application/json")
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::from(payload))
                    .expect("request should build"),
            )
            .await
            .expect("request should return a response");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn create_sync_session_requires_workspace_membership() {
        let jwt_service = Arc::new(
            JwtAccessTokenService::new(TEST_SECRET).expect("jwt service should initialize"),
        );
        let workspace_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let app = router(
            jwt_service.clone(),
            Arc::new(SyncSessionStore::default()),
            Arc::new(DocSyncStore::default()),
            WorkspaceMembershipStore::for_tests(),
            "ws://localhost:8080".to_string(),
        );
        let token = jwt_service
            .issue_workspace_token(user_id, workspace_id)
            .expect("access token should be created");
        let payload = r#"{"protocol":"scriptum-sync.v1","client_id":"11111111-1111-1111-1111-111111111111","device_id":"22222222-2222-2222-2222-222222222222"}"#;

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/v1/workspaces/{workspace_id}/sync-sessions"))
                    .header("content-type", "application/json")
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::from(payload))
                    .expect("request should build"),
            )
            .await
            .expect("request should return a response");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn create_sync_session_returns_expected_contract() {
        let jwt_service = Arc::new(
            JwtAccessTokenService::new(TEST_SECRET).expect("jwt service should initialize"),
        );
        let session_store = Arc::new(SyncSessionStore::default());
        let membership_store = WorkspaceMembershipStore::for_tests();
        let user_id = Uuid::new_v4();
        let app = router(
            jwt_service.clone(),
            session_store.clone(),
            Arc::new(DocSyncStore::default()),
            membership_store.clone(),
            "ws://localhost:8080".to_string(),
        );
        let workspace_id = Uuid::new_v4();
        membership_store.grant_for_tests(workspace_id, user_id, WorkspaceRole::Viewer).await;
        let token = jwt_service
            .issue_workspace_token(user_id, workspace_id)
            .expect("access token should be created");
        let payload = format!(
            "{{\"protocol\":\"scriptum-sync.v1\",\"client_id\":\"{}\",\"device_id\":\"{}\"}}",
            Uuid::new_v4(),
            Uuid::new_v4()
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/v1/workspaces/{workspace_id}/sync-sessions"))
                    .header("content-type", "application/json")
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::from(payload))
                    .expect("request should build"),
            )
            .await
            .expect("request should return a response");

        assert_eq!(response.status(), StatusCode::CREATED);

        let body = response_body(response).await;
        let parsed: CreateSyncSessionResponse =
            serde_json::from_str(&body).expect("response should deserialize");
        assert_eq!(parsed.heartbeat_interval_ms, HEARTBEAT_INTERVAL_MS);
        assert_eq!(parsed.max_frame_bytes, MAX_FRAME_BYTES);
        assert!(parsed.ws_url.ends_with(&parsed.session_id.to_string()));
        assert_eq!(
            session_store.workspace_for_session(parsed.session_id).await,
            Some(workspace_id)
        );
        assert_eq!(
            session_store.token_for_session(parsed.session_id).await,
            Some(parsed.session_token)
        );
    }

    #[tokio::test]
    async fn sync_session_store_tracks_active_connections() {
        let store = SyncSessionStore::default();
        let session_id = Uuid::new_v4();
        store
            .create_session(
                session_id,
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4().to_string(),
                Uuid::new_v4().to_string(),
                Utc::now() + Duration::minutes(15),
                Utc::now() + Duration::minutes(10),
            )
            .await;

        assert!(store.mark_connected(session_id).await);
        assert_eq!(store.active_connections(session_id).await, Some(1));

        store.mark_disconnected(session_id).await;
        assert_eq!(store.active_connections(session_id).await, Some(0));
    }

    #[tokio::test]
    async fn hello_ack_is_returned_for_valid_session_token() {
        let store = SyncSessionStore::default();
        let session_id = Uuid::new_v4();
        let session_token = Uuid::new_v4().to_string();
        let resume_token = Uuid::new_v4().to_string();
        store
            .create_session(
                session_id,
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4(),
                session_token.clone(),
                resume_token.clone(),
                Utc::now() + Duration::minutes(15),
                Utc::now() + Duration::minutes(10),
            )
            .await;

        let result =
            handle_hello_message(&store, session_id, session_token, Some(resume_token)).await;

        match result {
            Ok(WsMessage::HelloAck { resume_accepted, .. }) => assert!(resume_accepted),
            other => panic!("expected hello ack, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn hello_ack_sets_resume_accepted_false_for_invalid_resume_token() {
        let store = SyncSessionStore::default();
        let session_id = Uuid::new_v4();
        let session_token = Uuid::new_v4().to_string();
        store
            .create_session(
                session_id,
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4(),
                session_token.clone(),
                Uuid::new_v4().to_string(),
                Utc::now() + Duration::minutes(15),
                Utc::now() + Duration::minutes(10),
            )
            .await;

        let result = handle_hello_message(
            &store,
            session_id,
            session_token,
            Some("different-resume-token".to_string()),
        )
        .await;

        match result {
            Ok(WsMessage::HelloAck { resume_accepted, .. }) => assert!(!resume_accepted),
            other => panic!("expected hello ack, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn hello_rejects_invalid_session_token() {
        let store = SyncSessionStore::default();
        let session_id = Uuid::new_v4();
        store
            .create_session(
                session_id,
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4().to_string(),
                Uuid::new_v4().to_string(),
                Utc::now() + Duration::minutes(15),
                Utc::now() + Duration::minutes(10),
            )
            .await;

        let result =
            handle_hello_message(&store, session_id, "wrong-token".to_string(), None).await;

        match result {
            Err(WsMessage::Error { code, .. }) => assert_eq!(code, "SYNC_TOKEN_INVALID"),
            other => panic!("expected token invalid error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn hello_rejects_expired_session_token() {
        let store = SyncSessionStore::default();
        let session_id = Uuid::new_v4();
        let session_token = Uuid::new_v4().to_string();
        store
            .create_session(
                session_id,
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4(),
                session_token.clone(),
                Uuid::new_v4().to_string(),
                Utc::now() - Duration::seconds(1),
                Utc::now() + Duration::minutes(10),
            )
            .await;

        let validation = store.validate_session_token(session_id, &session_token, None).await;
        assert_eq!(validation, SessionTokenValidation::Expired);

        let result = handle_hello_message(&store, session_id, session_token, None).await;
        match result {
            Err(WsMessage::Error { code, .. }) => assert_eq!(code, "SYNC_TOKEN_EXPIRED"),
            other => panic!("expected token expired error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn subscribe_tracks_subscription_and_sends_snapshot_and_updates() {
        let session_store = SyncSessionStore::default();
        let doc_store = DocSyncStore::default();
        let membership_store = WorkspaceMembershipStore::for_tests();
        let session_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();

        session_store
            .create_session(
                session_id,
                workspace_id,
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4().to_string(),
                Uuid::new_v4().to_string(),
                Utc::now() + Duration::minutes(15),
                Utc::now() + Duration::minutes(10),
            )
            .await;

        doc_store.set_snapshot(workspace_id, doc_id, 10, "snapshot-b64".to_string()).await;
        doc_store
            .append_update(
                workspace_id,
                doc_id,
                11,
                Uuid::new_v4(),
                Uuid::new_v4(),
                "update-11-b64".to_string(),
            )
            .await;
        doc_store
            .append_update(
                workspace_id,
                doc_id,
                12,
                Uuid::new_v4(),
                Uuid::new_v4(),
                "update-12-b64".to_string(),
            )
            .await;

        let messages = handle_subscribe_message(
            &session_store,
            &doc_store,
            &membership_store,
            session_id,
            doc_id,
            Some(5),
        )
        .await
        .expect("subscribe should succeed");

        assert_eq!(messages.len(), 3);
        match &messages[0] {
            WsMessage::Snapshot { doc_id: snapshot_doc_id, snapshot_seq, payload_b64 } => {
                assert_eq!(*snapshot_doc_id, doc_id);
                assert_eq!(*snapshot_seq, 10);
                assert_eq!(payload_b64, "snapshot-b64");
            }
            other => panic!("expected snapshot, got {other:?}"),
        }
        match &messages[1] {
            WsMessage::YjsUpdate {
                doc_id: update_doc_id, base_server_seq, payload_b64, ..
            } => {
                assert_eq!(*update_doc_id, doc_id);
                assert_eq!(*base_server_seq, 10);
                assert_eq!(payload_b64, "update-11-b64");
            }
            other => panic!("expected yjs update, got {other:?}"),
        }
        match &messages[2] {
            WsMessage::YjsUpdate {
                doc_id: update_doc_id, base_server_seq, payload_b64, ..
            } => {
                assert_eq!(*update_doc_id, doc_id);
                assert_eq!(*base_server_seq, 11);
                assert_eq!(payload_b64, "update-12-b64");
            }
            other => panic!("expected yjs update, got {other:?}"),
        }

        assert_eq!(session_store.subscriptions_for_session(session_id).await, Some(vec![doc_id]));
    }

    #[tokio::test]
    async fn subscribe_rejects_negative_last_server_seq() {
        let session_store = SyncSessionStore::default();
        let doc_store = DocSyncStore::default();
        let membership_store = WorkspaceMembershipStore::for_tests();
        let session_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();

        session_store
            .create_session(
                session_id,
                workspace_id,
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4().to_string(),
                Uuid::new_v4().to_string(),
                Utc::now() + Duration::minutes(15),
                Utc::now() + Duration::minutes(10),
            )
            .await;

        let error = handle_subscribe_message(
            &session_store,
            &doc_store,
            &membership_store,
            session_id,
            doc_id,
            Some(-1),
        )
        .await
        .expect_err("subscribe should reject negative cursor");

        match error {
            WsMessage::Error { code, doc_id: message_doc_id, .. } => {
                assert_eq!(code, "SYNC_INVALID_LAST_SERVER_SEQ");
                assert_eq!(message_doc_id, Some(doc_id));
            }
            other => panic!("expected protocol error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn subscribe_requires_workspace_membership_for_authenticated_actor() {
        let session_store = SyncSessionStore::default();
        let doc_store = DocSyncStore::default();
        let membership_store = WorkspaceMembershipStore::for_tests();
        let session_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        let actor_user_id = Uuid::new_v4();

        session_store
            .create_session_with_actor(
                session_id,
                workspace_id,
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4().to_string(),
                Uuid::new_v4().to_string(),
                Utc::now() + Duration::minutes(15),
                Utc::now() + Duration::minutes(10),
                Some(actor_user_id),
                None,
            )
            .await;

        let error = handle_subscribe_message(
            &session_store,
            &doc_store,
            &membership_store,
            session_id,
            doc_id,
            None,
        )
        .await
        .expect_err("subscribe should fail without membership");

        match error {
            WsMessage::Error { code, doc_id: message_doc_id, .. } => {
                assert_eq!(code, "AUTH_FORBIDDEN");
                assert_eq!(message_doc_id, Some(doc_id));
            }
            other => panic!("expected forbidden error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn yjs_update_applies_and_returns_ack() {
        let session_store = SyncSessionStore::default();
        let doc_store = DocSyncStore::default();
        let membership_store = WorkspaceMembershipStore::for_tests();
        let session_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        let client_id = Uuid::new_v4();
        let client_update_id = Uuid::new_v4();

        session_store
            .create_session(
                session_id,
                workspace_id,
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4().to_string(),
                Uuid::new_v4().to_string(),
                Utc::now() + Duration::minutes(15),
                Utc::now() + Duration::minutes(10),
            )
            .await;
        handle_subscribe_message(
            &session_store,
            &doc_store,
            &membership_store,
            session_id,
            doc_id,
            None,
        )
        .await
        .expect("subscribe should succeed");

        let result = handle_yjs_update_message(
            &session_store,
            &doc_store,
            session_id,
            doc_id,
            client_id,
            client_update_id,
            0,
            "update-payload-b64".to_string(),
        )
        .await
        .expect("yjs_update should apply");

        match result.ack {
            WsMessage::Ack {
                doc_id: ack_doc_id,
                client_update_id: ack_update_id,
                server_seq,
                applied,
            } => {
                assert_eq!(ack_doc_id, doc_id);
                assert_eq!(ack_update_id, client_update_id);
                assert_eq!(server_seq, 1);
                assert!(applied);
            }
            other => panic!("expected ack, got {other:?}"),
        }

        let replay = doc_store.build_state_sync_messages(workspace_id, doc_id, Some(0)).await;
        assert_eq!(replay.len(), 1);
        match &replay[0] {
            WsMessage::YjsUpdate {
                client_id: replay_client_id,
                client_update_id: replay_update_id,
                base_server_seq,
                payload_b64,
                ..
            } => {
                assert_eq!(*replay_client_id, client_id);
                assert_eq!(*replay_update_id, client_update_id);
                assert_eq!(*base_server_seq, 0);
                assert_eq!(payload_b64, "update-payload-b64");
            }
            other => panic!("expected yjs update, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn yjs_update_deduplicates_client_update_id() {
        let session_store = SyncSessionStore::default();
        let doc_store = DocSyncStore::default();
        let membership_store = WorkspaceMembershipStore::for_tests();
        let session_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        let client_id = Uuid::new_v4();
        let client_update_id = Uuid::new_v4();

        session_store
            .create_session(
                session_id,
                workspace_id,
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4().to_string(),
                Uuid::new_v4().to_string(),
                Utc::now() + Duration::minutes(15),
                Utc::now() + Duration::minutes(10),
            )
            .await;
        handle_subscribe_message(
            &session_store,
            &doc_store,
            &membership_store,
            session_id,
            doc_id,
            None,
        )
        .await
        .expect("subscribe should succeed");

        handle_yjs_update_message(
            &session_store,
            &doc_store,
            session_id,
            doc_id,
            client_id,
            client_update_id,
            0,
            "payload-1".to_string(),
        )
        .await
        .expect("first yjs_update should apply");

        let duplicate = handle_yjs_update_message(
            &session_store,
            &doc_store,
            session_id,
            doc_id,
            client_id,
            client_update_id,
            1,
            "payload-1".to_string(),
        )
        .await
        .expect("duplicate yjs_update should return ack");

        match duplicate.ack {
            WsMessage::Ack { applied, server_seq, .. } => {
                assert!(!applied);
                assert_eq!(server_seq, 1);
            }
            other => panic!("expected ack, got {other:?}"),
        }

        let replay = doc_store.build_state_sync_messages(workspace_id, doc_id, Some(0)).await;
        assert_eq!(replay.len(), 1);
    }

    #[tokio::test]
    async fn yjs_update_rejects_future_base_server_seq() {
        let session_store = SyncSessionStore::default();
        let doc_store = DocSyncStore::default();
        let membership_store = WorkspaceMembershipStore::for_tests();
        let session_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();

        session_store
            .create_session(
                session_id,
                workspace_id,
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4().to_string(),
                Uuid::new_v4().to_string(),
                Utc::now() + Duration::minutes(15),
                Utc::now() + Duration::minutes(10),
            )
            .await;
        handle_subscribe_message(
            &session_store,
            &doc_store,
            &membership_store,
            session_id,
            doc_id,
            None,
        )
        .await
        .expect("subscribe should succeed");

        let error = handle_yjs_update_message(
            &session_store,
            &doc_store,
            session_id,
            doc_id,
            Uuid::new_v4(),
            Uuid::new_v4(),
            5,
            "payload".to_string(),
        )
        .await
        .expect_err("future base_server_seq should be rejected");

        match error {
            WsMessage::Error { code, doc_id: message_doc_id, .. } => {
                assert_eq!(code, "SYNC_BASE_SERVER_SEQ_MISMATCH");
                assert_eq!(message_doc_id, Some(doc_id));
            }
            other => panic!("expected protocol error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn yjs_update_captures_session_attribution_in_update_log() {
        let session_store = SyncSessionStore::default();
        let doc_store = DocSyncStore::default();
        let membership_store = WorkspaceMembershipStore::for_tests();
        let session_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        let client_id = Uuid::new_v4();
        let client_update_id = Uuid::new_v4();
        let actor_user_id = Uuid::new_v4();
        let actor_agent_id = "claude-reviewer".to_string();
        membership_store.grant_for_tests(workspace_id, actor_user_id, WorkspaceRole::Editor).await;

        session_store
            .create_session_with_actor(
                session_id,
                workspace_id,
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4().to_string(),
                Uuid::new_v4().to_string(),
                Utc::now() + Duration::minutes(15),
                Utc::now() + Duration::minutes(10),
                Some(actor_user_id),
                Some(actor_agent_id.clone()),
            )
            .await;
        handle_subscribe_message(
            &session_store,
            &doc_store,
            &membership_store,
            session_id,
            doc_id,
            None,
        )
        .await
        .expect("subscribe should succeed");

        handle_yjs_update_message(
            &session_store,
            &doc_store,
            session_id,
            doc_id,
            client_id,
            client_update_id,
            0,
            "payload".to_string(),
        )
        .await
        .expect("yjs_update should apply");

        let attribution = doc_store
            .update_attribution(workspace_id, doc_id, client_id, client_update_id)
            .await
            .expect("attribution should be stored");
        assert_eq!(attribution.user_id, Some(actor_user_id));
        assert_eq!(attribution.agent_id, Some(actor_agent_id));
    }

    #[tokio::test]
    async fn create_sync_session_rejects_unsupported_protocol_with_upgrade_required() {
        let jwt_service = Arc::new(
            JwtAccessTokenService::new(TEST_SECRET).expect("jwt service should initialize"),
        );
        let session_store = Arc::new(SyncSessionStore::default());
        let membership_store = WorkspaceMembershipStore::for_tests();
        let user_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        membership_store.grant_for_tests(workspace_id, user_id, WorkspaceRole::Editor).await;

        let app = router(
            jwt_service.clone(),
            session_store,
            Arc::new(DocSyncStore::default()),
            membership_store,
            "ws://localhost:8080".to_string(),
        );

        let token = jwt_service
            .issue_workspace_token(user_id, workspace_id)
            .expect("access token should be created");

        let payload = format!(
            "{{\"protocol\":\"scriptum-sync.v99\",\"client_id\":\"{}\",\"device_id\":\"{}\"}}",
            Uuid::new_v4(),
            Uuid::new_v4()
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/v1/workspaces/{workspace_id}/sync-sessions"))
                    .header("content-type", "application/json")
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::from(payload))
                    .expect("request should build"),
            )
            .await
            .expect("request should return a response");

        assert_eq!(response.status(), StatusCode::UPGRADE_REQUIRED);

        let body = response_body(response).await;
        let parsed: serde_json::Value =
            serde_json::from_str(&body).expect("response should be valid json");
        assert_eq!(parsed["error"]["code"], "UPGRADE_REQUIRED");
        assert_eq!(parsed["error"]["retryable"], false);
        assert_eq!(parsed["error"]["details"]["requested_version"], "scriptum-sync.v99");
        assert!(parsed["error"]["details"]["supported_versions"]
            .as_array()
            .expect("supported_versions should be an array")
            .iter()
            .any(|v| v == "scriptum-sync.v1"));
    }
}
