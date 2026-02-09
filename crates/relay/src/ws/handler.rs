use super::protocol as ws_protocol;
use super::session::{
    ApplyClientUpdateResult, CreateSyncSessionRequest, CreateSyncSessionResponse, DocSyncStore,
    SessionTokenValidation, SyncSessionRouterState, SyncSessionStore, WorkspaceMembershipStore,
    HEARTBEAT_INTERVAL_MS, HEARTBEAT_TIMEOUT_MS, MAX_FRAME_BYTES, RESUME_TOKEN_TTL_MINUTES,
    SESSION_TOKEN_TTL_MINUTES,
};
use crate::auth::{
    jwt::JwtAccessTokenService,
    middleware::{require_bearer_auth, AuthenticatedUser, WorkspaceRole},
};
use crate::awareness::AwarenessStore;
use crate::error::{
    current_trace_id, trace_id_from_headers_or_generate, with_trace_id_scope, ErrorCode, RelayError,
};
use crate::metrics;
use crate::protocol;
use axum::{
    extract::{
        ws::{close_code, CloseFrame, Message, WebSocket, WebSocketUpgrade},
        Extension, Path, State,
    },
    http::{HeaderMap, StatusCode},
    middleware,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{Duration, Utc};
use scriptum_common::protocol::ws::WsMessage;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::{error, warn};
use uuid::Uuid;

pub fn router(
    jwt_service: Arc<JwtAccessTokenService>,
    session_store: Arc<SyncSessionStore>,
    doc_store: Arc<DocSyncStore>,
    awareness_store: Arc<AwarenessStore>,
    membership_store: WorkspaceMembershipStore,
    ws_base_url: String,
) -> Router {
    let state = SyncSessionRouterState {
        session_store,
        doc_store,
        awareness_store,
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
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if !state.session_store.session_exists(session_id).await {
        return RelayError::from_code(ErrorCode::NotFound).into_response();
    }

    let session_store = state.session_store.clone();
    let doc_store = state.doc_store.clone();
    let awareness_store = state.awareness_store.clone();
    let membership_store = state.membership_store.clone();
    let trace_id = trace_id_from_headers_or_generate(&headers);
    ws.max_frame_size(MAX_FRAME_BYTES as usize).on_upgrade(move |socket| async move {
        with_trace_id_scope(
            trace_id,
            handle_socket(
                session_store,
                doc_store,
                awareness_store,
                membership_store,
                session_id,
                socket,
            ),
        )
        .await;
    })
}

fn frame_size_exceeded_reason() -> String {
    format!("websocket frame exceeds maximum size of {MAX_FRAME_BYTES} bytes")
}

fn is_frame_size_violation(error: &axum::Error) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("message too long")
        || message.contains("frame too long")
        || message.contains("too large")
        || message.contains("too big")
        || message.contains("size limit")
}

async fn close_frame_too_large(socket: &mut WebSocket) {
    let _ = socket
        .send(Message::Close(Some(CloseFrame {
            code: close_code::SIZE,
            reason: frame_size_exceeded_reason().into(),
        })))
        .await;
}

async fn handle_socket(
    session_store: Arc<SyncSessionStore>,
    doc_store: Arc<DocSyncStore>,
    awareness_store: Arc<AwarenessStore>,
    membership_store: WorkspaceMembershipStore,
    session_id: Uuid,
    mut socket: WebSocket,
) {
    let trace_id = current_trace_id().unwrap_or_else(|| "unknown".to_string());

    if !session_store.mark_connected(session_id).await {
        return;
    }

    let hello_started_at = Instant::now();
    let hello = match socket.recv().await {
        Some(Ok(Message::Text(raw_message))) => {
            if raw_message.len() > MAX_FRAME_BYTES as usize {
                metrics::record_ws_request(
                    "hello",
                    true,
                    hello_started_at.elapsed().as_millis() as u64,
                );
                close_frame_too_large(&mut socket).await;
                session_store.mark_disconnected(session_id).await;
                return;
            }

            match ws_protocol::decode_message(&raw_message) {
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
                        metrics::record_ws_request(
                            "hello",
                            true,
                            hello_started_at.elapsed().as_millis() as u64,
                        );
                        let _ = ws_protocol::send_ws_message(&mut socket, &error_message).await;
                        let _ = socket.send(Message::Close(None)).await;
                        session_store.mark_disconnected(session_id).await;
                        return;
                    }
                },
                _ => {
                    metrics::record_ws_request(
                        "hello",
                        true,
                        hello_started_at.elapsed().as_millis() as u64,
                    );
                    let _ = ws_protocol::send_ws_message(
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
        Some(Err(error)) if is_frame_size_violation(&error) => {
            metrics::record_ws_request(
                "hello",
                true,
                hello_started_at.elapsed().as_millis() as u64,
            );
            close_frame_too_large(&mut socket).await;
            session_store.mark_disconnected(session_id).await;
            return;
        }
        _ => {
            metrics::record_ws_request(
                "hello",
                true,
                hello_started_at.elapsed().as_millis() as u64,
            );
            session_store.mark_disconnected(session_id).await;
            return;
        }
    };

    if ws_protocol::send_ws_message(&mut socket, &hello).await.is_err() {
        metrics::record_ws_request("hello", true, hello_started_at.elapsed().as_millis() as u64);
        session_store.mark_disconnected(session_id).await;
        return;
    }
    metrics::record_ws_request("hello", false, hello_started_at.elapsed().as_millis() as u64);

    let (outbound_sender, mut outbound_receiver) = mpsc::unbounded_channel::<WsMessage>();
    if !session_store.register_outbound(session_id, outbound_sender).await {
        session_store.mark_disconnected(session_id).await;
        return;
    }

    // Heartbeat: server pings every HEARTBEAT_INTERVAL_MS, disconnects if no
    // pong arrives within HEARTBEAT_TIMEOUT_MS.
    let mut heartbeat_interval =
        tokio::time::interval(std::time::Duration::from_millis(HEARTBEAT_INTERVAL_MS as u64));
    heartbeat_interval.reset(); // skip immediate first tick
    let mut last_pong = Instant::now();
    let heartbeat_timeout = std::time::Duration::from_millis(HEARTBEAT_TIMEOUT_MS);

    loop {
        tokio::select! {
            _ = heartbeat_interval.tick() => {
                if last_pong.elapsed() > heartbeat_timeout {
                    warn!(
                        session_id = %session_id,
                        trace_id = %trace_id,
                        "heartbeat timeout, disconnecting"
                    );
                    break;
                }
                if socket.send(Message::Ping(vec![].into())).await.is_err() {
                    break;
                }
            }
            maybe_outbound = outbound_receiver.recv() => {
                match maybe_outbound {
                    Some(outbound_message) => {
                        if ws_protocol::send_ws_message(&mut socket, &outbound_message).await.is_err() {
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
                        if raw_message.len() > MAX_FRAME_BYTES as usize {
                            close_frame_too_large(&mut socket).await;
                            break;
                        }

                        let inbound = match ws_protocol::decode_message(&raw_message) {
                            Ok(message) => message,
                            Err(_) => {
                                if ws_protocol::send_ws_message(
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
                                let started_at = Instant::now();
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
                                        metrics::record_ws_request(
                                            "subscribe",
                                            false,
                                            started_at.elapsed().as_millis() as u64,
                                        );
                                        let mut send_failed = false;
                                        for outbound in outbound_messages {
                                            if ws_protocol::send_ws_message(&mut socket, &outbound).await.is_err() {
                                                send_failed = true;
                                                break;
                                            }
                                        }

                                        if send_failed {
                                            break;
                                        }
                                    }
                                    Err(error_message) => {
                                        metrics::record_ws_request(
                                            "subscribe",
                                            true,
                                            started_at.elapsed().as_millis() as u64,
                                        );
                                        if ws_protocol::send_ws_message(&mut socket, &error_message).await.is_err() {
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
                                let started_at = Instant::now();
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
                                        let elapsed_ms = started_at.elapsed().as_millis() as u64;
                                        metrics::observe_sync_ack_latency_ms(elapsed_ms);
                                        metrics::record_ws_request("yjs_update", false, elapsed_ms);
                                        if ws_protocol::send_ws_message(&mut socket, &result.ack).await.is_err() {
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
                                        let elapsed_ms = started_at.elapsed().as_millis() as u64;
                                        metrics::record_ws_request("yjs_update", true, elapsed_ms);
                                        if matches!(
                                            &error_message,
                                            WsMessage::Error { code, .. } if code == "SYNC_BASE_SERVER_SEQ_MISMATCH"
                                        ) {
                                            metrics::increment_sequence_gap_count();
                                        }
                                        if ws_protocol::send_ws_message(&mut socket, &error_message).await.is_err() {
                                            break;
                                        }
                                    }
                                }
                            }
                            WsMessage::AwarenessUpdate { doc_id, peers } => {
                                let started_at = Instant::now();
                                match handle_awareness_update(
                                    &session_store,
                                    &awareness_store,
                                    session_id,
                                    doc_id,
                                    peers,
                                )
                                .await
                                {
                                    Ok(broadcast) => {
                                        metrics::record_ws_request(
                                            "awareness_update",
                                            false,
                                            started_at.elapsed().as_millis() as u64,
                                        );
                                        let _ = session_store
                                            .broadcast_to_subscribers_excluding(
                                                broadcast.workspace_id,
                                                doc_id,
                                                broadcast.message,
                                                session_id,
                                            )
                                            .await;
                                    }
                                    Err(error_message) => {
                                        metrics::record_ws_request(
                                            "awareness_update",
                                            true,
                                            started_at.elapsed().as_millis() as u64,
                                        );
                                        if ws_protocol::send_ws_message(&mut socket, &error_message).await.is_err() {
                                            break;
                                        }
                                    }
                                }
                            }
                            _ => {
                                if ws_protocol::send_ws_message(
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
                    Ok(Message::Pong(_)) => {
                        last_pong = Instant::now();
                    }
                    Ok(Message::Close(_)) => break,
                    Ok(_) => {}
                    Err(error) => {
                        if is_frame_size_violation(&error) {
                            close_frame_too_large(&mut socket).await;
                        }
                        break;
                    }
                }
            }
        }
    }

    // Clean up awareness state for this session.
    if let Some(workspace_id) = session_store.workspace_for_session(session_id).await {
        if let Some(subscriptions) = session_store.subscriptions_for_session(session_id).await {
            awareness_store.remove_session(workspace_id, &subscriptions, session_id).await;
            // Broadcast updated awareness to remaining subscribers.
            for doc_id in &subscriptions {
                let aggregated = awareness_store.aggregate(workspace_id, *doc_id).await;
                let _ = session_store
                    .broadcast_to_subscribers(
                        workspace_id,
                        *doc_id,
                        WsMessage::AwarenessUpdate { doc_id: *doc_id, peers: aggregated },
                    )
                    .await;
            }
        }
    }

    session_store.mark_disconnected(session_id).await;
}

pub(crate) async fn handle_hello_message(
    session_store: &SyncSessionStore,
    session_id: Uuid,
    session_token: String,
    resume_token: Option<String>,
) -> Result<WsMessage, WsMessage> {
    match session_store
        .validate_session_token(session_id, &session_token, resume_token.as_deref())
        .await
    {
        SessionTokenValidation::Valid { resume_accepted, resume_token, resume_expires_at } => {
            Ok(WsMessage::HelloAck {
                server_time: Utc::now().to_rfc3339(),
                resume_accepted,
                resume_token,
                resume_expires_at: resume_expires_at.to_rfc3339(),
            })
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

pub(crate) async fn handle_subscribe_message(
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
                    trace_id = current_trace_id().as_deref().unwrap_or(""),
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
pub(crate) struct YjsUpdateHandlingResult {
    pub(crate) workspace_id: Uuid,
    pub(crate) ack: WsMessage,
    pub(crate) broadcast: Option<WsMessage>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_yjs_update_message(
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

// ── Awareness handling ──────────────────────────────────────────────

#[derive(Debug)]
pub(crate) struct AwarenessBroadcast {
    pub(crate) workspace_id: Uuid,
    pub(crate) message: WsMessage,
}

pub(crate) async fn handle_awareness_update(
    session_store: &SyncSessionStore,
    awareness_store: &AwarenessStore,
    session_id: Uuid,
    doc_id: Uuid,
    peers: Vec<serde_json::Value>,
) -> Result<AwarenessBroadcast, WsMessage> {
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
            message: "subscribe before sending awareness_update".to_string(),
            retryable: false,
            doc_id: Some(doc_id),
        });
    }

    // Store this session's awareness data.
    awareness_store.update(workspace_id, doc_id, session_id, peers).await;

    // Build aggregate for broadcast (all peers from all sessions).
    let aggregated = awareness_store.aggregate(workspace_id, doc_id).await;

    Ok(AwarenessBroadcast {
        workspace_id,
        message: WsMessage::AwarenessUpdate { doc_id, peers: aggregated },
    })
}
