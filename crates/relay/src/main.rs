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
pub mod validation;
mod ws;

use anyhow::Context;
use axum::{
    body::Body,
    extract::DefaultBodyLimit,
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Extension, Json, Router,
};
use serde::Serialize;
use sqlx::PgPool;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{sync::Arc, time::Instant};
use tokio::net::TcpListener;
use tracing::{error, info};
use ws::{DocSyncStore, SyncSessionStore};

use crate::auth::{jwt::JwtAccessTokenService, oauth::OAuthState};
use crate::db::pool::{check_pool_health, create_pg_pool, PoolConfig};
use crate::error::{
    attach_request_id_header, request_id_from_headers_or_generate, with_request_id_scope,
    ErrorCode, RelayError,
};
use crate::sync::sequencer::UpdateSequencer;

const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;
type DbCheckFuture = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>;
type DbCheckFn = dyn Fn() -> DbCheckFuture + Send + Sync;

#[derive(Clone)]
struct ReadinessProbe {
    db_check: Arc<DbCheckFn>,
    sequencer_recovered: Arc<AtomicBool>,
}

impl ReadinessProbe {
    fn from_pool(pool: PgPool) -> Self {
        let pool = Arc::new(pool);
        let db_check = Arc::new(move || {
            let pool = Arc::clone(&pool);
            Box::pin(async move { check_pool_health(&pool).await })
                as Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>
        });

        Self { db_check, sequencer_recovered: Arc::new(AtomicBool::new(false)) }
    }

    #[cfg(test)]
    fn from_db_check<F>(db_check: F) -> Self
    where
        F: Fn() -> DbCheckFuture + Send + Sync + 'static,
    {
        Self { db_check: Arc::new(db_check), sequencer_recovered: Arc::new(AtomicBool::new(false)) }
    }

    fn mark_sequencer_recovered(&self) {
        self.sequencer_recovered.store(true, Ordering::SeqCst);
    }

    async fn evaluate(&self) -> ReadinessResponse {
        let db_connected = (self.db_check)().await.is_ok();
        let sequencer_recovered = self.sequencer_recovered.load(Ordering::SeqCst);
        ReadinessResponse {
            ready: db_connected && sequencer_recovered,
            db_connected,
            sequencer_recovered,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct ReadinessResponse {
    ready: bool,
    db_connected: bool,
    sequencer_recovered: bool,
}

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
    let readiness_database_url = cfg
        .database_url
        .as_deref()
        .context("SCRIPTUM_RELAY_DATABASE_URL must be set for readiness checks")?;
    let readiness_pool = create_pg_pool(readiness_database_url, PoolConfig::from_env())
        .await
        .context("failed to initialize relay PostgreSQL pool for readiness checks")?;
    check_pool_health(&readiness_pool)
        .await
        .context("relay PostgreSQL health check failed for readiness checks")?;
    let readiness_probe = Arc::new(ReadinessProbe::from_pool(readiness_pool.clone()));
    let sequencer = UpdateSequencer::new();
    sequencer
        .recover_from_max_server_seq(&readiness_pool)
        .await
        .context("failed to recover relay update sequencer from postgres")?;
    readiness_probe.mark_sequencer_recovered();
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
        readiness_probe,
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
    readiness_probe: Arc<ReadinessProbe>,
) -> Router {
    apply_middleware(
        Router::new()
            .route("/health", get(health))
            .route("/healthz", get(health))
            .route("/ready", get(ready))
            .merge(auth::oauth::router(oauth_state))
            .merge(ws::router(jwt_service, session_store, doc_store, Arc::new(awareness::AwarenessStore::default()), membership_store, ws_base_url))
            .merge(api_router),
    )
    .layer(Extension(readiness_probe))
}

fn apply_middleware(router: Router) -> Router {
    router
        .layer(cors::cors_layer())
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .layer(middleware::from_fn(request_context_middleware))
        .layer(middleware::from_fn(panic_handler))
}

async fn health() -> (StatusCode, &'static str) {
    (StatusCode::OK, "ok")
}

async fn ready(Extension(readiness_probe): Extension<Arc<ReadinessProbe>>) -> impl IntoResponse {
    let readiness = readiness_probe.evaluate().await;
    let status = if readiness.ready { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
    (status, Json(readiness))
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

    use super::{
        apply_middleware, build_router, DbCheckFuture, ReadinessProbe, MAX_REQUEST_BODY_BYTES,
    };
    use crate::{
        auth::{jwt::JwtAccessTokenService, oauth::OAuthState},
        error::REQUEST_ID_HEADER,
        ws::{DocSyncStore, SyncSessionStore, WorkspaceMembershipStore},
    };

    fn test_router(db_ready: bool, sequencer_ready: bool) -> Router {
        let jwt_service = Arc::new(
            JwtAccessTokenService::new("scriptum_test_secret_that_is_definitely_long_enough")
                .expect("test jwt service should initialize"),
        );
        let session_store = Arc::new(SyncSessionStore::default());
        let doc_store = Arc::new(DocSyncStore::default());
        let readiness_probe = Arc::new(ReadinessProbe::from_db_check(move || {
            if db_ready {
                Box::pin(async { Ok(()) }) as DbCheckFuture
            } else {
                Box::pin(async { Err(anyhow::anyhow!("db unavailable")) }) as DbCheckFuture
            }
        }));
        if sequencer_ready {
            readiness_probe.mark_sequencer_recovered();
        }
        build_router(
            jwt_service,
            session_store,
            doc_store,
            WorkspaceMembershipStore::for_tests(),
            OAuthState::from_env(),
            "ws://localhost:8080".to_string(),
            Router::new(),
            readiness_probe,
        )
    }

    #[tokio::test]
    async fn health_check_has_request_id_header() {
        let response = test_router(true, true)
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .expect("health request should build"),
            )
            .await
            .expect("health request should succeed");

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("x-request-id"));
    }

    #[tokio::test]
    async fn readiness_returns_service_unavailable_when_sequencer_not_recovered() {
        let response = test_router(true, false)
            .oneshot(
                Request::builder()
                    .uri("/ready")
                    .body(Body::empty())
                    .expect("ready request should build"),
            )
            .await
            .expect("ready request should return response");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("ready response body should read");
        let parsed: Value =
            serde_json::from_slice(&body).expect("ready response should be valid json");
        assert_eq!(parsed["ready"], false);
        assert_eq!(parsed["db_connected"], true);
        assert_eq!(parsed["sequencer_recovered"], false);
    }

    #[tokio::test]
    async fn readiness_returns_service_unavailable_when_database_is_unreachable() {
        let response = test_router(false, true)
            .oneshot(
                Request::builder()
                    .uri("/ready")
                    .body(Body::empty())
                    .expect("ready request should build"),
            )
            .await
            .expect("ready request should return response");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("ready response body should read");
        let parsed: Value =
            serde_json::from_slice(&body).expect("ready response should be valid json");
        assert_eq!(parsed["ready"], false);
        assert_eq!(parsed["db_connected"], false);
        assert_eq!(parsed["sequencer_recovered"], true);
    }

    #[tokio::test]
    async fn readiness_returns_ok_when_database_and_sequencer_are_ready() {
        let response = test_router(true, true)
            .oneshot(
                Request::builder()
                    .uri("/ready")
                    .body(Body::empty())
                    .expect("ready request should build"),
            )
            .await
            .expect("ready request should return response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("ready response body should read");
        let parsed: Value =
            serde_json::from_slice(&body).expect("ready response should be valid json");
        assert_eq!(parsed["ready"], true);
        assert_eq!(parsed["db_connected"], true);
        assert_eq!(parsed["sequencer_recovered"], true);
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

        let response = test_router(true, true)
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
