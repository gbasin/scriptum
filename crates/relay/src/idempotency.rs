// Idempotency key middleware for mutating POST requests.
//
// Extracts the `Idempotency-Key` header and checks an in-memory store
// for duplicate requests. Returns cached responses for matching keys,
// errors on payload hash mismatch, and stores new responses for TTL.

use axum::{
    body::{to_bytes, Body, Bytes},
    extract::FromRequestParts,
    http::{request::Parts, HeaderValue, Request, Response, StatusCode},
    middleware::Next,
    response::IntoResponse,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;

use crate::error::{ErrorCode, RelayError};

/// Header name for idempotency keys.
pub const IDEMPOTENCY_KEY_HEADER: &str = "idempotency-key";

/// Default TTL for cached idempotency entries.
const DEFAULT_TTL: Duration = Duration::from_secs(24 * 60 * 60); // 24 hours

/// Maximum body size we'll buffer for hashing (1 MiB).
const MAX_HASH_BODY_BYTES: usize = 1024 * 1024;

/// In-memory idempotency key store.
#[derive(Debug, Clone, Default)]
pub struct IdempotencyStore {
    entries: Arc<RwLock<HashMap<String, IdempotencyEntry>>>,
    ttl: Duration,
}

#[derive(Debug, Clone)]
struct IdempotencyEntry {
    /// SHA-256 hash of the request body.
    body_hash: String,
    /// Cached response status.
    status: StatusCode,
    /// Cached response body.
    body: Bytes,
    /// When this entry was created.
    created_at: Instant,
}

impl IdempotencyStore {
    pub fn new() -> Self {
        Self { entries: Arc::new(RwLock::new(HashMap::new())), ttl: DEFAULT_TTL }
    }

    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// Look up a cached response. Returns `None` if not found or expired.
    async fn get(&self, key: &str) -> Option<IdempotencyEntry> {
        let guard = self.entries.read().await;
        guard.get(key).and_then(|entry| {
            if entry.created_at.elapsed() < self.ttl {
                Some(entry.clone())
            } else {
                None
            }
        })
    }

    /// Store a response for a key.
    async fn insert(&self, key: String, entry: IdempotencyEntry) {
        let mut guard = self.entries.write().await;
        guard.insert(key, entry);
    }

    /// Remove expired entries. Call periodically for memory hygiene.
    pub async fn evict_expired(&self) -> usize {
        let mut guard = self.entries.write().await;
        let before = guard.len();
        guard.retain(|_, entry| entry.created_at.elapsed() < self.ttl);
        before - guard.len()
    }

    /// Number of cached entries (including potentially expired).
    pub async fn len(&self) -> usize {
        self.entries.read().await.len()
    }

    /// Whether there are no cached idempotency entries.
    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }
}

/// Axum middleware function for idempotency key enforcement.
///
/// For POST requests with an `Idempotency-Key` header:
/// - If a cached response exists with matching body hash, return it.
/// - If a cached response exists with different body hash, return 409 REPLAY_MISMATCH.
/// - Otherwise, process the request and cache the response.
///
/// Non-POST requests and requests without the header pass through unchanged.
pub async fn idempotency_middleware(request: Request<Body>, next: Next) -> Response<Body> {
    // Only apply to POST requests.
    if request.method() != axum::http::Method::POST {
        return next.run(request).await;
    }

    // Extract idempotency key header.
    let key = match request.headers().get(IDEMPOTENCY_KEY_HEADER) {
        Some(value) => match value.to_str() {
            Ok(s) => s.to_owned(),
            Err(_) => return next.run(request).await,
        },
        None => return next.run(request).await,
    };

    // Extract the store from extensions.
    let store = match request.extensions().get::<IdempotencyStore>() {
        Some(store) => store.clone(),
        None => return next.run(request).await,
    };

    // Buffer the body for hashing.
    let (parts, body) = request.into_parts();
    let body_bytes = match to_bytes(body, MAX_HASH_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return RelayError::new(
                ErrorCode::ValidationFailed,
                "request body too large for idempotency check",
            )
            .into_response();
        }
    };
    let body_hash = hash_body(&body_bytes);

    // Check cache.
    if let Some(entry) = store.get(&key).await {
        if entry.body_hash == body_hash {
            // Replay: return cached response.
            return Response::builder()
                .status(entry.status)
                .header("idempotency-key", &key)
                .header("idempotency-replay", "true")
                .body(Body::from(entry.body))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
        } else {
            // Different payload for same key: conflict.
            return RelayError::new(
                ErrorCode::DocPathConflict,
                "idempotency key reused with different request body",
            )
            .with_details(json!({ "idempotency_key": key }))
            .into_response();
        }
    }

    // Reconstruct request and process.
    let request = Request::from_parts(parts, Body::from(body_bytes.clone()));
    let response = next.run(request).await;

    // Cache the response.
    let (resp_parts, resp_body) = response.into_parts();
    let resp_bytes = to_bytes(resp_body, MAX_HASH_BODY_BYTES).await.unwrap_or_default();

    store
        .insert(
            key.clone(),
            IdempotencyEntry {
                body_hash,
                status: resp_parts.status,
                body: resp_bytes.clone(),
                created_at: Instant::now(),
            },
        )
        .await;

    let mut response = Response::from_parts(resp_parts, Body::from(resp_bytes));
    response.headers_mut().insert(
        "idempotency-key",
        HeaderValue::from_str(&key).unwrap_or(HeaderValue::from_static("")),
    );
    response
}

/// Axum extractor for the `Idempotency-Key` header value.
///
/// Does not reject if missing — returns `None`.
#[derive(Debug, Clone)]
pub struct IdempotencyKey(pub Option<String>);

impl<S> FromRequestParts<S> for IdempotencyKey
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let key = parts
            .headers
            .get(IDEMPOTENCY_KEY_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        Ok(IdempotencyKey(key))
    }
}

fn hash_body(body: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(body);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::post, Extension, Router};
    use tower::ServiceExt;

    async fn echo_handler(body: String) -> impl IntoResponse {
        body
    }

    fn test_app(store: IdempotencyStore) -> Router {
        Router::new()
            .route("/create", post(echo_handler))
            .layer(axum::middleware::from_fn(idempotency_middleware))
            .layer(Extension(store))
    }

    fn post_request(key: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method(axum::http::Method::POST)
            .uri("/create")
            .header(IDEMPOTENCY_KEY_HEADER, key)
            .body(Body::from(body.to_owned()))
            .unwrap()
    }

    async fn response_body(response: Response<Body>) -> String {
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn first_request_passes_through() {
        let store = IdempotencyStore::new();
        let app = test_app(store.clone());

        let response = app.oneshot(post_request("key-1", "hello")).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response_body(response).await, "hello");
    }

    #[tokio::test]
    async fn duplicate_request_returns_cached_response() {
        let store = IdempotencyStore::new();

        // First request.
        let app = test_app(store.clone());
        let resp1 = app.oneshot(post_request("key-2", "data")).await.unwrap();
        assert_eq!(resp1.status(), StatusCode::OK);

        // Second request with same key and body.
        let app = test_app(store.clone());
        let resp2 = app.oneshot(post_request("key-2", "data")).await.unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);
        assert_eq!(resp2.headers().get("idempotency-replay").unwrap(), "true");
    }

    #[tokio::test]
    async fn different_body_same_key_returns_conflict() {
        let store = IdempotencyStore::new();

        // First request.
        let app = test_app(store.clone());
        let _resp1 = app.oneshot(post_request("key-3", "body-a")).await.unwrap();

        // Second request with same key but different body.
        let app = test_app(store.clone());
        let resp2 = app.oneshot(post_request("key-3", "body-b")).await.unwrap();
        assert_eq!(resp2.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn no_header_passes_through() {
        let store = IdempotencyStore::new();
        let app = test_app(store.clone());

        let request = Request::builder()
            .method(axum::http::Method::POST)
            .uri("/create")
            .body(Body::from("no key"))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Nothing cached.
        assert_eq!(store.len().await, 0);
    }

    #[tokio::test]
    async fn get_requests_bypass_middleware() {
        let store = IdempotencyStore::new();
        let app = Router::new()
            .route("/get", axum::routing::get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(idempotency_middleware))
            .layer(Extension(store.clone()));

        let request = Request::builder()
            .method(axum::http::Method::GET)
            .uri("/get")
            .header(IDEMPOTENCY_KEY_HEADER, "ignored")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(store.len().await, 0);
    }

    #[tokio::test]
    async fn expired_entries_are_evicted() {
        let store = IdempotencyStore::new().with_ttl(Duration::from_millis(1));

        // Store an entry.
        let app = test_app(store.clone());
        let _resp = app.oneshot(post_request("key-4", "data")).await.unwrap();
        assert_eq!(store.len().await, 1);

        // Wait for expiry.
        tokio::time::sleep(Duration::from_millis(10)).await;

        let evicted = store.evict_expired().await;
        assert_eq!(evicted, 1);
        assert_eq!(store.len().await, 0);
    }

    #[tokio::test]
    async fn expired_entry_allows_new_request() {
        let store = IdempotencyStore::new().with_ttl(Duration::from_millis(1));

        // First request.
        let app = test_app(store.clone());
        let _resp = app.oneshot(post_request("key-5", "first")).await.unwrap();

        // Wait for expiry.
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Same key, different body — should pass (expired).
        let app = test_app(store.clone());
        let resp = app.oneshot(post_request("key-5", "second")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().get("idempotency-replay").is_none());
    }

    #[tokio::test]
    async fn response_includes_idempotency_key_header() {
        let store = IdempotencyStore::new();
        let app = test_app(store);

        let resp = app.oneshot(post_request("my-key", "data")).await.unwrap();
        assert_eq!(resp.headers().get("idempotency-key").unwrap(), "my-key");
    }

    #[test]
    fn hash_body_is_deterministic() {
        let h1 = hash_body(b"test data");
        let h2 = hash_body(b"test data");
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_body_differs_for_different_data() {
        let h1 = hash_body(b"data-a");
        let h2 = hash_body(b"data-b");
        assert_ne!(h1, h2);
    }
}
