use std::{
    collections::{HashMap, VecDeque},
    env,
    future::Future,
    pin::Pin,
    sync::Arc,
    time::{Duration as StdDuration, Instant},
};

use axum::{
    extract::{Json, State},
    http::{header::RETRY_AFTER, HeaderValue},
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;
use url::Url;
use uuid::Uuid;

use crate::error::{ErrorCode, RelayError};

const DEFAULT_GITHUB_AUTH_URL: &str = "https://github.com/login/oauth/authorize";
const DEFAULT_GITHUB_SCOPE: &str = "read:user user:email";
const DEFAULT_GITHUB_CLIENT_ID: &str = "scriptum-dev-github-client-id";
const DEFAULT_FLOW_TTL_MINUTES: i64 = 10;
const DEFAULT_RATE_LIMIT_WINDOW_SECS: u64 = 60;
const DEFAULT_RATE_LIMIT_MAX_REQUESTS: usize = 30;
const MAX_STATE_LEN: usize = 512;
const MIN_CODE_CHALLENGE_LEN: usize = 43;
const MAX_CODE_CHALLENGE_LEN: usize = 128;

const DEFAULT_GITHUB_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const DEFAULT_ACCESS_TOKEN_TTL_SECONDS: i64 = 15 * 60;
const DEFAULT_REFRESH_TOKEN_TTL_DAYS: i64 = 30;
const REFRESH_TOKEN_BYTES: usize = 32;

/// Information returned by GitHub after code exchange.
#[derive(Debug, Clone)]
pub struct GithubTokenResponse {
    pub access_token: String,
}

/// GitHub user profile information.
#[derive(Debug, Clone)]
pub struct GithubUserInfo {
    pub email: String,
    pub display_name: String,
}

/// Trait for GitHub OAuth API calls (exchanging codes, fetching user info).
/// Using boxed futures for object safety / dynamic dispatch in tests.
pub trait GithubExchange: Send + Sync {
    fn exchange_code(
        &self,
        code: &str,
        client_id: &str,
        client_secret: &str,
        redirect_uri: &str,
    ) -> Pin<Box<dyn Future<Output = Result<GithubTokenResponse, RelayError>> + Send>>;

    fn get_user_info(
        &self,
        access_token: &str,
    ) -> Pin<Box<dyn Future<Output = Result<GithubUserInfo, RelayError>> + Send>>;
}

/// User record as returned after create-or-find.
#[derive(Debug, Clone, Serialize)]
pub struct OAuthUser {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
}

/// In-memory refresh token store entry.
#[derive(Debug, Clone)]
struct RefreshSession {
    user_id: Uuid,
    token_hash: Vec<u8>,
    family_id: Uuid,
    expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

/// In-memory store for refresh tokens (will be replaced by PostgreSQL).
#[derive(Debug, Default)]
pub struct RefreshTokenStore {
    sessions: RwLock<HashMap<Uuid, RefreshSession>>,
    /// Maps user_id → list of session IDs in the same family.
    users: RwLock<HashMap<Uuid, Vec<Uuid>>>,
}

impl RefreshTokenStore {
    async fn insert(
        &self,
        session_id: Uuid,
        user_id: Uuid,
        token_hash: Vec<u8>,
        family_id: Uuid,
        expires_at: DateTime<Utc>,
    ) {
        let session =
            RefreshSession { user_id, token_hash, family_id, expires_at, revoked_at: None };
        self.sessions.write().await.insert(session_id, session);
        self.users.write().await.entry(user_id).or_default().push(session_id);
    }
}

#[derive(Clone)]
pub struct OAuthState {
    flow_store: Arc<OAuthFlowStore>,
    rate_limiter: Arc<OAuthStartRateLimiter>,
    github_client_id: String,
    github_client_secret: String,
    github_authorize_url: String,
    github_token_url: String,
    github_scope: String,
    flow_ttl: Duration,
    github_exchange: Arc<dyn GithubExchange>,
    refresh_store: Arc<RefreshTokenStore>,
    jwt_secret: String,
}

/// Stub GithubExchange that always fails (used when from_env is called without a real client).
struct StubGithubExchange;

impl GithubExchange for StubGithubExchange {
    fn exchange_code(
        &self,
        _code: &str,
        _client_id: &str,
        _client_secret: &str,
        _redirect_uri: &str,
    ) -> Pin<Box<dyn Future<Output = Result<GithubTokenResponse, RelayError>> + Send>> {
        Box::pin(async { Err(RelayError::from_code(ErrorCode::AuthCodeInvalid)) })
    }

    fn get_user_info(
        &self,
        _access_token: &str,
    ) -> Pin<Box<dyn Future<Output = Result<GithubUserInfo, RelayError>> + Send>> {
        Box::pin(async { Err(RelayError::from_code(ErrorCode::InternalError)) })
    }
}

impl OAuthState {
    pub fn from_env() -> Self {
        let github_client_id = env::var("SCRIPTUM_RELAY_GITHUB_CLIENT_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_GITHUB_CLIENT_ID.to_string());

        let github_client_secret = env::var("SCRIPTUM_RELAY_GITHUB_CLIENT_SECRET")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_default();

        let github_authorize_url = env::var("SCRIPTUM_RELAY_GITHUB_AUTHORIZE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_GITHUB_AUTH_URL.to_string());

        let github_token_url = env::var("SCRIPTUM_RELAY_GITHUB_TOKEN_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_GITHUB_TOKEN_URL.to_string());

        let github_scope = env::var("SCRIPTUM_RELAY_GITHUB_SCOPE")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_GITHUB_SCOPE.to_string());

        let jwt_secret = env::var("SCRIPTUM_RELAY_JWT_SECRET")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| {
                "scriptum_local_development_jwt_secret_must_be_32_chars".to_string()
            });

        let flow_ttl_minutes = env::var("SCRIPTUM_RELAY_OAUTH_FLOW_TTL_MINUTES")
            .ok()
            .and_then(|value| value.parse::<i64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_FLOW_TTL_MINUTES);

        let rate_limit_window_secs = env::var("SCRIPTUM_RELAY_OAUTH_START_RATE_LIMIT_WINDOW_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_RATE_LIMIT_WINDOW_SECS);

        let rate_limit_max_requests =
            env::var("SCRIPTUM_RELAY_OAUTH_START_RATE_LIMIT_MAX_REQUESTS")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|value| *value > 0)
                .unwrap_or(DEFAULT_RATE_LIMIT_MAX_REQUESTS);

        Self {
            flow_store: Arc::new(OAuthFlowStore::default()),
            rate_limiter: Arc::new(OAuthStartRateLimiter::new(
                rate_limit_max_requests,
                StdDuration::from_secs(rate_limit_window_secs),
            )),
            github_client_id,
            github_client_secret,
            github_authorize_url,
            github_token_url,
            github_scope,
            flow_ttl: Duration::minutes(flow_ttl_minutes),
            github_exchange: Arc::new(StubGithubExchange),
            refresh_store: Arc::new(RefreshTokenStore::default()),
            jwt_secret,
        }
    }

    #[cfg(test)]
    fn for_tests(
        flow_store: Arc<OAuthFlowStore>,
        max_requests: usize,
        rate_limit_window: StdDuration,
    ) -> Self {
        Self::for_tests_with_exchange(
            flow_store,
            max_requests,
            rate_limit_window,
            Arc::new(StubGithubExchange),
        )
    }

    #[cfg(test)]
    fn for_tests_with_exchange(
        flow_store: Arc<OAuthFlowStore>,
        max_requests: usize,
        rate_limit_window: StdDuration,
        github_exchange: Arc<dyn GithubExchange>,
    ) -> Self {
        Self {
            flow_store,
            rate_limiter: Arc::new(OAuthStartRateLimiter::new(max_requests, rate_limit_window)),
            github_client_id: "test-client-id".to_string(),
            github_client_secret: "test-client-secret".to_string(),
            github_authorize_url: DEFAULT_GITHUB_AUTH_URL.to_string(),
            github_token_url: DEFAULT_GITHUB_TOKEN_URL.to_string(),
            github_scope: DEFAULT_GITHUB_SCOPE.to_string(),
            flow_ttl: Duration::minutes(DEFAULT_FLOW_TTL_MINUTES),
            github_exchange,
            refresh_store: Arc::new(RefreshTokenStore::default()),
            jwt_secret: "scriptum_test_jwt_secret_that_is_definitely_long_enough".to_string(),
        }
    }
}

pub fn router(state: OAuthState) -> Router {
    Router::new()
        .route("/v1/auth/oauth/github/start", post(start_github_oauth))
        .route("/v1/auth/oauth/github/callback", post(callback_github_oauth))
        .with_state(state)
}

#[derive(Debug, Clone)]
pub struct OAuthFlowRecord {
    pub redirect_uri: String,
    pub state: String,
    pub code_challenge: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Default)]
pub struct OAuthFlowStore {
    flows: RwLock<HashMap<Uuid, OAuthFlowRecord>>,
}

impl OAuthFlowStore {
    async fn insert(&self, flow_id: Uuid, record: OAuthFlowRecord) {
        let mut guard = self.flows.write().await;
        prune_expired_flows(&mut guard);
        guard.insert(flow_id, record);
    }

    /// Consume a flow (one-time use). Returns None if expired or not found.
    async fn take(&self, flow_id: Uuid) -> Option<OAuthFlowRecord> {
        let mut guard = self.flows.write().await;
        prune_expired_flows(&mut guard);
        guard.remove(&flow_id)
    }

    #[cfg(test)]
    async fn get(&self, flow_id: Uuid) -> Option<OAuthFlowRecord> {
        let mut guard = self.flows.write().await;
        prune_expired_flows(&mut guard);
        guard.get(&flow_id).cloned()
    }
}

#[derive(Debug)]
struct OAuthStartRateLimiter {
    max_requests: usize,
    window: StdDuration,
    requests: RwLock<VecDeque<Instant>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RateLimitDecision {
    Allowed,
    Limited { retry_after_secs: u64 },
}

impl OAuthStartRateLimiter {
    fn new(max_requests: usize, window: StdDuration) -> Self {
        Self { max_requests, window, requests: RwLock::new(VecDeque::new()) }
    }

    async fn check(&self) -> RateLimitDecision {
        let now = Instant::now();
        let mut guard = self.requests.write().await;
        prune_old_requests(&mut guard, now, self.window);

        if guard.len() >= self.max_requests {
            let retry_after_secs = guard
                .front()
                .map(|oldest| {
                    let elapsed = now.duration_since(*oldest);
                    self.window
                        .checked_sub(elapsed)
                        .unwrap_or_else(|| StdDuration::from_secs(0))
                        .as_secs()
                        .max(1)
                })
                .unwrap_or(1);

            return RateLimitDecision::Limited { retry_after_secs };
        }

        guard.push_back(now);
        RateLimitDecision::Allowed
    }
}

#[derive(Debug, Deserialize)]
struct OAuthGithubStartRequest {
    redirect_uri: String,
    state: String,
    code_challenge: String,
    code_challenge_method: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct OAuthGithubStartResponse {
    flow_id: Uuid,
    authorization_url: String,
    expires_at: DateTime<Utc>,
}

enum OAuthStartError {
    Relay(RelayError),
    RateLimited { retry_after_secs: u64 },
}

impl From<RelayError> for OAuthStartError {
    fn from(value: RelayError) -> Self {
        Self::Relay(value)
    }
}

impl IntoResponse for OAuthStartError {
    fn into_response(self) -> Response {
        match self {
            Self::Relay(error) => error.into_response(),
            Self::RateLimited { retry_after_secs } => {
                let mut response = RelayError::from_code(ErrorCode::RateLimited).into_response();
                if let Ok(value) = HeaderValue::from_str(&retry_after_secs.to_string()) {
                    response.headers_mut().insert(RETRY_AFTER, value);
                }
                response
            }
        }
    }
}

async fn start_github_oauth(
    State(state): State<OAuthState>,
    Json(payload): Json<OAuthGithubStartRequest>,
) -> Result<Json<OAuthGithubStartResponse>, OAuthStartError> {
    match state.rate_limiter.check().await {
        RateLimitDecision::Allowed => {}
        RateLimitDecision::Limited { retry_after_secs } => {
            return Err(OAuthStartError::RateLimited { retry_after_secs });
        }
    }

    validate_redirect_uri(&payload.redirect_uri)?;
    validate_state(&payload.state)?;
    validate_code_challenge_method(&payload.code_challenge_method)?;
    validate_code_challenge(&payload.code_challenge)?;

    let flow_id = Uuid::new_v4();
    let expires_at = Utc::now() + state.flow_ttl;
    state
        .flow_store
        .insert(
            flow_id,
            OAuthFlowRecord {
                redirect_uri: payload.redirect_uri.clone(),
                state: payload.state.clone(),
                code_challenge: payload.code_challenge.clone(),
                expires_at,
            },
        )
        .await;

    let authorization_url = build_authorization_url(
        &state.github_authorize_url,
        &state.github_client_id,
        &state.github_scope,
        &payload,
    )?;

    Ok(Json(OAuthGithubStartResponse { flow_id, authorization_url, expires_at }))
}

fn validate_redirect_uri(redirect_uri: &str) -> Result<(), OAuthStartError> {
    let parsed = Url::parse(redirect_uri).map_err(|_| {
        OAuthStartError::from(RelayError::new(
            ErrorCode::AuthInvalidRedirect,
            "redirect_uri must be a valid absolute URL",
        ))
    })?;

    if parsed.fragment().is_some() {
        return Err(RelayError::new(
            ErrorCode::AuthInvalidRedirect,
            "redirect_uri must not contain a fragment",
        )
        .into());
    }

    let host = parsed.host_str().ok_or_else(|| {
        OAuthStartError::from(RelayError::new(
            ErrorCode::AuthInvalidRedirect,
            "redirect_uri must include a host",
        ))
    })?;

    match parsed.scheme() {
        "https" => Ok(()),
        "http" if is_loopback_host(host) => Ok(()),
        _ => Err(RelayError::new(
            ErrorCode::AuthInvalidRedirect,
            "redirect_uri must use https or localhost http",
        )
        .into()),
    }
}

fn validate_state(state: &str) -> Result<(), OAuthStartError> {
    if state.trim().is_empty() {
        return Err(RelayError::new(ErrorCode::ValidationFailed, "state must not be empty").into());
    }
    if state.len() > MAX_STATE_LEN {
        return Err(RelayError::new(
            ErrorCode::ValidationFailed,
            format!("state must be at most {MAX_STATE_LEN} bytes"),
        )
        .into());
    }
    Ok(())
}

fn validate_code_challenge_method(method: &str) -> Result<(), OAuthStartError> {
    if method != "S256" {
        return Err(RelayError::new(
            ErrorCode::ValidationFailed,
            "code_challenge_method must be S256",
        )
        .into());
    }
    Ok(())
}

fn validate_code_challenge(code_challenge: &str) -> Result<(), OAuthStartError> {
    let len = code_challenge.len();
    if !(MIN_CODE_CHALLENGE_LEN..=MAX_CODE_CHALLENGE_LEN).contains(&len) {
        return Err(RelayError::new(
            ErrorCode::ValidationFailed,
            format!(
                "code_challenge must be between {MIN_CODE_CHALLENGE_LEN} and {MAX_CODE_CHALLENGE_LEN} chars"
            ),
        )
        .into());
    }
    if !code_challenge
        .chars()
        .all(|char| char.is_ascii_alphanumeric() || char == '-' || char == '_')
    {
        return Err(RelayError::new(
            ErrorCode::ValidationFailed,
            "code_challenge must be base64url (A-Z, a-z, 0-9, -, _)",
        )
        .into());
    }
    Ok(())
}

fn build_authorization_url(
    authorize_base: &str,
    client_id: &str,
    scope: &str,
    payload: &OAuthGithubStartRequest,
) -> Result<String, OAuthStartError> {
    let mut url = Url::parse(authorize_base).map_err(|error| {
        tracing::error!(?error, "invalid GitHub authorize URL configuration");
        OAuthStartError::from(RelayError::from_code(ErrorCode::InternalError))
    })?;

    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("client_id", client_id);
        pairs.append_pair("redirect_uri", &payload.redirect_uri);
        pairs.append_pair("state", &payload.state);
        pairs.append_pair("code_challenge", &payload.code_challenge);
        pairs.append_pair("code_challenge_method", "S256");
        pairs.append_pair("response_type", "code");
        pairs.append_pair("scope", scope);
    }

    Ok(url.to_string())
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn prune_expired_flows(flows: &mut HashMap<Uuid, OAuthFlowRecord>) {
    let now = Utc::now();
    flows.retain(|_, record| record.expires_at > now);
}

fn prune_old_requests(requests: &mut VecDeque<Instant>, now: Instant, window: StdDuration) {
    while requests.front().map(|entry| now.duration_since(*entry) >= window).unwrap_or(false) {
        requests.pop_front();
    }
}

// ─── OAuth Callback ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct OAuthGithubCallbackRequest {
    flow_id: Uuid,
    code: String,
    state: String,
    code_verifier: String,
    #[allow(dead_code)]
    device_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct OAuthGithubCallbackResponse {
    access_token: String,
    access_expires_at: DateTime<Utc>,
    refresh_token: String,
    refresh_expires_at: DateTime<Utc>,
    user: OAuthUser,
}

impl IntoResponse for OAuthCallbackError {
    fn into_response(self) -> Response {
        match self {
            Self::Relay(error) => error.into_response(),
        }
    }
}

enum OAuthCallbackError {
    Relay(RelayError),
}

impl From<RelayError> for OAuthCallbackError {
    fn from(value: RelayError) -> Self {
        Self::Relay(value)
    }
}

async fn callback_github_oauth(
    State(state): State<OAuthState>,
    Json(payload): Json<OAuthGithubCallbackRequest>,
) -> Result<Json<OAuthGithubCallbackResponse>, OAuthCallbackError> {
    // 1. Consume the flow (one-time use)
    let flow =
        state.flow_store.take(payload.flow_id).await.ok_or_else(|| {
            RelayError::new(ErrorCode::AuthCodeInvalid, "unknown or expired flow")
        })?;

    // 2. Verify state matches
    if flow.state != payload.state {
        return Err(RelayError::from_code(ErrorCode::AuthStateMismatch).into());
    }

    // 3. Verify PKCE: SHA256(code_verifier) base64url == stored code_challenge
    verify_pkce_s256(&payload.code_verifier, &flow.code_challenge)?;

    // 4. Exchange authorization code with GitHub
    let github_tokens = state
        .github_exchange
        .exchange_code(
            &payload.code,
            &state.github_client_id,
            &state.github_client_secret,
            &flow.redirect_uri,
        )
        .await?;

    // 5. Fetch GitHub user info
    let github_user = state.github_exchange.get_user_info(&github_tokens.access_token).await?;

    // 6. Create or find user (for now, user is constructed from GitHub info)
    let user = OAuthUser {
        id: Uuid::new_v4(),
        email: github_user.email,
        display_name: github_user.display_name,
    };

    // 7. Generate JWT access token (15 min)
    let now = Utc::now();
    let access_expires_at = now + Duration::seconds(DEFAULT_ACCESS_TOKEN_TTL_SECONDS);
    let jwt_service = crate::auth::jwt::JwtAccessTokenService::new(&state.jwt_secret)
        .map_err(|_| RelayError::from_code(ErrorCode::InternalError))?;
    // Use a placeholder workspace ID for auth-level tokens
    let access_token = jwt_service
        .issue_workspace_token(user.id, Uuid::nil())
        .map_err(|_| RelayError::from_code(ErrorCode::InternalError))?;

    // 8. Generate opaque refresh token (30 day, rotating)
    let refresh_expires_at = now + Duration::days(DEFAULT_REFRESH_TOKEN_TTL_DAYS);
    let (refresh_token, refresh_hash) = generate_refresh_token();
    let session_id = Uuid::new_v4();
    let family_id = Uuid::new_v4();
    state
        .refresh_store
        .insert(session_id, user.id, refresh_hash, family_id, refresh_expires_at)
        .await;

    Ok(Json(OAuthGithubCallbackResponse {
        access_token,
        access_expires_at,
        refresh_token,
        refresh_expires_at,
        user,
    }))
}

/// Verify PKCE S256: SHA256(code_verifier) base64url-encoded must equal code_challenge.
fn verify_pkce_s256(code_verifier: &str, code_challenge: &str) -> Result<(), OAuthCallbackError> {
    let hash = Sha256::digest(code_verifier.as_bytes());
    let computed_challenge = URL_SAFE_NO_PAD.encode(hash);

    if computed_challenge != code_challenge {
        return Err(RelayError::new(
            ErrorCode::AuthCodeInvalid,
            "PKCE code_verifier does not match code_challenge",
        )
        .into());
    }
    Ok(())
}

/// Generate a random opaque refresh token and its SHA-256 hash.
fn generate_refresh_token() -> (String, Vec<u8>) {
    let mut bytes = [0u8; REFRESH_TOKEN_BYTES];
    rand::thread_rng().fill_bytes(&mut bytes);
    let token = URL_SAFE_NO_PAD.encode(bytes);
    let hash = Sha256::digest(token.as_bytes()).to_vec();
    (token, hash)
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc, time::Duration as StdDuration};

    use axum::{
        body::{to_bytes, Body},
        http::{Method, Request, StatusCode},
    };
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use url::Url;
    use uuid::Uuid;

    use super::{
        router, GithubExchange, GithubTokenResponse, GithubUserInfo, OAuthFlowRecord,
        OAuthFlowStore, OAuthState,
    };
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use chrono::{Duration, Utc};
    use sha2::{Digest, Sha256};
    use std::future::Future;
    use std::pin::Pin;

    use crate::error::RelayError;

    fn test_router(max_requests: usize) -> (axum::Router, Arc<OAuthFlowStore>) {
        let flow_store = Arc::new(OAuthFlowStore::default());
        let state =
            OAuthState::for_tests(flow_store.clone(), max_requests, StdDuration::from_secs(60));
        (router(state), flow_store)
    }

    fn start_request(payload: Value) -> Request<Body> {
        Request::builder()
            .method(Method::POST)
            .uri("/v1/auth/oauth/github/start")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("request should build")
    }

    #[tokio::test]
    async fn start_returns_flow_id_and_authorization_url() {
        let (app, flow_store) = test_router(10);

        let payload = json!({
            "redirect_uri": "https://app.scriptum.dev/callback",
            "state": "state-123",
            "code_challenge": "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMN1234567",
            "code_challenge_method": "S256"
        });

        let response =
            app.oneshot(start_request(payload)).await.expect("start request should complete");
        assert_eq!(response.status(), StatusCode::OK);

        let body =
            to_bytes(response.into_body(), usize::MAX).await.expect("response body should read");
        let parsed: Value = serde_json::from_slice(&body).expect("response body should be JSON");

        let flow_id =
            Uuid::parse_str(parsed["flow_id"].as_str().expect("flow_id should be present"))
                .expect("flow_id should parse");
        let authorization_url =
            parsed["authorization_url"].as_str().expect("authorization_url should be present");
        let expires_at = parsed["expires_at"].as_str().expect("expires_at should be present");
        assert!(!expires_at.is_empty());

        let auth_url = Url::parse(authorization_url).expect("authorization_url should parse");
        let params: HashMap<String, String> = auth_url.query_pairs().into_owned().collect();
        assert_eq!(params.get("client_id"), Some(&"test-client-id".to_string()));
        assert_eq!(
            params.get("redirect_uri"),
            Some(&"https://app.scriptum.dev/callback".to_string())
        );
        assert_eq!(params.get("state"), Some(&"state-123".to_string()));
        assert_eq!(
            params.get("code_challenge"),
            Some(&"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMN1234567".to_string())
        );
        assert_eq!(params.get("code_challenge_method"), Some(&"S256".to_string()));

        let stored = flow_store.get(flow_id).await.expect("flow should be stored");
        assert_eq!(stored.redirect_uri, "https://app.scriptum.dev/callback");
        assert_eq!(stored.state, "state-123");
    }

    #[tokio::test]
    async fn start_rejects_invalid_redirect_uri() {
        let (app, _) = test_router(10);
        let payload = json!({
            "redirect_uri": "http://example.com/callback",
            "state": "state-123",
            "code_challenge": "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMN1234567",
            "code_challenge_method": "S256"
        });

        let response =
            app.oneshot(start_request(payload)).await.expect("start request should complete");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body =
            to_bytes(response.into_body(), usize::MAX).await.expect("response body should read");
        let parsed: Value = serde_json::from_slice(&body).expect("response body should be JSON");
        assert_eq!(parsed["error"]["code"], "AUTH_INVALID_REDIRECT");
    }

    #[tokio::test]
    async fn start_rejects_non_s256_pkce_method() {
        let (app, _) = test_router(10);
        let payload = json!({
            "redirect_uri": "https://app.scriptum.dev/callback",
            "state": "state-123",
            "code_challenge": "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMN1234567",
            "code_challenge_method": "plain"
        });

        let response =
            app.oneshot(start_request(payload)).await.expect("start request should complete");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body =
            to_bytes(response.into_body(), usize::MAX).await.expect("response body should read");
        let parsed: Value = serde_json::from_slice(&body).expect("response body should be JSON");
        assert_eq!(parsed["error"]["code"], "VALIDATION_FAILED");
    }

    #[tokio::test]
    async fn start_rate_limits_and_sets_retry_after_header() {
        let (app, _) = test_router(1);

        let payload = json!({
            "redirect_uri": "https://app.scriptum.dev/callback",
            "state": "state-123",
            "code_challenge": "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMN1234567",
            "code_challenge_method": "S256"
        });

        let first = app
            .clone()
            .oneshot(start_request(payload.clone()))
            .await
            .expect("first request should complete");
        assert_eq!(first.status(), StatusCode::OK);

        let second =
            app.oneshot(start_request(payload)).await.expect("second request should complete");
        assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(second.headers().get("retry-after").is_some());

        let body =
            to_bytes(second.into_body(), usize::MAX).await.expect("response body should read");
        let parsed: Value = serde_json::from_slice(&body).expect("response body should be JSON");
        assert_eq!(parsed["error"]["code"], "RATE_LIMITED");
    }

    // ─── Callback tests ────────────────────────────────────────────

    struct MockGithubExchange;

    impl GithubExchange for MockGithubExchange {
        fn exchange_code(
            &self,
            _code: &str,
            _client_id: &str,
            _client_secret: &str,
            _redirect_uri: &str,
        ) -> Pin<Box<dyn Future<Output = Result<GithubTokenResponse, RelayError>> + Send>> {
            Box::pin(async {
                Ok(GithubTokenResponse { access_token: "gh-mock-token".to_string() })
            })
        }

        fn get_user_info(
            &self,
            _access_token: &str,
        ) -> Pin<Box<dyn Future<Output = Result<GithubUserInfo, RelayError>> + Send>> {
            Box::pin(async {
                Ok(GithubUserInfo {
                    email: "gary@example.com".to_string(),
                    display_name: "Gary".to_string(),
                })
            })
        }
    }

    fn make_pkce_pair() -> (String, String) {
        let code_verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".to_string();
        let hash = Sha256::digest(code_verifier.as_bytes());
        let code_challenge = URL_SAFE_NO_PAD.encode(hash);
        (code_verifier, code_challenge)
    }

    fn test_callback_router() -> (axum::Router, Arc<OAuthFlowStore>) {
        let flow_store = Arc::new(OAuthFlowStore::default());
        let state = OAuthState::for_tests_with_exchange(
            flow_store.clone(),
            100,
            StdDuration::from_secs(60),
            Arc::new(MockGithubExchange),
        );
        (router(state), flow_store)
    }

    fn callback_request(payload: Value) -> Request<Body> {
        Request::builder()
            .method(Method::POST)
            .uri("/v1/auth/oauth/github/callback")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("request should build")
    }

    async fn seed_flow(flow_store: &OAuthFlowStore, code_challenge: &str) -> Uuid {
        let flow_id = Uuid::new_v4();
        flow_store
            .insert(
                flow_id,
                OAuthFlowRecord {
                    redirect_uri: "https://app.scriptum.dev/callback".to_string(),
                    state: "test-state-abc".to_string(),
                    code_challenge: code_challenge.to_string(),
                    expires_at: Utc::now() + Duration::minutes(10),
                },
            )
            .await;
        flow_id
    }

    #[tokio::test]
    async fn callback_returns_tokens_and_user() {
        let (app, flow_store) = test_callback_router();
        let (code_verifier, code_challenge) = make_pkce_pair();
        let flow_id = seed_flow(&flow_store, &code_challenge).await;

        let payload = json!({
            "flow_id": flow_id,
            "code": "github-auth-code-123",
            "state": "test-state-abc",
            "code_verifier": code_verifier,
            "device_name": "Test Device"
        });

        let response =
            app.oneshot(callback_request(payload)).await.expect("callback should complete");
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.expect("body should read");
        let parsed: Value = serde_json::from_slice(&body).expect("body should be JSON");

        assert!(parsed["access_token"].as_str().is_some_and(|t| !t.is_empty()));
        assert!(parsed["refresh_token"].as_str().is_some_and(|t| !t.is_empty()));
        assert!(parsed["access_expires_at"].as_str().is_some());
        assert!(parsed["refresh_expires_at"].as_str().is_some());
        assert_eq!(parsed["user"]["email"], "gary@example.com");
        assert_eq!(parsed["user"]["display_name"], "Gary");
        assert!(parsed["user"]["id"].as_str().is_some());
    }

    #[tokio::test]
    async fn callback_rejects_unknown_flow() {
        let (app, _) = test_callback_router();
        let (code_verifier, _) = make_pkce_pair();

        let payload = json!({
            "flow_id": Uuid::new_v4(),
            "code": "some-code",
            "state": "test-state-abc",
            "code_verifier": code_verifier,
        });

        let response = app.oneshot(callback_request(payload)).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let parsed: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["error"]["code"], "AUTH_CODE_INVALID");
    }

    #[tokio::test]
    async fn callback_rejects_state_mismatch() {
        let (app, flow_store) = test_callback_router();
        let (code_verifier, code_challenge) = make_pkce_pair();
        let flow_id = seed_flow(&flow_store, &code_challenge).await;

        let payload = json!({
            "flow_id": flow_id,
            "code": "some-code",
            "state": "WRONG-STATE",
            "code_verifier": code_verifier,
        });

        let response = app.oneshot(callback_request(payload)).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let parsed: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["error"]["code"], "AUTH_STATE_MISMATCH");
    }

    #[tokio::test]
    async fn callback_rejects_bad_pkce_verifier() {
        let (app, flow_store) = test_callback_router();
        let (_, code_challenge) = make_pkce_pair();
        let flow_id = seed_flow(&flow_store, &code_challenge).await;

        let payload = json!({
            "flow_id": flow_id,
            "code": "some-code",
            "state": "test-state-abc",
            "code_verifier": "WRONG-VERIFIER-THAT-WONT-HASH-MATCH",
        });

        let response = app.oneshot(callback_request(payload)).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let parsed: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["error"]["code"], "AUTH_CODE_INVALID");
    }

    #[tokio::test]
    async fn callback_flow_is_consumed_single_use() {
        let (app, flow_store) = test_callback_router();
        let (code_verifier, code_challenge) = make_pkce_pair();
        let flow_id = seed_flow(&flow_store, &code_challenge).await;

        let payload = json!({
            "flow_id": flow_id,
            "code": "github-auth-code-123",
            "state": "test-state-abc",
            "code_verifier": code_verifier,
        });

        let first = app.clone().oneshot(callback_request(payload.clone())).await.unwrap();
        assert_eq!(first.status(), StatusCode::OK);

        let second = app.oneshot(callback_request(payload)).await.unwrap();
        assert_eq!(second.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn pkce_verification_works_for_valid_pair() {
        let (verifier, challenge) = make_pkce_pair();
        assert!(super::verify_pkce_s256(&verifier, &challenge).is_ok());
    }

    #[test]
    fn pkce_verification_fails_for_wrong_verifier() {
        let (_, challenge) = make_pkce_pair();
        assert!(super::verify_pkce_s256("wrong-verifier", &challenge).is_err());
    }

    #[test]
    fn generate_refresh_token_produces_unique_tokens() {
        let (token1, hash1) = super::generate_refresh_token();
        let (token2, hash2) = super::generate_refresh_token();
        assert_ne!(token1, token2);
        assert_ne!(hash1, hash2);
        assert!(!token1.is_empty());
        assert_eq!(hash1.len(), 32);
    }
}
