// ETag generation, comparison, and If-Match extraction.
//
// Provides a shared `IfMatchHeader` Axum extractor and utilities for
// conditional writes across all API endpoints.

use axum::{
    extract::FromRequestParts,
    http::{header::IF_MATCH, request::Parts, StatusCode},
    response::{IntoResponse, Response},
};
use uuid::Uuid;

use crate::error::{ErrorCode, RelayError};

/// Axum extractor that pulls the `If-Match` header value.
///
/// Returns 428 Precondition Required if the header is absent.
/// The extracted value is the raw header string (before normalization).
#[derive(Debug, Clone)]
pub struct IfMatchHeader(pub String);

impl<S> FromRequestParts<S> for IfMatchHeader
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let value = parts
            .headers
            .get(IF_MATCH)
            .ok_or_else(|| RelayError::from_code(ErrorCode::PreconditionRequired).into_response())?
            .to_str()
            .map_err(|_| {
                RelayError::new(ErrorCode::ValidationFailed, "If-Match header is not valid utf-8")
                    .into_response()
            })?
            .to_owned();

        Ok(IfMatchHeader(value))
    }
}

impl IfMatchHeader {
    /// Check whether this If-Match value matches the given current etag.
    ///
    /// Returns `Ok(())` on match or wildcard `"*"`.
    /// Returns `Err(412)` on mismatch.
    pub fn require_match(&self, current_etag: &str) -> Result<(), Response> {
        if etag_matches(&self.0, current_etag) {
            Ok(())
        } else {
            Err(RelayError::new(
                ErrorCode::EditPreconditionFailed,
                "If-Match does not match current resource state",
            )
            .into_response())
        }
    }

    /// Raw header value.
    pub fn value(&self) -> &str {
        &self.0
    }
}

/// Generate an opaque etag from a UUID v4.
///
/// Format: `"<uuid>"` (with quotes, per RFC 7232).
pub fn generate_etag() -> String {
    format!("\"{}\"", Uuid::new_v4())
}

/// Generate an etag from specific input values (e.g. ID + timestamp).
///
/// This produces a deterministic etag for the given components.
pub fn etag_from_parts(id: Uuid, updated_at: chrono::DateTime<chrono::Utc>) -> String {
    format!("\"{}:{}\"", id, updated_at.timestamp_millis())
}

/// Compare an If-Match header value against the current etag.
///
/// - `"*"` always matches.
/// - Weak etags (`W/`) are stripped before comparison.
/// - Surrounding quotes are stripped.
pub fn etag_matches(if_match: &str, current_etag: &str) -> bool {
    if if_match.trim() == "*" {
        return true;
    }
    normalize_etag(if_match) == normalize_etag(current_etag)
}

/// Strip optional `W/` prefix and surrounding quotes from an etag value.
pub fn normalize_etag(value: &str) -> &str {
    let trimmed = value.trim();
    let without_weak = trimmed.strip_prefix("W/").unwrap_or(trimmed);
    without_weak.strip_prefix('"').and_then(|v| v.strip_suffix('"')).unwrap_or(without_weak)
}

/// Return the response status code for a missing or invalid If-Match.
///
/// Use this in custom middleware or handlers that need explicit control.
pub fn precondition_status(has_header: bool) -> StatusCode {
    if has_header {
        StatusCode::PRECONDITION_FAILED // 412
    } else {
        StatusCode::PRECONDITION_REQUIRED // 428
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Method, Request},
        response::IntoResponse,
        routing::patch,
        Router,
    };
    use tower::ServiceExt;

    // ── generate_etag ──────────────────────────────────────────────

    #[test]
    fn generate_etag_is_quoted_uuid() {
        let etag = generate_etag();
        assert!(etag.starts_with('"'));
        assert!(etag.ends_with('"'));
        // Inner value should be a valid UUID.
        let inner = &etag[1..etag.len() - 1];
        assert!(Uuid::parse_str(inner).is_ok());
    }

    #[test]
    fn generate_etag_is_unique() {
        let a = generate_etag();
        let b = generate_etag();
        assert_ne!(a, b);
    }

    // ── etag_from_parts ────────────────────────────────────────────

    #[test]
    fn etag_from_parts_is_deterministic() {
        let id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let ts = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let a = etag_from_parts(id, ts);
        let b = etag_from_parts(id, ts);
        assert_eq!(a, b);
    }

    #[test]
    fn etag_from_parts_changes_with_time() {
        let id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let t1 = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let t2 = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:01Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_ne!(etag_from_parts(id, t1), etag_from_parts(id, t2));
    }

    // ── normalize_etag ─────────────────────────────────────────────

    #[test]
    fn normalize_strips_quotes() {
        assert_eq!(normalize_etag("\"abc\""), "abc");
    }

    #[test]
    fn normalize_strips_weak_prefix() {
        assert_eq!(normalize_etag("W/\"abc\""), "abc");
    }

    #[test]
    fn normalize_handles_unquoted() {
        assert_eq!(normalize_etag("abc"), "abc");
    }

    #[test]
    fn normalize_handles_whitespace() {
        assert_eq!(normalize_etag("  \"abc\"  "), "abc");
    }

    // ── etag_matches ───────────────────────────────────────────────

    #[test]
    fn wildcard_always_matches() {
        assert!(etag_matches("*", "\"anything\""));
        assert!(etag_matches(" * ", "\"anything\""));
    }

    #[test]
    fn exact_match_with_quotes() {
        assert!(etag_matches("\"abc\"", "\"abc\""));
    }

    #[test]
    fn mismatch_returns_false() {
        assert!(!etag_matches("\"abc\"", "\"def\""));
    }

    #[test]
    fn weak_etag_match() {
        assert!(etag_matches("W/\"abc\"", "\"abc\""));
    }

    #[test]
    fn quoted_vs_unquoted_match() {
        assert!(etag_matches("abc", "\"abc\""));
    }

    // ── IfMatchHeader extractor ────────────────────────────────────

    async fn patch_handler(if_match: IfMatchHeader) -> impl IntoResponse {
        let current = "\"current-etag\"";
        if let Err(resp) = if_match.require_match(current) {
            return resp;
        }
        "ok".into_response()
    }

    fn test_app() -> Router {
        Router::new().route("/resource", patch(patch_handler))
    }

    #[tokio::test]
    async fn missing_if_match_returns_428() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri("/resource")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::PRECONDITION_REQUIRED);
    }

    #[tokio::test]
    async fn mismatched_if_match_returns_412() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri("/resource")
                    .header("If-Match", "\"wrong-etag\"")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::PRECONDITION_FAILED);
    }

    #[tokio::test]
    async fn matching_if_match_returns_ok() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri("/resource")
                    .header("If-Match", "\"current-etag\"")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn wildcard_if_match_returns_ok() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri("/resource")
                    .header("If-Match", "*")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    // ── precondition_status ────────────────────────────────────────

    #[test]
    fn precondition_status_missing_is_428() {
        assert_eq!(precondition_status(false), StatusCode::PRECONDITION_REQUIRED);
    }

    #[test]
    fn precondition_status_present_is_412() {
        assert_eq!(precondition_status(true), StatusCode::PRECONDITION_FAILED);
    }
}
