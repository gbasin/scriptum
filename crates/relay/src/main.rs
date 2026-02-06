mod api;
mod audit;
mod auth;
mod awareness;
mod db;
mod leader;
mod sync;
mod ws;

use anyhow::Context;
use axum::{
    body::Body,
    extract::DefaultBodyLimit,
    http::{header::HeaderValue, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use std::time::Instant;
use tokio::net::TcpListener;
use tracing::{error, info};
use uuid::Uuid;

const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;
const REQUEST_ID_HEADER: &str = "x-request-id";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let app = build_router();
    let addr = "0.0.0.0:8080";
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind relay listener on {addr}"))?;

    info!(listen_addr = addr, "starting relay server");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("relay server exited unexpectedly")
}

fn build_router() -> Router {
    apply_middleware(Router::new().route("/healthz", get(healthz)))
}

fn apply_middleware(router: Router) -> Router {
    router
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
    match tokio::spawn(async move { next.run(request).await }).await {
        Ok(response) => response,
        Err(join_error) => {
            error!(?join_error, "request handling panicked");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn request_context_middleware(request: Request<Body>, next: Next) -> Response {
    let request_id = request
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let started_at = Instant::now();

    let mut response = next.run(request).await;

    if let Ok(request_id_header) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert(REQUEST_ID_HEADER, request_id_header);
    }

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
    use axum::{
        body::Body,
        http::{Method, Request, StatusCode},
        routing::{get, post},
        Router,
    };
    use tower::ServiceExt;

    use super::{apply_middleware, build_router, MAX_REQUEST_BODY_BYTES};

    #[tokio::test]
    async fn health_check_has_request_id_header() {
        let response = build_router()
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
                    .body(Body::empty())
                    .expect("panic request should build"),
            )
            .await
            .expect("panic request should return a response");

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
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
}
