mod api;
mod audit;
mod auth;
mod awareness;
pub mod config;
mod cors;
pub mod etag;
pub mod idempotency;
mod db;
mod error;
mod leader;
mod protocol;
mod sync;
mod ws;

use anyhow::Context;
use axum::{
    body::Body,
    extract::DefaultBodyLimit,
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use std::{sync::Arc, time::Instant};
use tokio::net::TcpListener;
use tracing::{error, info};
use ws::{DocSyncStore, SyncSessionStore};

use crate::auth::{jwt::JwtAccessTokenService, oauth::OAuthState};
use crate::error::{
    attach_request_id_header, request_id_from_headers_or_generate, with_request_id_scope,
    ErrorCode, RelayError,
};

const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = config::RelayConfig::from_env();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&cfg.log_filter)),
        )
        .init();

    if cfg.is_dev_jwt_secret() {
        tracing::warn!("using development JWT secret — set SCRIPTUM_RELAY_JWT_SECRET in production");
    }

    let jwt_service =
        Arc::new(JwtAccessTokenService::new(&cfg.jwt_secret).context("invalid relay JWT secret")?);
    let session_store = Arc::new(SyncSessionStore::default());
    let doc_store = Arc::new(DocSyncStore::default());
    let membership_store = ws::WorkspaceMembershipStore::from_env()
        .await
        .context("failed to initialize websocket workspace membership store")?;
    let oauth_state = OAuthState::from_env();
    let app = build_router(
        Arc::clone(&jwt_service),
        session_store,
        doc_store,
        membership_store,
        oauth_state,
        cfg.ws_base_url.clone(),
        api::build_router_from_env(jwt_service)
            .await
            .context("failed to build relay workspace API router")?,
    );

    let listener = TcpListener::bind(cfg.listen_addr)
        .await
        .with_context(|| format!("failed to bind relay listener on {}", cfg.listen_addr))?;

    info!(listen_addr = %cfg.listen_addr, "starting relay server");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("relay server exited unexpectedly")
}

fn build_router(
    jwt_service: Arc<JwtAccessTokenService>,
    session_store: Arc<SyncSessionStore>,
    doc_store: Arc<DocSyncStore>,
    membership_store: ws::WorkspaceMembershipStore,
    oauth_state: OAuthState,
    ws_base_url: String,
    api_router: Router,
) -> Router {
    apply_middleware(
        Router::new()
            .route("/healthz", get(healthz))
            .merge(auth::oauth::router(oauth_state))
            .merge(ws::router(jwt_service, session_store, doc_store, Arc::new(awareness::AwarenessStore::default()), membership_store, ws_base_url))
            .merge(api_router),
    )
}

fn apply_middleware(router: Router) -> Router {
    router
        .layer(cors::cors_layer())
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .layer(middleware::from_fn(request_context_middleware))
        .layer(middleware::from_fn(panic_handler))
}

async fn healthz() -> (StatusCode, &'static str) {
    (StatusCode::OK, "ok")
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }

    info!("shutdown signal received");
}

async fn panic_handler(request: Request<Body>, next: Next) -> Response {
    let request_id = request_id_from_headers_or_generate(request.headers());
    match tokio::spawn(async move { next.run(request).await }).await {
        Ok(response) => response,
        Err(join_error) => {
            error!(?join_error, request_id = %request_id, "request handling panicked");
            RelayError::from_code(ErrorCode::InternalError)
                .with_request_id(request_id)
                .into_response()
        }
    }
}

async fn request_context_middleware(request: Request<Body>, next: Next) -> Response {
    let request_id = request_id_from_headers_or_generate(request.headers());

    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let started_at = Instant::now();

    let mut response = with_request_id_scope(request_id.clone(), next.run(request)).await;
    attach_request_id_header(&mut response, &request_id);

    info!(
        request_id = %request_id,
        method = %method,
        path = %path,
        status = response.status().as_u16(),
        latency_ms = started_at.elapsed().as_millis() as u64,
        "request completed"
    );

    response
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        body::{to_bytes, Body},
        http::{Method, Request, StatusCode},
        routing::{get, post},
        Router,
    };
    use serde_json::Value;
    use tower::ServiceExt;
    use uuid::Uuid;

    use super::{apply_middleware, build_router, MAX_REQUEST_BODY_BYTES};
    use crate::{
        auth::{jwt::JwtAccessTokenService, oauth::OAuthState},
        error::REQUEST_ID_HEADER,
        ws::{DocSyncStore, SyncSessionStore, WorkspaceMembershipStore},
    };

    fn test_router() -> Router {
        let jwt_service = Arc::new(
            JwtAccessTokenService::new("scriptum_test_secret_that_is_definitely_long_enough")
                .expect("test jwt service should initialize"),
        );
        let session_store = Arc::new(SyncSessionStore::default());
        let doc_store = Arc::new(DocSyncStore::default());
        build_router(
            jwt_service,
            session_store,
            doc_store,
            WorkspaceMembershipStore::for_tests(),
            OAuthState::from_env(),
            "ws://localhost:8080".to_string(),
            Router::new(),
        )
    }

    #[tokio::test]
    async fn health_check_has_request_id_header() {
        let response = test_router()
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .expect("healthz request should build"),
            )
            .await
            .expect("healthz request should succeed");

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("x-request-id"));
    }

    #[tokio::test]
    async fn panic_handler_returns_internal_server_error() {
        async fn panic_route() -> &'static str {
            panic!("test panic");
        }

        let app = apply_middleware(Router::new().route("/panic", get(panic_route)));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/panic")
                    .header(REQUEST_ID_HEADER, "req-panic-123")
                    .body(Body::empty())
                    .expect("panic request should build"),
            )
            .await
            .expect("panic request should return a response");

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(response.headers().get(REQUEST_ID_HEADER).unwrap(), "req-panic-123");

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("panic response body should read");
        let parsed: Value =
            serde_json::from_slice(&body).expect("panic response body should be valid json");
        assert_eq!(parsed["error"]["code"], "INTERNAL_ERROR");
        assert_eq!(parsed["error"]["retryable"], true);
        assert_eq!(parsed["error"]["request_id"], "req-panic-123");
    }

    #[tokio::test]
    async fn request_body_limit_is_enforced() {
        async fn echo(body: String) -> String {
            body
        }

        let oversized_body = "a".repeat(MAX_REQUEST_BODY_BYTES + 1);
        let app = apply_middleware(Router::new().route("/echo", post(echo)));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/echo")
                    .header("content-type", "text/plain")
                    .body(Body::from(oversized_body))
                    .expect("echo request should build"),
            )
            .await
            .expect("echo request should return a response");

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn missing_bearer_auth_uses_structured_error_envelope() {
        let workspace_id = Uuid::new_v4();
        let payload = format!(
            "{{\"protocol\":\"scriptum-sync.v1\",\"client_id\":\"{}\",\"device_id\":\"{}\"}}",
            Uuid::new_v4(),
            Uuid::new_v4()
        );

        let response = test_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/v1/workspaces/{workspace_id}/sync-sessions"))
                    .header(REQUEST_ID_HEADER, "req-auth-123")
                    .header("content-type", "application/json")
                    .body(Body::from(payload))
                    .expect("sync-session request should build"),
            )
            .await
            .expect("sync-session request should return response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers().get(REQUEST_ID_HEADER).unwrap(), "req-auth-123");

        let body =
            to_bytes(response.into_body(), usize::MAX).await.expect("auth error body should read");
        let parsed: Value =
            serde_json::from_slice(&body).expect("auth error body should be valid json");
        assert_eq!(parsed["error"]["code"], "AUTH_INVALID_TOKEN");
        assert_eq!(parsed["error"]["retryable"], false);
        assert_eq!(parsed["error"]["request_id"], "req-auth-123");
        assert!(parsed["error"]["details"].is_object());
    }
}
