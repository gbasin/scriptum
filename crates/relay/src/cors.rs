// CORS middleware configuration for the relay server.
//
// Reads allowed origins from the `SCRIPTUM_RELAY_CORS_ORIGINS` environment
// variable (comma-separated). Falls back to permissive localhost defaults
// in development.

use axum::http::{HeaderName, HeaderValue, Method};
use tower_http::cors::{AllowOrigin, CorsLayer};

/// Default origins allowed when `SCRIPTUM_RELAY_CORS_ORIGINS` is unset.
const DEFAULT_DEV_ORIGINS: &[&str] = &[
    "http://localhost:3000",
    "http://localhost:5173",
    "http://127.0.0.1:3000",
    "http://127.0.0.1:5173",
    "tauri://localhost",
];

/// Environment variable that overrides the allowed origin list.
const CORS_ORIGINS_ENV: &str = "SCRIPTUM_RELAY_CORS_ORIGINS";

/// Build a [`CorsLayer`] from the environment.
///
/// - If `SCRIPTUM_RELAY_CORS_ORIGINS` is set to `"*"`, allows any origin.
/// - If set to a comma-separated list, allows exactly those origins.
/// - If unset, allows the default development origins.
///
/// All configurations:
/// - Allow credentials (cookies, Authorization header).
/// - Allow common HTTP methods (GET, POST, PUT, PATCH, DELETE, OPTIONS).
/// - Allow common headers (Content-Type, Authorization, X-Request-Id, If-Match).
/// - Expose X-Request-Id to the browser.
/// - Cache preflight responses for 1 hour.
pub fn cors_layer() -> CorsLayer {
    cors_layer_from_env(std::env::var(CORS_ORIGINS_ENV).ok())
}

fn cors_layer_from_env(env_value: Option<String>) -> CorsLayer {
    let base = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
            HeaderName::from_static("x-request-id"),
            HeaderName::from_static("if-match"),
        ])
        .expose_headers([HeaderName::from_static("x-request-id")])
        .allow_credentials(true)
        .max_age(std::time::Duration::from_secs(3600));

    match env_value.as_deref() {
        Some("*") => base.allow_origin(AllowOrigin::any()).allow_credentials(false),
        Some(origins) => {
            let parsed = parse_origins(origins);
            base.allow_origin(parsed)
        }
        None => {
            let defaults = parse_origins(&DEFAULT_DEV_ORIGINS.join(","));
            base.allow_origin(defaults)
        }
    }
}

fn parse_origins(comma_separated: &str) -> Vec<HeaderValue> {
    comma_separated
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|s| HeaderValue::from_str(s).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request, response::IntoResponse, routing::get, Router};
    use tower::ServiceExt;

    async fn ok_handler() -> impl IntoResponse {
        "ok"
    }

    fn test_app(env_value: Option<String>) -> Router {
        Router::new().route("/test", get(ok_handler)).layer(cors_layer_from_env(env_value))
    }

    #[tokio::test]
    async fn preflight_returns_cors_headers_for_allowed_origin() {
        let app = test_app(None); // default dev origins

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/test")
                    .header("origin", "http://localhost:3000")
                    .header("access-control-request-method", "POST")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.headers().get("access-control-allow-origin").unwrap(),
            "http://localhost:3000"
        );
        assert!(response
            .headers()
            .get("access-control-allow-credentials")
            .unwrap()
            .to_str()
            .unwrap()
            .contains("true"));
    }

    #[tokio::test]
    async fn preflight_rejects_unknown_origin() {
        let app = test_app(None);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/test")
                    .header("origin", "https://evil.example.com")
                    .header("access-control-request-method", "POST")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert!(response.headers().get("access-control-allow-origin").is_none());
    }

    #[tokio::test]
    async fn custom_origins_from_env() {
        let app = test_app(Some("https://app.scriptum.dev,https://staging.scriptum.dev".into()));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/test")
                    .header("origin", "https://app.scriptum.dev")
                    .header("access-control-request-method", "GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.headers().get("access-control-allow-origin").unwrap(),
            "https://app.scriptum.dev"
        );
    }

    #[tokio::test]
    async fn wildcard_origin_disables_credentials() {
        let app = test_app(Some("*".into()));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/test")
                    .header("origin", "https://anything.example.com")
                    .header("access-control-request-method", "GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.headers().get("access-control-allow-origin").unwrap(), "*");
        // Credentials must be false when origin is wildcard.
        assert!(response.headers().get("access-control-allow-credentials").is_none());
    }

    #[tokio::test]
    async fn simple_get_includes_cors_on_response() {
        let app = test_app(None);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/test")
                    .header("origin", "http://localhost:5173")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.headers().get("access-control-allow-origin").unwrap(),
            "http://localhost:5173"
        );
    }

    #[tokio::test]
    async fn max_age_is_set() {
        let app = test_app(None);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/test")
                    .header("origin", "http://localhost:3000")
                    .header("access-control-request-method", "POST")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.headers().get("access-control-max-age").unwrap(), "3600");
    }

    #[tokio::test]
    async fn parse_origins_handles_whitespace() {
        let origins = parse_origins("  https://a.com , https://b.com  , ");
        assert_eq!(origins.len(), 2);
        assert_eq!(origins[0], "https://a.com");
        assert_eq!(origins[1], "https://b.com");
    }

    #[tokio::test]
    async fn default_dev_origins_include_vite_and_next() {
        let origins = parse_origins(&DEFAULT_DEV_ORIGINS.join(","));
        assert_eq!(origins.len(), 5);
    }

    #[tokio::test]
    async fn tauri_origin_allowed_in_defaults() {
        let app = test_app(None);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/test")
                    .header("origin", "tauri://localhost")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.headers().get("access-control-allow-origin").unwrap(),
            "tauri://localhost"
        );
    }
}
