use std::{
    collections::{HashMap, VecDeque},
    env,
    future::Future,
    pin::Pin,
    sync::Arc,
    time::{Duration as StdDuration, Instant},
};

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{Json, State},
    http::{
        header::{AUTHORIZATION, RETRY_AFTER},
        HeaderMap, HeaderValue, StatusCode,
    },
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

use crate::{
    error::{ErrorCode, RelayError},
    validation::ValidatedJson,
};

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
const MIN_PASSWORD_LEN: usize = 8;
const MAX_PASSWORD_LEN: usize = 256;

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

    /// Find a session by the SHA-256 hash of the raw token.
    async fn find_by_hash(&self, token_hash: &[u8]) -> Option<(Uuid, RefreshSession)> {
        let guard = self.sessions.read().await;
        guard
            .iter()
            .find(|(_, session)| session.token_hash == token_hash)
            .map(|(id, session)| (*id, session.clone()))
    }

    /// Revoke a single session. Returns true if the session was found and not already revoked.
    async fn revoke_session(&self, session_id: Uuid) -> bool {
        let mut guard = self.sessions.write().await;
        if let Some(session) = guard.get_mut(&session_id) {
            if session.revoked_at.is_none() {
                session.revoked_at = Some(Utc::now());
                return true;
            }
        }
        false
    }

    /// Revoke all sessions belonging to a token family (reuse detection).
    async fn revoke_family(&self, family_id: Uuid) -> usize {
        let mut guard = self.sessions.write().await;
        let now = Utc::now();
        let mut count = 0;
        for session in guard.values_mut() {
            if session.family_id == family_id && session.revoked_at.is_none() {
                session.revoked_at = Some(now);
                count += 1;
            }
        }
        count
    }
}

#[derive(Debug, Clone)]
struct PasswordUser {
    id: Uuid,
    email: String,
    display_name: String,
    password_hash: String,
}

#[derive(Debug, Default)]
struct PasswordUserStore {
    users_by_email: RwLock<HashMap<String, PasswordUser>>,
}

impl PasswordUserStore {
    async fn create(
        &self,
        email: String,
        display_name: String,
        password_hash: String,
    ) -> Result<PasswordUser, RelayError> {
        let mut guard = self.users_by_email.write().await;
        if guard.contains_key(&email) {
            return Err(RelayError::new(
                ErrorCode::ValidationFailed,
                "an account with this email already exists",
            ));
        }

        let user =
            PasswordUser { id: Uuid::new_v4(), email: email.clone(), display_name, password_hash };
        guard.insert(email, user.clone());
        Ok(user)
    }

    async fn find_by_email(&self, email: &str) -> Option<PasswordUser> {
        self.users_by_email.read().await.get(email).cloned()
    }

    async fn find_by_user_id(&self, user_id: Uuid) -> Option<PasswordUser> {
        self.users_by_email.read().await.values().find(|user| user.id == user_id).cloned()
    }

    async fn update_password_hash(
        &self,
        user_id: Uuid,
        new_password_hash: String,
    ) -> Result<(), RelayError> {
        let mut guard = self.users_by_email.write().await;
        let user = guard.values_mut().find(|user| user.id == user_id).ok_or_else(|| {
            RelayError::new(ErrorCode::AuthInvalidToken, "authenticated user account not found")
        })?;
        user.password_hash = new_password_hash;
        Ok(())
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
    password_store: Arc<PasswordUserStore>,
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
            password_store: Arc::new(PasswordUserStore::default()),
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
            password_store: Arc::new(PasswordUserStore::default()),
            jwt_secret: "scriptum_test_jwt_secret_that_is_definitely_long_enough".to_string(),
        }
    }

    #[cfg(test)]
    fn refresh_store(&self) -> &Arc<RefreshTokenStore> {
        &self.refresh_store
    }
}

pub fn router(state: OAuthState) -> Router {
    Router::new()
        .route("/v1/auth/oauth/github/start", post(start_github_oauth))
        .route("/v1/auth/oauth/github/callback", post(callback_github_oauth))
        .route("/v1/auth/password/register", post(register_password_user))
        .route("/v1/auth/password/login", post(login_password_user))
        .route("/v1/auth/password/change", post(change_password))
        .route("/v1/auth/token/refresh", post(handle_token_refresh))
        .route("/v1/auth/logout", post(handle_logout))
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
    ValidatedJson(payload): ValidatedJson<OAuthGithubStartRequest>,
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

#[derive(Debug, Deserialize)]
struct PasswordRegisterRequest {
    email: String,
    display_name: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct PasswordLoginRequest {
    email: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct PasswordChangeRequest {
    current_password: String,
    new_password: String,
}

#[derive(Debug, Serialize)]
struct AuthSessionResponse {
    access_token: String,
    access_expires_at: DateTime<Utc>,
    refresh_token: String,
    refresh_expires_at: DateTime<Utc>,
    user: OAuthUser,
}

async fn register_password_user(
    State(state): State<OAuthState>,
    ValidatedJson(payload): ValidatedJson<PasswordRegisterRequest>,
) -> Result<Json<AuthSessionResponse>, RelayError> {
    let email = normalize_email(&payload.email)?;
    let display_name = payload.display_name.trim().to_string();
    if display_name.is_empty() {
        return Err(RelayError::new(ErrorCode::ValidationFailed, "display_name must not be empty"));
    }

    validate_password(&payload.password)?;
    let password_hash = hash_password(&payload.password)?;
    let user =
        state.password_store.create(email, display_name, password_hash).await.map(|record| {
            OAuthUser { id: record.id, email: record.email, display_name: record.display_name }
        })?;

    Ok(Json(issue_auth_session(&state, user).await?))
}

async fn login_password_user(
    State(state): State<OAuthState>,
    ValidatedJson(payload): ValidatedJson<PasswordLoginRequest>,
) -> Result<Json<AuthSessionResponse>, RelayError> {
    let email = normalize_email(&payload.email)?;
    let user =
        state.password_store.find_by_email(&email).await.ok_or_else(|| {
            RelayError::new(ErrorCode::AuthInvalidToken, "invalid email or password")
        })?;

    if !verify_password(&payload.password, &user.password_hash) {
        return Err(RelayError::new(ErrorCode::AuthInvalidToken, "invalid email or password"));
    }

    let user = OAuthUser { id: user.id, email: user.email, display_name: user.display_name };
    Ok(Json(issue_auth_session(&state, user).await?))
}

async fn change_password(
    State(state): State<OAuthState>,
    headers: HeaderMap,
    ValidatedJson(payload): ValidatedJson<PasswordChangeRequest>,
) -> Result<StatusCode, RelayError> {
    let user_id = extract_user_id_from_bearer(&state, &headers)?;
    let user = state.password_store.find_by_user_id(user_id).await.ok_or_else(|| {
        RelayError::new(ErrorCode::AuthInvalidToken, "authenticated user account not found")
    })?;

    if !verify_password(&payload.current_password, &user.password_hash) {
        return Err(RelayError::new(ErrorCode::AuthInvalidToken, "current password is invalid"));
    }

    validate_password(&payload.new_password)?;
    if verify_password(&payload.new_password, &user.password_hash) {
        return Err(RelayError::new(
            ErrorCode::ValidationFailed,
            "new password must differ from current password",
        ));
    }

    let new_hash = hash_password(&payload.new_password)?;
    state.password_store.update_password_hash(user_id, new_hash).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn issue_auth_session(
    state: &OAuthState,
    user: OAuthUser,
) -> Result<AuthSessionResponse, RelayError> {
    let now = Utc::now();
    let access_expires_at = now + Duration::seconds(DEFAULT_ACCESS_TOKEN_TTL_SECONDS);
    let jwt_service = crate::auth::jwt::JwtAccessTokenService::new(&state.jwt_secret)
        .map_err(|_| RelayError::from_code(ErrorCode::InternalError))?;
    let access_token = jwt_service
        .issue_workspace_token(user.id, Uuid::nil())
        .map_err(|_| RelayError::from_code(ErrorCode::InternalError))?;

    let refresh_expires_at = now + Duration::days(DEFAULT_REFRESH_TOKEN_TTL_DAYS);
    let (refresh_token, refresh_hash) = generate_refresh_token();
    let session_id = Uuid::new_v4();
    let family_id = Uuid::new_v4();
    state
        .refresh_store
        .insert(session_id, user.id, refresh_hash, family_id, refresh_expires_at)
        .await;

    Ok(AuthSessionResponse {
        access_token,
        access_expires_at,
        refresh_token,
        refresh_expires_at,
        user,
    })
}

fn extract_user_id_from_bearer(
    state: &OAuthState,
    headers: &HeaderMap,
) -> Result<Uuid, RelayError> {
    let token = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(extract_bearer_token)
        .ok_or_else(|| RelayError::new(ErrorCode::AuthInvalidToken, "missing bearer token"))?;

    let jwt_service = crate::auth::jwt::JwtAccessTokenService::new(&state.jwt_secret)
        .map_err(|_| RelayError::from_code(ErrorCode::InternalError))?;
    let access = jwt_service
        .validate_workspace_token(token)
        .map_err(|_| RelayError::new(ErrorCode::AuthInvalidToken, "invalid bearer token"))?;
    Ok(access.user_id)
}

fn extract_bearer_token(value: &str) -> Option<&str> {
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("Bearer") {
        return None;
    }
    let token = token.trim();
    if token.is_empty() {
        return None;
    }
    Some(token)
}

fn normalize_email(raw_email: &str) -> Result<String, RelayError> {
    let email = raw_email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') {
        return Err(RelayError::new(ErrorCode::ValidationFailed, "email must be a valid address"));
    }
    Ok(email)
}

fn validate_password(password: &str) -> Result<(), RelayError> {
    let len = password.chars().count();
    if len < MIN_PASSWORD_LEN || len > MAX_PASSWORD_LEN {
        return Err(RelayError::new(
            ErrorCode::ValidationFailed,
            format!(
                "password must be between {MIN_PASSWORD_LEN} and {MAX_PASSWORD_LEN} characters"
            ),
        ));
    }
    Ok(())
}

fn hash_password(password: &str) -> Result<String, RelayError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| RelayError::from_code(ErrorCode::InternalError))
}

fn verify_password(password: &str, password_hash: &str) -> bool {
    let parsed_hash = match PasswordHash::new(password_hash) {
        Ok(parsed) => parsed,
        Err(_) => return false,
    };
    Argon2::default().verify_password(password.as_bytes(), &parsed_hash).is_ok()
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
    ValidatedJson(payload): ValidatedJson<OAuthGithubCallbackRequest>,
) -> Result<Json<AuthSessionResponse>, OAuthCallbackError> {
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

    Ok(Json(issue_auth_session(&state, user).await?))
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

// ─── Token Refresh ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct RefreshTokenRequest {
    refresh_token: String,
}

#[derive(Debug, Serialize)]
struct RefreshTokenResponse {
    access_token: String,
    access_expires_at: DateTime<Utc>,
    refresh_token: String,
    refresh_expires_at: DateTime<Utc>,
}

async fn handle_token_refresh(
    State(state): State<OAuthState>,
    ValidatedJson(payload): ValidatedJson<RefreshTokenRequest>,
) -> Result<Json<RefreshTokenResponse>, RelayError> {
    let token_hash = Sha256::digest(payload.refresh_token.as_bytes()).to_vec();

    let (session_id, session) = state
        .refresh_store
        .find_by_hash(&token_hash)
        .await
        .ok_or_else(|| RelayError::from_code(ErrorCode::AuthInvalidToken))?;

    // Reuse detection: if the token was already consumed, revoke the entire family.
    if session.revoked_at.is_some() {
        state.refresh_store.revoke_family(session.family_id).await;
        return Err(RelayError::new(
            ErrorCode::AuthTokenRevoked,
            "refresh token reuse detected; token family revoked",
        ));
    }

    // Reject expired tokens.
    if session.expires_at < Utc::now() {
        return Err(RelayError::new(ErrorCode::AuthInvalidToken, "refresh token has expired"));
    }

    // Revoke old session (single-use rotation).
    state.refresh_store.revoke_session(session_id).await;

    // Issue new refresh token in the same family.
    let (new_refresh_token, new_refresh_hash) = generate_refresh_token();
    let now = Utc::now();
    let refresh_expires_at = now + Duration::days(DEFAULT_REFRESH_TOKEN_TTL_DAYS);
    let new_session_id = Uuid::new_v4();
    state
        .refresh_store
        .insert(
            new_session_id,
            session.user_id,
            new_refresh_hash,
            session.family_id,
            refresh_expires_at,
        )
        .await;

    // Issue new JWT access token.
    let access_expires_at = now + Duration::seconds(DEFAULT_ACCESS_TOKEN_TTL_SECONDS);
    let jwt_service = crate::auth::jwt::JwtAccessTokenService::new(&state.jwt_secret)
        .map_err(|_| RelayError::from_code(ErrorCode::InternalError))?;
    let access_token = jwt_service
        .issue_workspace_token(session.user_id, Uuid::nil())
        .map_err(|_| RelayError::from_code(ErrorCode::InternalError))?;

    Ok(Json(RefreshTokenResponse {
        access_token,
        access_expires_at,
        refresh_token: new_refresh_token,
        refresh_expires_at,
    }))
}

// ─── Logout ────────────────────────────────────────────────────────────

async fn handle_logout(
    State(state): State<OAuthState>,
    headers: HeaderMap,
    ValidatedJson(payload): ValidatedJson<RefreshTokenRequest>,
) -> Result<StatusCode, RelayError> {
    extract_user_id_from_bearer(&state, &headers)?;

    let token_hash = Sha256::digest(payload.refresh_token.as_bytes()).to_vec();

    let (session_id, _) = state
        .refresh_store
        .find_by_hash(&token_hash)
        .await
        .ok_or_else(|| RelayError::from_code(ErrorCode::AuthInvalidToken))?;

    // Idempotent: succeeds even if already revoked.
    state.refresh_store.revoke_session(session_id).await;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc, time::Duration as StdDuration};

    use axum::{
        body::{to_bytes, Body},
        http::{header::AUTHORIZATION, Method, Request, StatusCode},
    };
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use url::Url;
    use uuid::Uuid;

    use super::{
        router, GithubExchange, GithubTokenResponse, GithubUserInfo, OAuthFlowRecord,
        OAuthFlowStore, OAuthState, RefreshTokenStore,
    };
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use chrono::{Duration, Utc};
    use sha2::{Digest, Sha256};
    use std::future::Future;
    use std::pin::Pin;

    use crate::{auth::jwt::JwtAccessTokenService, error::RelayError};

    const TEST_JWT_SECRET: &str = "scriptum_test_jwt_secret_that_is_definitely_long_enough";

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

    fn password_register_request(payload: Value) -> Request<Body> {
        Request::builder()
            .method(Method::POST)
            .uri("/v1/auth/password/register")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("request should build")
    }

    fn password_login_request(payload: Value) -> Request<Body> {
        Request::builder()
            .method(Method::POST)
            .uri("/v1/auth/password/login")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("request should build")
    }

    fn password_change_request(payload: Value, token: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .method(Method::POST)
            .uri("/v1/auth/password/change")
            .header("content-type", "application/json");
        if let Some(token) = token {
            builder = builder.header(AUTHORIZATION, format!("Bearer {token}"));
        }
        builder.body(Body::from(payload.to_string())).expect("request should build")
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

    #[tokio::test]
    async fn password_register_and_login_issue_session_tokens() {
        let (app, _) = test_router(10);
        let register = app
            .clone()
            .oneshot(password_register_request(json!({
                "email": "TeSt+auth@example.com",
                "display_name": "Password User",
                "password": "Sup3rSecurePass!"
            })))
            .await
            .expect("register request should return");
        assert_eq!(register.status(), StatusCode::OK);

        let body = to_bytes(register.into_body(), usize::MAX).await.expect("body should read");
        let parsed: Value = serde_json::from_slice(&body).expect("body should be valid json");
        assert!(parsed["access_token"].as_str().is_some_and(|value| !value.is_empty()));
        assert!(parsed["refresh_token"].as_str().is_some_and(|value| !value.is_empty()));
        assert_eq!(parsed["user"]["email"], "test+auth@example.com");

        let wrong_login = app
            .clone()
            .oneshot(password_login_request(json!({
                "email": "test+auth@example.com",
                "password": "wrong-password"
            })))
            .await
            .expect("login request should return");
        assert_eq!(wrong_login.status(), StatusCode::UNAUTHORIZED);

        let login = app
            .oneshot(password_login_request(json!({
                "email": "test+auth@example.com",
                "password": "Sup3rSecurePass!"
            })))
            .await
            .expect("login request should return");
        assert_eq!(login.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn password_register_rejects_duplicate_email() {
        let (app, _) = test_router(10);
        let payload = json!({
            "email": "duplicate@example.com",
            "display_name": "Dupe",
            "password": "Sup3rSecurePass!"
        });

        let first = app
            .clone()
            .oneshot(password_register_request(payload.clone()))
            .await
            .expect("first register should return");
        assert_eq!(first.status(), StatusCode::OK);

        let second = app
            .oneshot(password_register_request(payload))
            .await
            .expect("second register should return");
        assert_eq!(second.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn password_change_requires_bearer_token() {
        let (app, _) = test_router(10);
        let response = app
            .oneshot(password_change_request(
                json!({
                    "current_password": "Sup3rSecurePass!",
                    "new_password": "N3werSecurePass!"
                }),
                None,
            ))
            .await
            .expect("change-password request should return");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn password_change_updates_credentials() {
        let (app, _) = test_router(10);
        let register = app
            .clone()
            .oneshot(password_register_request(json!({
                "email": "changeme@example.com",
                "display_name": "Change Me",
                "password": "Sup3rSecurePass!"
            })))
            .await
            .expect("register request should return");
        assert_eq!(register.status(), StatusCode::OK);
        let register_body =
            to_bytes(register.into_body(), usize::MAX).await.expect("register body should read");
        let register_json: Value =
            serde_json::from_slice(&register_body).expect("register body should be json");
        let access_token = register_json["access_token"]
            .as_str()
            .expect("access token should be present")
            .to_string();

        let change = app
            .clone()
            .oneshot(password_change_request(
                json!({
                    "current_password": "Sup3rSecurePass!",
                    "new_password": "N3werSecurePass!"
                }),
                Some(&access_token),
            ))
            .await
            .expect("change-password request should return");
        assert_eq!(change.status(), StatusCode::NO_CONTENT);

        let old_login = app
            .clone()
            .oneshot(password_login_request(json!({
                "email": "changeme@example.com",
                "password": "Sup3rSecurePass!"
            })))
            .await
            .expect("old-password login should return");
        assert_eq!(old_login.status(), StatusCode::UNAUTHORIZED);

        let new_login = app
            .oneshot(password_login_request(json!({
                "email": "changeme@example.com",
                "password": "N3werSecurePass!"
            })))
            .await
            .expect("new-password login should return");
        assert_eq!(new_login.status(), StatusCode::OK);
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

    // ─── Refresh token rotation tests ─────────────────────────────────

    fn test_refresh_router_with_store() -> (axum::Router, Arc<RefreshTokenStore>) {
        let flow_store = Arc::new(OAuthFlowStore::default());
        let state = OAuthState::for_tests_with_exchange(
            flow_store,
            100,
            StdDuration::from_secs(60),
            Arc::new(MockGithubExchange),
        );
        let store = state.refresh_store().clone();
        (router(state), store)
    }

    fn refresh_request(payload: Value) -> Request<Body> {
        Request::builder()
            .method(Method::POST)
            .uri("/v1/auth/token/refresh")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("request should build")
    }

    fn logout_request(payload: Value, access_token: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .method(Method::POST)
            .uri("/v1/auth/logout")
            .header("content-type", "application/json");
        if let Some(token) = access_token {
            builder = builder.header(AUTHORIZATION, format!("Bearer {token}"));
        }
        builder.body(Body::from(payload.to_string())).expect("request should build")
    }

    async fn seed_refresh(
        store: &RefreshTokenStore,
        user_id: Uuid,
        family_id: Uuid,
        ttl: Duration,
    ) -> String {
        let (raw_token, token_hash) = super::generate_refresh_token();
        let session_id = Uuid::new_v4();
        let expires_at = Utc::now() + ttl;
        store.insert(session_id, user_id, token_hash, family_id, expires_at).await;
        raw_token
    }

    fn issue_test_access_token(user_id: Uuid) -> String {
        JwtAccessTokenService::new(TEST_JWT_SECRET)
            .expect("jwt service should initialize")
            .issue_workspace_token(user_id, Uuid::nil())
            .expect("token should be issued")
    }

    #[tokio::test]
    async fn refresh_rotates_token_and_returns_new_pair() {
        let (app, store) = test_refresh_router_with_store();
        let user_id = Uuid::new_v4();
        let family_id = Uuid::new_v4();
        let raw_token = seed_refresh(&store, user_id, family_id, Duration::days(30)).await;

        let response =
            app.oneshot(refresh_request(json!({ "refresh_token": raw_token }))).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let parsed: Value = serde_json::from_slice(&body).unwrap();

        assert!(parsed["access_token"].as_str().is_some_and(|t| !t.is_empty()));
        let new_refresh =
            parsed["refresh_token"].as_str().expect("refresh_token should be present");
        assert!(!new_refresh.is_empty());
        assert_ne!(new_refresh, raw_token, "rotated token should differ from old");
        assert!(parsed["access_expires_at"].as_str().is_some());
        assert!(parsed["refresh_expires_at"].as_str().is_some());
    }

    #[tokio::test]
    async fn refresh_reuse_detection_revokes_family() {
        let (app, store) = test_refresh_router_with_store();
        let user_id = Uuid::new_v4();
        let family_id = Uuid::new_v4();
        let token_a = seed_refresh(&store, user_id, family_id, Duration::days(30)).await;
        let token_b = seed_refresh(&store, user_id, family_id, Duration::days(30)).await;

        // First use of token_a succeeds (rotates it).
        let first = app
            .clone()
            .oneshot(refresh_request(json!({ "refresh_token": token_a })))
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::OK);

        // Second use of (now-revoked) token_a triggers reuse detection.
        let second = app
            .clone()
            .oneshot(refresh_request(json!({ "refresh_token": token_a })))
            .await
            .unwrap();
        assert_eq!(second.status(), StatusCode::UNAUTHORIZED);

        let body = to_bytes(second.into_body(), usize::MAX).await.unwrap();
        let parsed: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["error"]["code"], "AUTH_TOKEN_REVOKED");

        // token_b (same family) should also be revoked now.
        let third =
            app.oneshot(refresh_request(json!({ "refresh_token": token_b }))).await.unwrap();
        assert_eq!(third.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn refresh_rejects_expired_token() {
        let (app, store) = test_refresh_router_with_store();
        let user_id = Uuid::new_v4();
        let family_id = Uuid::new_v4();
        // Seed with negative TTL so it's already expired.
        let raw_token = seed_refresh(&store, user_id, family_id, Duration::seconds(-1)).await;

        let response =
            app.oneshot(refresh_request(json!({ "refresh_token": raw_token }))).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let parsed: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["error"]["code"], "AUTH_INVALID_TOKEN");
    }

    #[tokio::test]
    async fn refresh_rejects_unknown_token() {
        let (app, _) = test_refresh_router_with_store();

        let response = app
            .oneshot(refresh_request(json!({ "refresh_token": "totally-bogus-token" })))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let parsed: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["error"]["code"], "AUTH_INVALID_TOKEN");
    }

    #[tokio::test]
    async fn logout_revokes_session() {
        let (app, store) = test_refresh_router_with_store();
        let user_id = Uuid::new_v4();
        let family_id = Uuid::new_v4();
        let raw_token = seed_refresh(&store, user_id, family_id, Duration::days(30)).await;
        let access_token = issue_test_access_token(user_id);

        let response = app
            .clone()
            .oneshot(logout_request(json!({ "refresh_token": raw_token }), Some(&access_token)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // Token should no longer work for refresh (revoked → reuse detection).
        let refresh_attempt =
            app.oneshot(refresh_request(json!({ "refresh_token": raw_token }))).await.unwrap();
        assert_eq!(refresh_attempt.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn logout_rejects_unknown_token() {
        let (app, _) = test_refresh_router_with_store();
        let access_token = issue_test_access_token(Uuid::new_v4());

        let response = app
            .oneshot(logout_request(
                json!({ "refresh_token": "unknown-token" }),
                Some(&access_token),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn logout_requires_bearer_token() {
        let (app, store) = test_refresh_router_with_store();
        let raw_token =
            seed_refresh(&store, Uuid::new_v4(), Uuid::new_v4(), Duration::days(30)).await;

        let response =
            app.oneshot(logout_request(json!({ "refresh_token": raw_token }), None)).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
