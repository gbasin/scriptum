use crate::auth::{
    jwt::JwtAccessTokenService,
    middleware::{require_bearer_auth, AuthenticatedUser},
};
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
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use uuid::Uuid;

const SUPPORTED_PROTOCOL: &str = "scriptum-sync.v1";
const HEARTBEAT_INTERVAL_MS: u32 = 15_000;
const MAX_FRAME_BYTES: u32 = 262_144;
const SESSION_TOKEN_TTL_MINUTES: i64 = 15;
const RESUME_TOKEN_TTL_MINUTES: i64 = 10;

#[derive(Clone)]
pub struct SyncSessionRouterState {
    session_store: Arc<SyncSessionStore>,
    ws_base_url: Arc<str>,
}

#[derive(Debug, Clone, Default)]
pub struct SyncSessionStore {
    sessions: Arc<RwLock<HashMap<Uuid, SyncSessionRecord>>>,
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
    ws_base_url: String,
) -> Router {
    let state =
        SyncSessionRouterState { session_store, ws_base_url: Arc::<str>::from(ws_base_url) };
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
    if payload.protocol != SUPPORTED_PROTOCOL {
        return (StatusCode::BAD_REQUEST, "unsupported sync protocol").into_response();
    }

    if workspace_id != user.workspace_id {
        return (StatusCode::FORBIDDEN, "workspace mismatch").into_response();
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
        .create_session(
            session_id,
            workspace_id,
            payload.client_id,
            payload.device_id,
            session_token.clone(),
            resume_token.clone(),
            session_expires_at,
            resume_expires_at,
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
        return StatusCode::NOT_FOUND.into_response();
    }

    let session_store = state.session_store.clone();
    ws.max_frame_size(MAX_FRAME_BYTES as usize).on_upgrade(move |socket| async move {
        handle_socket(session_store, session_id, socket).await;
    })
}

async fn handle_socket(
    session_store: Arc<SyncSessionStore>,
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

    while let Some(message) = socket.recv().await {
        match message {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionTokenValidation {
    Valid { resume_accepted: bool },
    Invalid,
    Expired,
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
        }
    }

    async fn active_connections(&self, session_id: Uuid) -> Option<usize> {
        self.sessions.read().await.get(&session_id).map(|session| session.active_connections)
    }

    async fn workspace_for_session(&self, session_id: Uuid) -> Option<Uuid> {
        self.sessions.read().await.get(&session_id).map(|session| session.workspace_id)
    }

    async fn token_for_session(&self, session_id: Uuid) -> Option<String> {
        self.sessions.read().await.get(&session_id).map(|session| session.session_token.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        handle_hello_message, router, CreateSyncSessionResponse, SessionTokenValidation,
        SyncSessionStore, HEARTBEAT_INTERVAL_MS, MAX_FRAME_BYTES,
    };
    use crate::auth::jwt::JwtAccessTokenService;
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
        let app = router(jwt_service.clone(), session_store, "ws://localhost:8080".to_string());

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
    async fn create_sync_session_returns_expected_contract() {
        let jwt_service = Arc::new(
            JwtAccessTokenService::new(TEST_SECRET).expect("jwt service should initialize"),
        );
        let session_store = Arc::new(SyncSessionStore::default());
        let app =
            router(jwt_service.clone(), session_store.clone(), "ws://localhost:8080".to_string());
        let workspace_id = Uuid::new_v4();
        let token = jwt_service
            .issue_workspace_token(Uuid::new_v4(), workspace_id)
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

        let result = handle_hello_message(&store, session_id, session_token, Some(resume_token)).await;

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
}
