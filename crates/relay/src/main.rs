#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::manual_strip)]
#![allow(clippy::needless_borrows_for_generic_args)]
#![allow(clippy::result_large_err)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::trim_split_whitespace)]
#![allow(clippy::type_complexity)]
#![allow(clippy::unnecessary_get_then_check)]

mod api;
mod audit;
mod auth;
mod awareness;
pub mod config;
mod cors;
mod db;
mod error;
pub mod etag;
pub mod idempotency;
mod leader;
mod metrics;
mod protocol;
mod sync;
pub mod validation;
mod ws;

use anyhow::Context;
use axum::{
    body::Body,
    extract::{DefaultBodyLimit, State},
    http::{
        header::{AUTHORIZATION, CONTENT_TYPE},
        HeaderMap, HeaderValue, Request, StatusCode,
    },
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Extension, Json, Router,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{sync::Arc, time::Instant};
use tokio::net::TcpListener;
use tracing::{error, info, info_span, Instrument};
use uuid::Uuid;
use ws::{DocSyncStore, SyncSessionStore};

use crate::auth::{jwt::JwtAccessTokenService, oauth::OAuthState};
use crate::db::pool::{check_pool_health, create_pg_pool, PoolConfig};
use crate::error::{
    attach_request_id_header, attach_trace_id_header, default_code_for_status,
    request_id_from_headers_or_generate, trace_id_from_headers_or_generate, with_request_id_scope,
    with_trace_id_scope, ErrorCode, RelayError, REQUEST_ID_HEADER, TRACE_ID_HEADER,
};
use crate::metrics::{set_global_metrics, RelayMetrics};
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
    cfg.validate_security().context("relay transport security configuration is invalid")?;

    tracing_subscriber::fmt()
        .json()
        .flatten_event(true)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&cfg.log_filter)),
        )
        .init();

    if cfg.is_dev_jwt_secret() {
        tracing::warn!(
            "using development JWT secret — set SCRIPTUM_RELAY_JWT_SECRET in production"
        );
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
    let metrics = Arc::new(RelayMetrics::default());
    set_global_metrics(Arc::clone(&metrics));
    let recovery_started_at = std::time::Instant::now();
    sequencer
        .recover_from_max_server_seq(&readiness_pool)
        .await
        .context("failed to recover relay update sequencer from postgres")?;
    metrics.set_daemon_recovery_time_ms(recovery_started_at.elapsed().as_millis() as u64);
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
        cfg.ws_base_url.clone(),
        api::build_router_from_env(jwt_service, oauth_state)
            .await
            .context("failed to build relay workspace API router")?,
        readiness_probe,
        metrics,
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
    ws_base_url: String,
    api_router: Router,
    readiness_probe: Arc<ReadinessProbe>,
    metrics: Arc<RelayMetrics>,
) -> Router {
    apply_middleware(
        Router::new()
            .route("/health", get(health))
            .route("/healthz", get(health))
            .route("/ready", get(ready))
            .route("/metrics", get(prometheus_metrics))
            .merge(ws::router(
                jwt_service,
                session_store,
                doc_store,
                Arc::new(awareness::AwarenessStore::default()),
                membership_store,
                ws_base_url,
            ))
            .merge(api_router),
        Arc::clone(&metrics),
    )
    .layer(Extension(readiness_probe))
    .layer(Extension(metrics))
}

fn apply_middleware(router: Router, metrics: Arc<RelayMetrics>) -> Router {
    router
        .layer(cors::cors_layer())
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .layer(middleware::from_fn_with_state(metrics, request_context_middleware))
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

async fn prometheus_metrics(Extension(metrics): Extension<Arc<RelayMetrics>>) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
        metrics.render_prometheus(),
    )
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

async fn panic_handler(mut request: Request<Body>, next: Next) -> Response {
    let request_id = request_id_from_headers_or_generate(request.headers());
    let trace_id = trace_id_from_headers_or_generate(request.headers());
    ensure_request_header(&mut request, REQUEST_ID_HEADER, &request_id);
    ensure_request_header(&mut request, TRACE_ID_HEADER, &trace_id);

    match tokio::spawn(with_trace_id_scope(
        trace_id.clone(),
        with_request_id_scope(request_id.clone(), async move { next.run(request).await }),
    ))
    .await
    {
        Ok(response) => response,
        Err(join_error) => {
            error!(
                ?join_error,
                request_id = %request_id,
                trace_id = %trace_id,
                "request handling panicked"
            );
            let mut response = RelayError::from_code(ErrorCode::InternalError)
                .with_request_id(request_id)
                .into_response();
            attach_trace_id_header(&mut response, &trace_id);
            response
        }
    }
}

async fn request_context_middleware(
    State(metrics): State<Arc<RelayMetrics>>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let request_id = request_id_from_headers_or_generate(request.headers());
    let trace_id = trace_id_from_headers_or_generate(request.headers());
    ensure_request_header(&mut request, REQUEST_ID_HEADER, &request_id);
    ensure_request_header(&mut request, TRACE_ID_HEADER, &trace_id);

    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let endpoint = format!("{method} {path}");
    let workspace_id = workspace_id_from_path(&path).map(|id| id.to_string());
    let actor_hash = actor_hash_from_headers(request.headers());
    let started_at = Instant::now();

    let request_span = info_span!(
        "relay.http.request",
        request_id = %request_id,
        trace_id = %trace_id,
        method = %method,
        path = %path
    );
    let mut response = with_trace_id_scope(
        trace_id.clone(),
        with_request_id_scope(request_id.clone(), next.run(request)),
    )
    .instrument(request_span)
    .await;
    attach_request_id_header(&mut response, &request_id);
    attach_trace_id_header(&mut response, &trace_id);
    let status = response.status();
    let error_code = response_error_code(&response);

    info!(
        request_id = %request_id,
        trace_id = %trace_id,
        workspace_id = workspace_id.as_deref().unwrap_or(""),
        actor_hash = actor_hash.as_deref().unwrap_or(""),
        endpoint = %endpoint,
        error_code = error_code.unwrap_or(""),
        status = status.as_u16(),
        latency_ms = started_at.elapsed().as_millis() as u64,
        "relay_request"
    );

    metrics.record_http_request(
        method.as_str(),
        &path,
        status.as_u16(),
        started_at.elapsed().as_millis() as u64,
    );

    response
}

fn ensure_request_header(request: &mut Request<Body>, header_name: &'static str, value: &str) {
    if request.headers().contains_key(header_name) {
        return;
    }
    if let Ok(header_value) = HeaderValue::from_str(value) {
        request.headers_mut().insert(header_name, header_value);
    }
}

fn workspace_id_from_path(path: &str) -> Option<Uuid> {
    let mut segments = path.trim_start_matches('/').split('/');
    while let Some(segment) = segments.next() {
        if segment == "workspaces" {
            let value = segments.next()?;
            return Uuid::parse_str(value).ok();
        }
    }
    None
}

fn actor_hash_from_headers(headers: &HeaderMap) -> Option<String> {
    let token = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_bearer_token)?;
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    Some(format!("{:x}", hasher.finalize()))
}

fn parse_bearer_token(value: &str) -> Option<&str> {
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    if token.is_empty() {
        return None;
    }
    Some(token)
}

fn response_error_code(response: &Response) -> Option<&'static str> {
    if let Some(code) = response.extensions().get::<ErrorCode>() {
        return Some(code.as_str());
    }
    let status = response.status();
    if status.is_client_error() || status.is_server_error() {
        return Some(default_code_for_status(status).as_str());
    }
    None
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        body::{to_bytes, Body},
        http::{
            header::{AUTHORIZATION, CONTENT_TYPE},
            HeaderMap, Method, Request, StatusCode,
        },
        response::IntoResponse,
        routing::{get, post},
        Router,
    };
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    use tower::ServiceExt;
    use uuid::Uuid;

    use super::{
        actor_hash_from_headers, apply_middleware, build_router, response_error_code,
        workspace_id_from_path, DbCheckFuture, ReadinessProbe, MAX_REQUEST_BODY_BYTES,
    };
    use crate::{
        auth::jwt::JwtAccessTokenService,
        error::{ErrorCode, REQUEST_ID_HEADER, TRACE_ID_HEADER},
        metrics::RelayMetrics,
        validation::ValidatedJson,
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
        let metrics = Arc::new(RelayMetrics::default());
        build_router(
            jwt_service,
            session_store,
            doc_store,
            WorkspaceMembershipStore::for_tests(),
            "ws://localhost:8080".to_string(),
            Router::new(),
            readiness_probe,
            metrics,
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
        assert!(response.headers().contains_key(REQUEST_ID_HEADER));
        assert!(response.headers().contains_key(TRACE_ID_HEADER));
    }

    #[tokio::test]
    async fn health_check_reuses_inbound_trace_id_header() {
        let response = test_router(true, true)
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .header(TRACE_ID_HEADER, "trace-health-123")
                    .body(Body::empty())
                    .expect("health request should build"),
            )
            .await
            .expect("health request should succeed");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get(TRACE_ID_HEADER).unwrap(), "trace-health-123");
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

        let app = apply_middleware(
            Router::new().route("/panic", get(panic_route)),
            Arc::new(RelayMetrics::default()),
        );

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/panic")
                    .header(REQUEST_ID_HEADER, "req-panic-123")
                    .header(TRACE_ID_HEADER, "trace-panic-456")
                    .body(Body::empty())
                    .expect("panic request should build"),
            )
            .await
            .expect("panic request should return a response");

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(response.headers().get(REQUEST_ID_HEADER).unwrap(), "req-panic-123");
        assert_eq!(response.headers().get(TRACE_ID_HEADER).unwrap(), "trace-panic-456");

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
        let app = apply_middleware(
            Router::new().route("/echo", post(echo)),
            Arc::new(RelayMetrics::default()),
        );

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

    #[tokio::test]
    async fn invalid_json_uses_structured_error_envelope_with_request_id() {
        async fn validated_endpoint(ValidatedJson(_): ValidatedJson<Value>) -> StatusCode {
            StatusCode::NO_CONTENT
        }

        let app = apply_middleware(
            Router::new().route("/validated", post(validated_endpoint)),
            Arc::new(RelayMetrics::default()),
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/validated")
                    .header(REQUEST_ID_HEADER, "req-invalid-json-123")
                    .header("content-type", "application/json")
                    .body(Body::from("{\"email\":"))
                    .expect("validated request should build"),
            )
            .await
            .expect("validated request should return response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers().get(REQUEST_ID_HEADER).unwrap(), "req-invalid-json-123");

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("invalid json response body should read");
        let parsed: Value =
            serde_json::from_slice(&body).expect("invalid json response should be valid json");
        assert_eq!(parsed["error"]["code"], "VALIDATION_FAILED");
        assert_eq!(parsed["error"]["retryable"], false);
        assert_eq!(parsed["error"]["request_id"], "req-invalid-json-123");
        assert_eq!(parsed["error"]["details"]["kind"], "syntax_error");
    }

    #[tokio::test]
    async fn missing_content_type_uses_structured_error_envelope_with_details() {
        async fn validated_endpoint(ValidatedJson(_): ValidatedJson<Value>) -> StatusCode {
            StatusCode::NO_CONTENT
        }

        let app = apply_middleware(
            Router::new().route("/validated", post(validated_endpoint)),
            Arc::new(RelayMetrics::default()),
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/validated")
                    .header(REQUEST_ID_HEADER, "req-missing-content-type-123")
                    .body(Body::from("{\"ok\":true}"))
                    .expect("validated request should build"),
            )
            .await
            .expect("validated request should return response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response.headers().get(REQUEST_ID_HEADER).unwrap(),
            "req-missing-content-type-123"
        );

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("missing content-type response body should read");
        let parsed: Value = serde_json::from_slice(&body)
            .expect("missing content-type response should be valid json");
        assert_eq!(parsed["error"]["code"], "VALIDATION_FAILED");
        assert_eq!(parsed["error"]["retryable"], false);
        assert_eq!(parsed["error"]["request_id"], "req-missing-content-type-123");
        assert_eq!(parsed["error"]["details"]["kind"], "missing_content_type");
    }

    #[test]
    fn workspace_id_from_path_extracts_workspace_uuid() {
        let workspace_id = Uuid::new_v4();
        let path = format!("/v1/workspaces/{workspace_id}/documents");
        assert_eq!(workspace_id_from_path(&path), Some(workspace_id));
    }

    #[test]
    fn workspace_id_from_path_rejects_invalid_uuid() {
        assert_eq!(workspace_id_from_path("/v1/workspaces/not-a-uuid/documents"), None);
    }

    #[test]
    fn actor_hash_from_headers_hashes_bearer_token() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, "Bearer token-abc-123".parse().expect("header is valid"));

        let actor_hash = actor_hash_from_headers(&headers).expect("hash should be produced");
        assert_eq!(actor_hash, format!("{:x}", Sha256::digest(b"token-abc-123")));
    }

    #[test]
    fn actor_hash_from_headers_skips_missing_or_invalid_auth() {
        assert_eq!(actor_hash_from_headers(&HeaderMap::new()), None);

        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, "Basic abcdef".parse().expect("header is valid"));
        assert_eq!(actor_hash_from_headers(&headers), None);
    }

    #[test]
    fn response_error_code_uses_response_extension_when_present() {
        let mut response = StatusCode::UNAUTHORIZED.into_response();
        response.extensions_mut().insert(ErrorCode::RateLimited);
        assert_eq!(response_error_code(&response), Some("RATE_LIMITED"));
    }

    #[test]
    fn response_error_code_falls_back_to_status_mapping() {
        let response = StatusCode::UNAUTHORIZED.into_response();
        assert_eq!(response_error_code(&response), Some("AUTH_INVALID_TOKEN"));
    }

    #[tokio::test]
    async fn metrics_endpoint_exposes_red_and_custom_metrics() {
        let app = test_router(true, true);

        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .expect("health request should build"),
            )
            .await
            .expect("health request should succeed");

        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/does-not-exist")
                    .body(Body::empty())
                    .expect("404 request should build"),
            )
            .await
            .expect("404 request should return response");

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .expect("metrics request should build"),
            )
            .await
            .expect("metrics request should return response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .expect("metrics content-type should be present"),
            "text/plain; version=0.0.4; charset=utf-8"
        );

        let body =
            to_bytes(response.into_body(), usize::MAX).await.expect("metrics body should read");
        let rendered =
            String::from_utf8(body.to_vec()).expect("metrics body should be valid utf-8");

        assert!(rendered.contains("relay_request_rate_total"));
        assert!(rendered.contains("relay_request_errors_total"));
        assert!(rendered.contains("relay_request_duration_ms_sum"));
        assert!(rendered.contains("relay_request_duration_ms_count"));
        assert!(rendered.contains("relay_ws_rate_total"));
        assert!(rendered.contains("relay_ws_errors_total"));
        assert!(rendered.contains("relay_ws_duration_ms_sum"));
        assert!(rendered.contains("relay_ws_duration_ms_count"));
        assert!(rendered.contains("sync_ack_latency_ms"));
        assert!(rendered.contains("outbox_depth{workspace_id=\"unknown\"} 0"));
        assert!(rendered.contains("daemon_recovery_time_ms"));
        assert!(rendered.contains("git_sync_jobs_total{state=\"queued\"} 0"));
        assert!(rendered.contains("git_sync_jobs_total{state=\"running\"} 0"));
        assert!(rendered.contains("git_sync_jobs_total{state=\"completed\"} 0"));
        assert!(rendered.contains("git_sync_jobs_total{state=\"failed\"} 0"));
        assert!(rendered.contains("sequence_gap_count"));
        assert!(rendered.contains("endpoint=\"/health\""));
    }
}
