pub mod auth;
pub mod comments;
pub mod documents;
pub mod members;
pub mod search;
pub mod workspaces;

use std::{
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
    time::Instant,
};

use anyhow::{Context, Result};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, SaltString},
    Argon2, PasswordVerifier,
};
use axum::{
    extract::{Extension, FromRequestParts, Json, Path, Request, State},
    http::{header::IF_MATCH, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post},
    Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use scriptum_common::types::Workspace;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{
    types::chrono::{DateTime, Utc},
    PgPool,
};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{
    auth::{
        jwt::JwtAccessTokenService,
        middleware::{require_bearer_auth, AuthenticatedUser, WorkspaceRole},
        oauth::OAuthState,
    },
    db::pool::{check_pool_health, create_pg_pool, PoolConfig},
    error::{ErrorCode, RelayError},
    idempotency::{self, IdempotencyDbState},
    validation::ValidatedJson,
};

const DEFAULT_PAGE_SIZE: usize = 50;
const MAX_PAGE_SIZE: usize = 100;
const DEFAULT_INVITE_EXPIRY_HOURS: u32 = 72;
const MAX_INVITE_EXPIRY_HOURS: u32 = 720; // 30 days
const REDEEM_RATE_LIMIT_MAX: u32 = 5;
const REDEEM_RATE_LIMIT_WINDOW_SECS: u64 = 900; // 15 minutes

struct RedeemRateEntry {
    count: u32,
    window_start: Instant,
}

#[derive(Clone)]
struct ApiState {
    store: WorkspaceStore,
    redeem_limiter: Arc<Mutex<HashMap<Vec<u8>, RedeemRateEntry>>>,
}

#[derive(Clone)]
enum WorkspaceStore {
    Postgres(PgPool),
    #[cfg_attr(not(test), allow(dead_code))]
    Memory(Arc<RwLock<MemoryWorkspaceStore>>),
}

#[derive(Clone)]
struct WorkspaceRecord {
    id: Uuid,
    slug: String,
    name: String,
    role: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl WorkspaceRecord {
    fn etag(&self) -> String {
        workspace_etag(self.id, self.updated_at)
    }

    fn into_workspace(self) -> Workspace {
        let etag = self.etag();
        Workspace {
            id: self.id,
            slug: self.slug,
            name: self.name,
            role: Some(self.role),
            created_at: self.created_at,
            updated_at: self.updated_at,
            etag,
        }
    }
}

#[derive(Default)]
struct MemoryWorkspaceStore {
    workspaces: HashMap<Uuid, MemoryWorkspace>,
    memberships: HashMap<(Uuid, Uuid), MemoryMembership>,
    share_links: HashMap<Uuid, MemoryShareLink>,
    invites: HashMap<Uuid, MemoryInvite>,
}

#[derive(Clone)]
struct MemoryWorkspace {
    id: Uuid,
    slug: String,
    name: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
}

#[derive(Clone)]
struct MemoryMembership {
    role: String,
    status: String,
    email: String,
    display_name: String,
    joined_at: DateTime<Utc>,
}

#[derive(Clone)]
struct MemoryShareLink {
    id: Uuid,
    workspace_id: Uuid,
    target_type: String,
    target_id: Uuid,
    permission: String,
    token_hash: Vec<u8>,
    password_hash: Option<String>,
    expires_at: Option<DateTime<Utc>>,
    max_uses: Option<i32>,
    use_count: i32,
    disabled: bool,
    created_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize, Serialize)]
struct CreateWorkspaceRequest {
    name: String,
    slug: String,
}

#[derive(Deserialize)]
struct UpdateWorkspaceRequest {
    name: Option<String>,
    slug: Option<String>,
}

#[derive(Deserialize, Clone)]
struct CreateShareLinkRequest {
    target_type: String,
    target_id: Uuid,
    permission: String,
    expires_at: Option<DateTime<Utc>>,
    max_uses: Option<i32>,
    password: Option<String>,
}

#[derive(Deserialize, Clone)]
struct UpdateShareLinkRequest {
    permission: Option<String>,
    #[serde(default)]
    expires_at: Option<Option<DateTime<Utc>>>,
    #[serde(default)]
    max_uses: Option<Option<i32>>,
    disabled: Option<bool>,
}

#[derive(Deserialize)]
struct RedeemShareLinkRequest {
    token: String,
    password: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct RedeemShareLinkResponse {
    workspace_id: Uuid,
    target_type: String,
    target_id: Uuid,
    permission: String,
    remaining_uses: Option<i32>,
}

#[derive(Deserialize)]
struct ListWorkspacesQuery {
    limit: Option<usize>,
    cursor: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct WorkspaceEnvelope {
    workspace: Workspace,
}

#[derive(Serialize, Deserialize)]
struct WorkspacesPageEnvelope {
    items: Vec<Workspace>,
    next_cursor: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct ShareLinkEnvelope {
    share_link: ShareLink,
}

#[derive(Serialize, Deserialize)]
struct ShareLinksEnvelope {
    items: Vec<ShareLink>,
}

#[derive(Clone, Serialize, Deserialize)]
struct ShareLink {
    id: Uuid,
    target_type: String,
    target_id: Uuid,
    permission: String,
    expires_at: Option<DateTime<Utc>>,
    max_uses: Option<i32>,
    use_count: i32,
    disabled: bool,
    created_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    etag: String,
    url_once: String,
}

#[derive(Serialize, Deserialize)]
struct MemberEnvelope {
    member: Member,
}

#[derive(Serialize, Deserialize)]
struct MembersPageEnvelope {
    items: Vec<Member>,
    next_cursor: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Member {
    user_id: Uuid,
    email: String,
    display_name: String,
    role: String,
    status: String,
    joined_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct UpdateMemberRequest {
    role: Option<String>,
    status: Option<String>,
}

#[derive(Deserialize)]
struct ListMembersQuery {
    limit: Option<usize>,
    cursor: Option<String>,
}

#[derive(Clone)]
struct MemberRecord {
    user_id: Uuid,
    email: String,
    display_name: String,
    role: String,
    status: String,
    joined_at: DateTime<Utc>,
}

impl MemberRecord {
    fn etag(&self) -> String {
        member_etag(self.user_id, self.joined_at)
    }

    fn into_member(self) -> Member {
        Member {
            user_id: self.user_id,
            email: self.email,
            display_name: self.display_name,
            role: self.role,
            status: self.status,
            joined_at: self.joined_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct MemberRow {
    user_id: Uuid,
    email: String,
    display_name: String,
    role: String,
    status: String,
    joined_at: DateTime<Utc>,
}

impl From<MemberRow> for MemberRecord {
    fn from(value: MemberRow) -> Self {
        Self {
            user_id: value.user_id,
            email: value.email,
            display_name: value.display_name,
            role: value.role,
            status: value.status,
            joined_at: value.joined_at,
        }
    }
}

struct MemberPage {
    items: Vec<MemberRecord>,
    next_cursor: Option<String>,
}

#[derive(Clone)]
struct MemberCursor {
    joined_at: DateTime<Utc>,
    user_id: Uuid,
}

#[derive(Deserialize)]
struct CreateInviteRequest {
    email: String,
    role: String,
    expires_in_hours: Option<u32>,
}

#[derive(Serialize, Deserialize)]
struct InviteEnvelope {
    invite: Invite,
}

#[derive(Clone, Serialize, Deserialize)]
struct Invite {
    id: Uuid,
    workspace_id: Uuid,
    email: String,
    role: String,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
}

#[derive(Clone)]
struct InviteRecord {
    id: Uuid,
    workspace_id: Uuid,
    email: String,
    role: String,
    token_hash: Vec<u8>,
    expires_at: DateTime<Utc>,
    accepted_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl InviteRecord {
    fn into_invite(self) -> Invite {
        Invite {
            id: self.id,
            workspace_id: self.workspace_id,
            email: self.email,
            role: self.role,
            expires_at: self.expires_at,
            created_at: self.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct InviteRow {
    id: Uuid,
    workspace_id: Uuid,
    email: String,
    role: String,
    token_hash: Vec<u8>,
    expires_at: DateTime<Utc>,
    accepted_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl From<InviteRow> for InviteRecord {
    fn from(value: InviteRow) -> Self {
        Self {
            id: value.id,
            workspace_id: value.workspace_id,
            email: value.email,
            role: value.role,
            token_hash: value.token_hash,
            expires_at: value.expires_at,
            accepted_at: value.accepted_at,
            created_at: value.created_at,
        }
    }
}

#[derive(Clone)]
struct MemoryInvite {
    id: Uuid,
    workspace_id: Uuid,
    email: String,
    role: String,
    token_hash: Vec<u8>,
    expires_at: DateTime<Utc>,
    accepted_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

#[derive(Clone)]
struct WorkspaceCursor {
    created_at: DateTime<Utc>,
    id: Uuid,
}

struct WorkspacePage {
    items: Vec<WorkspaceRecord>,
    next_cursor: Option<String>,
}

#[derive(Clone)]
struct ShareLinkRecord {
    id: Uuid,
    target_type: String,
    target_id: Uuid,
    permission: String,
    expires_at: Option<DateTime<Utc>>,
    max_uses: Option<i32>,
    use_count: i32,
    disabled: bool,
    created_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

impl ShareLinkRecord {
    fn etag(&self) -> String {
        share_link_etag(self)
    }

    fn into_share_link(self, url_once: String) -> ShareLink {
        let etag = self.etag();
        ShareLink {
            id: self.id,
            target_type: self.target_type,
            target_id: self.target_id,
            permission: self.permission,
            expires_at: self.expires_at,
            max_uses: self.max_uses,
            use_count: self.use_count,
            disabled: self.disabled,
            created_at: self.created_at,
            revoked_at: self.revoked_at,
            etag,
            url_once,
        }
    }
}

#[derive(sqlx::FromRow)]
struct WorkspaceRow {
    id: Uuid,
    slug: String,
    name: String,
    role: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct ShareLinkRow {
    id: Uuid,
    target_type: String,
    target_id: Uuid,
    permission: String,
    expires_at: Option<DateTime<Utc>>,
    max_uses: Option<i32>,
    use_count: i32,
    disabled: bool,
    created_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

impl From<ShareLinkRow> for ShareLinkRecord {
    fn from(value: ShareLinkRow) -> Self {
        Self {
            id: value.id,
            target_type: value.target_type,
            target_id: value.target_id,
            permission: value.permission,
            expires_at: value.expires_at,
            max_uses: value.max_uses,
            use_count: value.use_count,
            disabled: value.disabled,
            created_at: value.created_at,
            revoked_at: value.revoked_at,
        }
    }
}

#[derive(Clone)]
struct ShareLinkLookup {
    id: Uuid,
    workspace_id: Uuid,
    target_type: String,
    target_id: Uuid,
    permission: String,
    password_hash: Option<String>,
    expires_at: Option<DateTime<Utc>>,
    max_uses: Option<i32>,
    use_count: i32,
    disabled: bool,
    revoked_at: Option<DateTime<Utc>>,
}

#[derive(sqlx::FromRow)]
struct ShareLinkLookupRow {
    id: Uuid,
    workspace_id: Uuid,
    target_type: String,
    target_id: Uuid,
    permission: String,
    password_hash: Option<String>,
    expires_at: Option<DateTime<Utc>>,
    max_uses: Option<i32>,
    use_count: i32,
    disabled: bool,
    revoked_at: Option<DateTime<Utc>>,
}

impl From<ShareLinkLookupRow> for ShareLinkLookup {
    fn from(value: ShareLinkLookupRow) -> Self {
        Self {
            id: value.id,
            workspace_id: value.workspace_id,
            target_type: value.target_type,
            target_id: value.target_id,
            permission: value.permission,
            password_hash: value.password_hash,
            expires_at: value.expires_at,
            max_uses: value.max_uses,
            use_count: value.use_count,
            disabled: value.disabled,
            revoked_at: value.revoked_at,
        }
    }
}

impl From<WorkspaceRow> for WorkspaceRecord {
    fn from(value: WorkspaceRow) -> Self {
        Self {
            id: value.id,
            slug: value.slug,
            name: value.name,
            role: value.role,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Debug)]
enum ApiError {
    BadRequest { message: String },
    Forbidden { message: &'static str },
    NotFound { message: &'static str },
    Conflict { message: &'static str },
    PreconditionRequired,
    PreconditionFailed,
    RateLimited { message: &'static str },
    Internal(anyhow::Error),
}

impl ApiError {
    fn bad_request(_code: &'static str, message: impl Into<String>) -> Self {
        Self::BadRequest { message: message.into() }
    }

    fn forbidden(_code: &'static str, message: &'static str) -> Self {
        Self::Forbidden { message }
    }

    fn not_found(_code: &'static str, message: &'static str) -> Self {
        Self::NotFound { message }
    }

    fn conflict(_code: &'static str, message: &'static str) -> Self {
        Self::Conflict { message }
    }

    fn rate_limited(_code: &'static str, message: &'static str) -> Self {
        Self::RateLimited { message }
    }

    fn internal(error: anyhow::Error) -> Self {
        Self::Internal(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            Self::BadRequest { message } => {
                RelayError::new(ErrorCode::ValidationFailed, message).into_response()
            }
            Self::Forbidden { message } => {
                RelayError::new(ErrorCode::AuthForbidden, message).into_response()
            }
            Self::NotFound { message } => {
                RelayError::new(ErrorCode::NotFound, message).into_response()
            }
            Self::Conflict { message } => {
                RelayError::new(ErrorCode::DocPathConflict, message).into_response()
            }
            Self::PreconditionRequired => {
                RelayError::new(ErrorCode::PreconditionRequired, "missing If-Match header")
                    .into_response()
            }
            Self::PreconditionFailed => RelayError::new(
                ErrorCode::EditPreconditionFailed,
                "If-Match does not match current workspace state",
            )
            .into_response(),
            Self::RateLimited { message } => {
                RelayError::new(ErrorCode::RateLimited, message).into_response()
            }
            Self::Internal(error) => {
                tracing::error!(error = ?error, "workspace api internal error");
                RelayError::from_code(ErrorCode::InternalError).into_response()
            }
        }
    }
}

pub async fn build_router_from_env(
    jwt_service: Arc<JwtAccessTokenService>,
    oauth_state: OAuthState,
) -> Result<Router> {
    let database_url = env::var("SCRIPTUM_RELAY_DATABASE_URL")
        .context("SCRIPTUM_RELAY_DATABASE_URL must be set for workspace API")?;

    let pool = create_pg_pool(&database_url, PoolConfig::from_env())
        .await
        .context("failed to initialize relay PostgreSQL pool for workspace API")?;
    check_pool_health(&pool)
        .await
        .context("relay PostgreSQL health check failed for workspace API")?;
    let idempotency_state = IdempotencyDbState::new(pool.clone());

    Ok(build_router_with_store(WorkspaceStore::Postgres(pool.clone()), Arc::clone(&jwt_service))
        .merge(auth::router(oauth_state))
        .merge(documents::router(pool.clone(), Arc::clone(&jwt_service)))
        .merge(comments::router(pool.clone(), Arc::clone(&jwt_service)))
        .merge(search::router(pool, jwt_service))
        .layer(middleware::from_fn_with_state(
            idempotency_state,
            idempotency::idempotency_db_middleware,
        )))
}

fn build_router_with_store(
    store: WorkspaceStore,
    jwt_service: Arc<JwtAccessTokenService>,
) -> Router {
    let state = ApiState { store, redeem_limiter: Arc::new(Mutex::new(HashMap::new())) };
    let viewer_role_layer =
        middleware::from_fn_with_state(state.clone(), require_workspace_viewer_role);
    let editor_role_layer =
        middleware::from_fn_with_state(state.clone(), require_workspace_editor_role);
    let owner_role_layer =
        middleware::from_fn_with_state(state.clone(), require_workspace_owner_role);
    let members_viewer_layer =
        middleware::from_fn_with_state(state.clone(), require_workspace_viewer_role);
    let members_owner_layer =
        middleware::from_fn_with_state(state.clone(), require_workspace_owner_role);
    let members_delete_owner_layer =
        middleware::from_fn_with_state(state.clone(), require_workspace_owner_role);

    Router::new()
        .route(
            "/v1/workspaces",
            post(workspaces::create_workspace).get(workspaces::list_workspaces),
        )
        .route("/v1/workspaces/{id}", get(workspaces::get_workspace).route_layer(viewer_role_layer))
        .route(
            "/v1/workspaces/{id}",
            patch(workspaces::update_workspace).route_layer(owner_role_layer),
        )
        .route(
            "/v1/workspaces/{id}/share-links",
            post(create_share_link).get(list_share_links).route_layer(editor_role_layer.clone()),
        )
        .route(
            "/v1/workspaces/{id}/share-links/{share_link_id}",
            patch(update_share_link).delete(revoke_share_link).route_layer(editor_role_layer),
        )
        .route(
            "/v1/workspaces/{workspace_id}/members",
            get(members::list_members).route_layer(members_viewer_layer),
        )
        .route(
            "/v1/workspaces/{workspace_id}/members/{member_id}",
            patch(members::update_member).route_layer(members_owner_layer),
        )
        .route(
            "/v1/workspaces/{workspace_id}/members/{member_id}",
            delete(members::delete_member).route_layer(members_delete_owner_layer),
        )
        .route(
            "/v1/workspaces/{workspace_id}/invites",
            post(members::create_invite).route_layer(middleware::from_fn_with_state(
                state.clone(),
                require_workspace_owner_role,
            )),
        )
        .route("/v1/invites/{token}/accept", post(members::accept_invite))
        .with_state(state.clone())
        .route_layer(middleware::from_fn_with_state(jwt_service, require_bearer_auth))
        .merge(
            Router::new()
                .route("/v1/share-links/redeem", post(redeem_share_link))
                .with_state(state),
        )
}

async fn create_share_link(
    State(state): State<ApiState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(workspace_id): Path<Uuid>,
    ValidatedJson(mut payload): ValidatedJson<CreateShareLinkRequest>,
) -> Result<(StatusCode, Json<ShareLinkEnvelope>), ApiError> {
    validate_share_link_request(workspace_id, &payload)?;

    if payload.password.as_deref().is_some_and(|value| value.trim().is_empty()) {
        payload.password = None;
    }

    let token = generate_share_link_token();
    let token_hash = hash_share_link_token(&token);
    let password_hash = payload.password.as_deref().map(hash_share_link_password).transpose()?;
    let share_link = state
        .store
        .create_share_link(workspace_id, user.user_id, payload, token_hash, password_hash)
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(ShareLinkEnvelope {
            share_link: share_link.into_share_link(build_share_link_url(&token)),
        }),
    ))
}

async fn list_share_links(
    State(state): State<ApiState>,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<ShareLinksEnvelope>, ApiError> {
    let items = state
        .store
        .list_share_links(workspace_id)
        .await?
        .into_iter()
        .map(|share_link| share_link.into_share_link(String::new()))
        .collect();
    Ok(Json(ShareLinksEnvelope { items }))
}

async fn update_share_link(
    State(state): State<ApiState>,
    Path((workspace_id, share_link_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    ValidatedJson(payload): ValidatedJson<UpdateShareLinkRequest>,
) -> Result<Json<ShareLinkEnvelope>, ApiError> {
    validate_share_link_update_request(&payload)?;
    let if_match = extract_if_match(&headers)?;
    let share_link =
        state.store.update_share_link(workspace_id, share_link_id, if_match, payload).await?;

    Ok(Json(ShareLinkEnvelope { share_link: share_link.into_share_link(String::new()) }))
}

async fn revoke_share_link(
    State(state): State<ApiState>,
    Path((workspace_id, share_link_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let if_match = extract_if_match(&headers)?;
    state.store.revoke_share_link(workspace_id, share_link_id, if_match).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn redeem_share_link(
    State(state): State<ApiState>,
    ValidatedJson(payload): ValidatedJson<RedeemShareLinkRequest>,
) -> Result<Json<RedeemShareLinkResponse>, ApiError> {
    if payload.token.trim().is_empty() {
        return Err(ApiError::bad_request("VALIDATION_ERROR", "token is required"));
    }

    let token_hash = hash_share_link_token(&payload.token);

    // Rate limit check per token hash.
    {
        let mut limiter = state.redeem_limiter.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let entry = limiter
            .entry(token_hash.clone())
            .or_insert(RedeemRateEntry { count: 0, window_start: now });
        if now.duration_since(entry.window_start).as_secs() > REDEEM_RATE_LIMIT_WINDOW_SECS {
            entry.count = 0;
            entry.window_start = now;
        }
        entry.count += 1;
        if entry.count > REDEEM_RATE_LIMIT_MAX {
            return Err(ApiError::rate_limited(
                "RATE_LIMITED",
                "too many redeem attempts for this token",
            ));
        }
    }

    let link = state.store.find_share_link_by_token_hash(token_hash).await?;

    if link.disabled {
        return Err(ApiError::bad_request(
            "SHARE_LINK_DISABLED",
            "this share link has been disabled",
        ));
    }
    if link.revoked_at.is_some() {
        return Err(ApiError::bad_request(
            "SHARE_LINK_REVOKED",
            "this share link has been revoked",
        ));
    }
    if link.expires_at.is_some_and(|expires| expires < Utc::now()) {
        return Err(ApiError::bad_request("SHARE_LINK_EXPIRED", "this share link has expired"));
    }
    if link.max_uses.is_some_and(|max| link.use_count >= max) {
        return Err(ApiError::bad_request(
            "SHARE_LINK_EXHAUSTED",
            "this share link has reached its maximum uses",
        ));
    }

    // Verify password if required.
    if let Some(ref hash) = link.password_hash {
        let password = payload.password.as_deref().unwrap_or("");
        verify_share_link_password(password, hash)?;
    }

    state.store.increment_share_link_use_count(link.id).await?;

    let remaining_uses = link.max_uses.map(|max| max - link.use_count - 1);

    Ok(Json(RedeemShareLinkResponse {
        workspace_id: link.workspace_id,
        target_type: link.target_type,
        target_id: link.target_id,
        permission: link.permission,
        remaining_uses,
    }))
}

async fn require_workspace_viewer_role(
    State(state): State<ApiState>,
    Extension(user): Extension<AuthenticatedUser>,
    request: Request,
    next: Next,
) -> Response {
    require_workspace_role(state, user, request, next, WorkspaceRole::Viewer).await
}

async fn require_workspace_editor_role(
    State(state): State<ApiState>,
    Extension(user): Extension<AuthenticatedUser>,
    request: Request,
    next: Next,
) -> Response {
    require_workspace_role(state, user, request, next, WorkspaceRole::Editor).await
}

async fn require_workspace_owner_role(
    State(state): State<ApiState>,
    Extension(user): Extension<AuthenticatedUser>,
    request: Request,
    next: Next,
) -> Response {
    require_workspace_role(state, user, request, next, WorkspaceRole::Owner).await
}

async fn require_workspace_role(
    state: ApiState,
    user: AuthenticatedUser,
    request: Request,
    next: Next,
    required_role: WorkspaceRole,
) -> Response {
    let (mut request, workspace_id) = match extract_workspace_id(request).await {
        Ok(result) => result,
        Err(error) => return error.into_response(),
    };

    let role = match state.store.workspace_role_for_user(user.user_id, workspace_id).await {
        Ok(Some(role)) => role,
        Ok(None) => {
            return ApiError::forbidden("AUTH_FORBIDDEN", "caller lacks workspace access")
                .into_response();
        }
        Err(error) => return error.into_response(),
    };

    if !role.allows(required_role) {
        return ApiError::forbidden("AUTH_FORBIDDEN", "caller lacks required role").into_response();
    }

    request.extensions_mut().insert(role);
    next.run(request).await
}

async fn extract_workspace_id(request: Request) -> Result<(Request, Uuid), ApiError> {
    let (mut parts, body) = request.into_parts();
    let Path(path_params) = Path::<HashMap<String, String>>::from_request_parts(&mut parts, &())
        .await
        .map_err(|_| ApiError::forbidden("AUTH_FORBIDDEN", "caller lacks workspace access"))?;
    let raw_workspace_id = path_params
        .get("workspace_id")
        .or_else(|| path_params.get("id"))
        .ok_or_else(|| ApiError::forbidden("AUTH_FORBIDDEN", "caller lacks workspace access"))?;
    let workspace_id = Uuid::parse_str(raw_workspace_id)
        .map_err(|_| ApiError::forbidden("AUTH_FORBIDDEN", "caller lacks workspace access"))?;

    Ok((Request::from_parts(parts, body), workspace_id))
}

impl WorkspaceStore {
    async fn create_workspace(
        &self,
        user_id: Uuid,
        name: String,
        slug: String,
    ) -> Result<WorkspaceRecord, ApiError> {
        match self {
            Self::Postgres(pool) => create_workspace_pg(pool, user_id, &name, &slug).await,
            Self::Memory(store) => create_workspace_memory(store, user_id, &name, &slug).await,
        }
    }

    async fn list_workspaces(
        &self,
        user_id: Uuid,
        limit: usize,
        cursor: Option<WorkspaceCursor>,
    ) -> Result<WorkspacePage, ApiError> {
        match self {
            Self::Postgres(pool) => list_workspaces_pg(pool, user_id, limit, cursor).await,
            Self::Memory(store) => list_workspaces_memory(store, user_id, limit, cursor).await,
        }
    }

    async fn get_workspace(
        &self,
        user_id: Uuid,
        workspace_id: Uuid,
    ) -> Result<WorkspaceRecord, ApiError> {
        match self {
            Self::Postgres(pool) => get_workspace_pg(pool, user_id, workspace_id).await,
            Self::Memory(store) => get_workspace_memory(store, user_id, workspace_id).await,
        }
    }

    async fn workspace_role_for_user(
        &self,
        user_id: Uuid,
        workspace_id: Uuid,
    ) -> Result<Option<WorkspaceRole>, ApiError> {
        match self {
            Self::Postgres(pool) => workspace_role_for_user_pg(pool, user_id, workspace_id).await,
            Self::Memory(store) => {
                workspace_role_for_user_memory(store, user_id, workspace_id).await
            }
        }
    }

    async fn create_share_link(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
        payload: CreateShareLinkRequest,
        token_hash: Vec<u8>,
        password_hash: Option<String>,
    ) -> Result<ShareLinkRecord, ApiError> {
        match self {
            Self::Postgres(pool) => {
                create_share_link_pg(
                    pool,
                    workspace_id,
                    user_id,
                    payload,
                    token_hash,
                    password_hash,
                )
                .await
            }
            Self::Memory(store) => {
                create_share_link_memory(
                    store,
                    workspace_id,
                    user_id,
                    payload,
                    token_hash,
                    password_hash,
                )
                .await
            }
        }
    }

    async fn list_share_links(&self, workspace_id: Uuid) -> Result<Vec<ShareLinkRecord>, ApiError> {
        match self {
            Self::Postgres(pool) => list_share_links_pg(pool, workspace_id).await,
            Self::Memory(store) => list_share_links_memory(store, workspace_id).await,
        }
    }

    async fn update_share_link(
        &self,
        workspace_id: Uuid,
        share_link_id: Uuid,
        if_match: &str,
        payload: UpdateShareLinkRequest,
    ) -> Result<ShareLinkRecord, ApiError> {
        match self {
            Self::Postgres(pool) => {
                update_share_link_pg(pool, workspace_id, share_link_id, if_match, payload).await
            }
            Self::Memory(store) => {
                update_share_link_memory(store, workspace_id, share_link_id, if_match, payload)
                    .await
            }
        }
    }

    async fn revoke_share_link(
        &self,
        workspace_id: Uuid,
        share_link_id: Uuid,
        if_match: &str,
    ) -> Result<(), ApiError> {
        match self {
            Self::Postgres(pool) => {
                revoke_share_link_pg(pool, workspace_id, share_link_id, if_match).await
            }
            Self::Memory(store) => {
                revoke_share_link_memory(store, workspace_id, share_link_id, if_match).await
            }
        }
    }

    async fn update_workspace(
        &self,
        user_id: Uuid,
        workspace_id: Uuid,
        if_match: &str,
        payload: UpdateWorkspaceRequest,
    ) -> Result<WorkspaceRecord, ApiError> {
        match self {
            Self::Postgres(pool) => {
                update_workspace_pg(pool, user_id, workspace_id, if_match, payload).await
            }
            Self::Memory(store) => {
                update_workspace_memory(store, user_id, workspace_id, if_match, payload).await
            }
        }
    }

    async fn list_members(
        &self,
        workspace_id: Uuid,
        limit: usize,
        cursor: Option<MemberCursor>,
    ) -> Result<MemberPage, ApiError> {
        match self {
            Self::Postgres(pool) => list_members_pg(pool, workspace_id, limit, cursor).await,
            Self::Memory(store) => list_members_memory(store, workspace_id, limit, cursor).await,
        }
    }

    async fn update_member(
        &self,
        workspace_id: Uuid,
        member_id: Uuid,
        if_match: &str,
        payload: UpdateMemberRequest,
    ) -> Result<MemberRecord, ApiError> {
        match self {
            Self::Postgres(pool) => {
                update_member_pg(pool, workspace_id, member_id, if_match, payload).await
            }
            Self::Memory(store) => {
                update_member_memory(store, workspace_id, member_id, if_match, payload).await
            }
        }
    }

    async fn delete_member(
        &self,
        workspace_id: Uuid,
        member_id: Uuid,
        if_match: &str,
    ) -> Result<(), ApiError> {
        match self {
            Self::Postgres(pool) => delete_member_pg(pool, workspace_id, member_id, if_match).await,
            Self::Memory(store) => {
                delete_member_memory(store, workspace_id, member_id, if_match).await
            }
        }
    }

    async fn create_invite(
        &self,
        workspace_id: Uuid,
        invited_by: Uuid,
        payload: CreateInviteRequest,
        token_hash: Vec<u8>,
        expires_at: DateTime<Utc>,
    ) -> Result<InviteRecord, ApiError> {
        match self {
            Self::Postgres(pool) => {
                create_invite_pg(pool, workspace_id, invited_by, payload, token_hash, expires_at)
                    .await
            }
            Self::Memory(store) => {
                create_invite_memory(
                    store,
                    workspace_id,
                    invited_by,
                    payload,
                    token_hash,
                    expires_at,
                )
                .await
            }
        }
    }

    async fn accept_invite(
        &self,
        token_hash: Vec<u8>,
        user_id: Uuid,
    ) -> Result<InviteRecord, ApiError> {
        match self {
            Self::Postgres(pool) => accept_invite_pg(pool, token_hash, user_id).await,
            Self::Memory(store) => accept_invite_memory(store, token_hash, user_id).await,
        }
    }

    async fn find_share_link_by_token_hash(
        &self,
        token_hash: Vec<u8>,
    ) -> Result<ShareLinkLookup, ApiError> {
        match self {
            Self::Postgres(pool) => find_share_link_by_token_hash_pg(pool, token_hash).await,
            Self::Memory(store) => find_share_link_by_token_hash_memory(store, token_hash).await,
        }
    }

    async fn increment_share_link_use_count(&self, share_link_id: Uuid) -> Result<(), ApiError> {
        match self {
            Self::Postgres(pool) => increment_share_link_use_count_pg(pool, share_link_id).await,
            Self::Memory(store) => {
                increment_share_link_use_count_memory(store, share_link_id).await
            }
        }
    }
}

async fn create_workspace_pg(
    pool: &PgPool,
    user_id: Uuid,
    name: &str,
    slug: &str,
) -> Result<WorkspaceRecord, ApiError> {
    let mut tx = pool.begin().await.map_err(|error| ApiError::internal(error.into()))?;

    let row = sqlx::query_as::<_, WorkspaceRow>(
        r#"
        INSERT INTO workspaces (slug, name, created_by)
        VALUES ($1, $2, $3)
        RETURNING id, slug, name, 'owner'::text AS role, created_at, updated_at
        "#,
    )
    .bind(slug)
    .bind(name)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    sqlx::query(
        r#"
        INSERT INTO workspace_members (workspace_id, user_id, role, status)
        VALUES ($1, $2, 'owner', 'active')
        "#,
    )
    .bind(row.id)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    tx.commit().await.map_err(|error| ApiError::internal(error.into()))?;

    Ok(row.into())
}

async fn list_workspaces_pg(
    pool: &PgPool,
    user_id: Uuid,
    limit: usize,
    cursor: Option<WorkspaceCursor>,
) -> Result<WorkspacePage, ApiError> {
    let cursor_created_at = cursor.as_ref().map(|value| value.created_at);
    let cursor_id = cursor.as_ref().map(|value| value.id);

    let mut items: Vec<WorkspaceRecord> = sqlx::query_as::<_, WorkspaceRow>(
        r#"
        SELECT
            w.id,
            w.slug,
            w.name,
            wm.role,
            w.created_at,
            w.updated_at
        FROM workspaces AS w
        INNER JOIN workspace_members AS wm
            ON wm.workspace_id = w.id
        WHERE wm.user_id = $1
          AND wm.status = 'active'
          AND w.deleted_at IS NULL
          AND (
            $3::timestamptz IS NULL
            OR w.created_at < $3
            OR (w.created_at = $3 AND w.id < $4)
          )
        ORDER BY w.created_at DESC, w.id DESC
        LIMIT $2
        "#,
    )
    .bind(user_id)
    .bind((limit + 1) as i64)
    .bind(cursor_created_at)
    .bind(cursor_id)
    .fetch_all(pool)
    .await
    .map_err(map_sqlx_error)?
    .into_iter()
    .map(WorkspaceRecord::from)
    .collect();

    Ok(paginate_records(&mut items, limit))
}

async fn get_workspace_pg(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<WorkspaceRecord, ApiError> {
    match workspace_row_for_user(pool, workspace_id, user_id).await? {
        Some(row) => Ok(row.into()),
        None => resolve_missing_workspace_access(pool, workspace_id).await,
    }
}

async fn workspace_role_for_user_pg(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<Option<WorkspaceRole>, ApiError> {
    let role = sqlx::query_scalar::<_, String>(
        r#"
        SELECT wm.role
        FROM workspace_members AS wm
        INNER JOIN workspaces AS w
            ON w.id = wm.workspace_id
        WHERE wm.workspace_id = $1
          AND wm.user_id = $2
          AND wm.status = 'active'
          AND w.deleted_at IS NULL
        "#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?
    .map(|role| {
        WorkspaceRole::from_db_value(&role).ok_or_else(|| {
            ApiError::internal(anyhow::anyhow!("invalid workspace role '{role}' in database"))
        })
    })
    .transpose()?;

    Ok(role)
}

async fn create_share_link_pg(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    payload: CreateShareLinkRequest,
    token_hash: Vec<u8>,
    password_hash: Option<String>,
) -> Result<ShareLinkRecord, ApiError> {
    let row = sqlx::query_as::<_, ShareLinkRow>(
        r#"
        INSERT INTO share_links (
            workspace_id,
            target_type,
            target_id,
            permission,
            token_hash,
            password_hash,
            expires_at,
            max_uses,
            created_by
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        RETURNING
            id,
            target_type,
            target_id,
            permission,
            expires_at,
            max_uses,
            use_count,
            disabled,
            created_at,
            revoked_at
        "#,
    )
    .bind(workspace_id)
    .bind(payload.target_type)
    .bind(payload.target_id)
    .bind(payload.permission)
    .bind(token_hash)
    .bind(password_hash)
    .bind(payload.expires_at)
    .bind(payload.max_uses)
    .bind(user_id)
    .fetch_one(pool)
    .await
    .map_err(map_sqlx_error)?;

    Ok(row.into())
}

async fn list_share_links_pg(
    pool: &PgPool,
    workspace_id: Uuid,
) -> Result<Vec<ShareLinkRecord>, ApiError> {
    let rows = sqlx::query_as::<_, ShareLinkRow>(
        r#"
        SELECT
            id,
            target_type,
            target_id,
            permission,
            expires_at,
            max_uses,
            use_count,
            disabled,
            created_at,
            revoked_at
        FROM share_links
        WHERE workspace_id = $1
        ORDER BY created_at DESC, id DESC
        "#,
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await
    .map_err(map_sqlx_error)?;

    Ok(rows.into_iter().map(ShareLinkRecord::from).collect())
}

async fn share_link_record_by_id_pg(
    pool: &PgPool,
    workspace_id: Uuid,
    share_link_id: Uuid,
) -> Result<Option<ShareLinkRecord>, ApiError> {
    let row = sqlx::query_as::<_, ShareLinkRow>(
        r#"
        SELECT
            id,
            target_type,
            target_id,
            permission,
            expires_at,
            max_uses,
            use_count,
            disabled,
            created_at,
            revoked_at
        FROM share_links
        WHERE workspace_id = $1
          AND id = $2
        "#,
    )
    .bind(workspace_id)
    .bind(share_link_id)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?;

    Ok(row.map(ShareLinkRecord::from))
}

async fn update_share_link_pg(
    pool: &PgPool,
    workspace_id: Uuid,
    share_link_id: Uuid,
    if_match: &str,
    payload: UpdateShareLinkRequest,
) -> Result<ShareLinkRecord, ApiError> {
    let Some(current) = share_link_record_by_id_pg(pool, workspace_id, share_link_id).await? else {
        return Err(ApiError::not_found("SHARE_LINK_NOT_FOUND", "share link does not exist"));
    };

    if !etag_matches(if_match, &current.etag()) {
        return Err(ApiError::PreconditionFailed);
    }

    let permission = payload.permission.unwrap_or(current.permission);
    let expires_at = match payload.expires_at {
        Some(value) => value,
        None => current.expires_at,
    };
    let max_uses = match payload.max_uses {
        Some(value) => value,
        None => current.max_uses,
    };
    let disabled = payload.disabled.unwrap_or(current.disabled);

    let row = sqlx::query_as::<_, ShareLinkRow>(
        r#"
        UPDATE share_links
        SET
            permission = $3,
            expires_at = $4,
            max_uses = $5,
            disabled = $6
        WHERE workspace_id = $1
          AND id = $2
        RETURNING
            id,
            target_type,
            target_id,
            permission,
            expires_at,
            max_uses,
            use_count,
            disabled,
            created_at,
            revoked_at
        "#,
    )
    .bind(workspace_id)
    .bind(share_link_id)
    .bind(permission)
    .bind(expires_at)
    .bind(max_uses)
    .bind(disabled)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?
    .ok_or_else(|| ApiError::not_found("SHARE_LINK_NOT_FOUND", "share link does not exist"))?;

    Ok(row.into())
}

async fn revoke_share_link_pg(
    pool: &PgPool,
    workspace_id: Uuid,
    share_link_id: Uuid,
    if_match: &str,
) -> Result<(), ApiError> {
    let Some(current) = share_link_record_by_id_pg(pool, workspace_id, share_link_id).await? else {
        return Err(ApiError::not_found("SHARE_LINK_NOT_FOUND", "share link does not exist"));
    };

    if !etag_matches(if_match, &current.etag()) {
        return Err(ApiError::PreconditionFailed);
    }

    let result = sqlx::query(
        r#"
        UPDATE share_links
        SET
            disabled = TRUE,
            revoked_at = COALESCE(revoked_at, now())
        WHERE workspace_id = $1
          AND id = $2
        "#,
    )
    .bind(workspace_id)
    .bind(share_link_id)
    .execute(pool)
    .await
    .map_err(map_sqlx_error)?;

    if result.rows_affected() == 0 {
        return Err(ApiError::not_found("SHARE_LINK_NOT_FOUND", "share link does not exist"));
    }

    Ok(())
}

async fn find_share_link_by_token_hash_pg(
    pool: &PgPool,
    token_hash: Vec<u8>,
) -> Result<ShareLinkLookup, ApiError> {
    let row = sqlx::query_as::<_, ShareLinkLookupRow>(
        r#"
        SELECT
            id,
            workspace_id,
            target_type,
            target_id,
            permission,
            password_hash,
            expires_at,
            max_uses,
            use_count,
            disabled,
            revoked_at
        FROM share_links
        WHERE token_hash = $1
        "#,
    )
    .bind(&token_hash)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?
    .ok_or_else(|| ApiError::not_found("SHARE_LINK_NOT_FOUND", "share link not found"))?;

    Ok(row.into())
}

async fn increment_share_link_use_count_pg(
    pool: &PgPool,
    share_link_id: Uuid,
) -> Result<(), ApiError> {
    sqlx::query("UPDATE share_links SET use_count = use_count + 1 WHERE id = $1")
        .bind(share_link_id)
        .execute(pool)
        .await
        .map_err(map_sqlx_error)?;

    Ok(())
}

async fn update_workspace_pg(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    if_match: &str,
    payload: UpdateWorkspaceRequest,
) -> Result<WorkspaceRecord, ApiError> {
    let Some(current) = workspace_row_for_user(pool, workspace_id, user_id).await? else {
        return resolve_missing_workspace_access(pool, workspace_id).await;
    };
    if current.role != "owner" {
        return Err(ApiError::forbidden("AUTH_FORBIDDEN", "caller lacks required role"));
    }

    let current_record: WorkspaceRecord = current.into();
    if !etag_matches(if_match, &current_record.etag()) {
        return Err(ApiError::PreconditionFailed);
    }

    let row = sqlx::query_as::<_, WorkspaceRow>(
        r#"
        UPDATE workspaces
        SET
            name = COALESCE($2, name),
            slug = COALESCE($3::citext, slug),
            updated_at = CASE
                WHEN $2::text IS NULL AND $3::citext IS NULL THEN updated_at
                ELSE now()
            END
        WHERE id = $1
          AND deleted_at IS NULL
          AND updated_at = $5
        RETURNING id, slug, name, $4::text AS role, created_at, updated_at
        "#,
    )
    .bind(workspace_id)
    .bind(payload.name)
    .bind(payload.slug)
    .bind(current_record.role)
    .bind(current_record.updated_at)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?
    .ok_or(ApiError::PreconditionFailed)?;

    Ok(row.into())
}

async fn workspace_row_for_user(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<Option<WorkspaceRow>, ApiError> {
    sqlx::query_as::<_, WorkspaceRow>(
        r#"
        SELECT
            w.id,
            w.slug,
            w.name,
            wm.role,
            w.created_at,
            w.updated_at
        FROM workspaces AS w
        INNER JOIN workspace_members AS wm
            ON wm.workspace_id = w.id
        WHERE w.id = $1
          AND wm.user_id = $2
          AND wm.status = 'active'
          AND w.deleted_at IS NULL
        "#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)
}

async fn resolve_missing_workspace_access(
    pool: &PgPool,
    workspace_id: Uuid,
) -> Result<WorkspaceRecord, ApiError> {
    let exists = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM workspaces
            WHERE id = $1
              AND deleted_at IS NULL
        )
        "#,
    )
    .bind(workspace_id)
    .fetch_one(pool)
    .await
    .map_err(map_sqlx_error)?;

    if exists {
        Err(ApiError::forbidden("AUTH_FORBIDDEN", "caller lacks workspace access"))
    } else {
        Err(ApiError::not_found("WORKSPACE_NOT_FOUND", "workspace does not exist"))
    }
}

async fn create_workspace_memory(
    store: &Arc<RwLock<MemoryWorkspaceStore>>,
    user_id: Uuid,
    name: &str,
    slug: &str,
) -> Result<WorkspaceRecord, ApiError> {
    let mut state = store.write().await;

    if state.workspaces.values().any(|workspace| {
        workspace.deleted_at.is_none() && workspace.slug.eq_ignore_ascii_case(slug)
    }) {
        return Err(ApiError::conflict("WORKSPACE_SLUG_CONFLICT", "workspace slug already exists"));
    }

    let now = Utc::now();
    let workspace_id = Uuid::new_v4();
    state.workspaces.insert(
        workspace_id,
        MemoryWorkspace {
            id: workspace_id,
            slug: slug.to_owned(),
            name: name.to_owned(),
            created_at: now,
            updated_at: now,
            deleted_at: None,
        },
    );
    state.memberships.insert(
        (workspace_id, user_id),
        MemoryMembership {
            role: "owner".to_owned(),
            status: "active".to_owned(),
            email: format!("user-{}@test.local", user_id),
            display_name: format!("User {}", &user_id.to_string()[..8]),
            joined_at: now,
        },
    );

    Ok(WorkspaceRecord {
        id: workspace_id,
        slug: slug.to_owned(),
        name: name.to_owned(),
        role: "owner".to_owned(),
        created_at: now,
        updated_at: now,
    })
}

async fn list_workspaces_memory(
    store: &Arc<RwLock<MemoryWorkspaceStore>>,
    user_id: Uuid,
    limit: usize,
    cursor: Option<WorkspaceCursor>,
) -> Result<WorkspacePage, ApiError> {
    let state = store.read().await;

    let mut items: Vec<WorkspaceRecord> = state
        .memberships
        .iter()
        .filter_map(|((workspace_id, member_user_id), membership)| {
            if *member_user_id != user_id || membership.status != "active" {
                return None;
            }
            let workspace = state.workspaces.get(workspace_id)?;
            if workspace.deleted_at.is_some() {
                return None;
            }

            Some(WorkspaceRecord {
                id: workspace.id,
                slug: workspace.slug.clone(),
                name: workspace.name.clone(),
                role: membership.role.clone(),
                created_at: workspace.created_at,
                updated_at: workspace.updated_at,
            })
        })
        .collect();

    items.sort_by(|left, right| {
        right.created_at.cmp(&left.created_at).then_with(|| right.id.cmp(&left.id))
    });

    if let Some(cursor) = cursor {
        items.retain(|item| {
            item.created_at < cursor.created_at
                || (item.created_at == cursor.created_at && item.id < cursor.id)
        });
    }

    Ok(paginate_records(&mut items, limit))
}

async fn get_workspace_memory(
    store: &Arc<RwLock<MemoryWorkspaceStore>>,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<WorkspaceRecord, ApiError> {
    let state = store.read().await;
    let Some(workspace) = state.workspaces.get(&workspace_id) else {
        return Err(ApiError::not_found("WORKSPACE_NOT_FOUND", "workspace does not exist"));
    };
    if workspace.deleted_at.is_some() {
        return Err(ApiError::not_found("WORKSPACE_NOT_FOUND", "workspace does not exist"));
    }

    let Some(member) = state.memberships.get(&(workspace_id, user_id)) else {
        return Err(ApiError::forbidden("AUTH_FORBIDDEN", "caller lacks workspace access"));
    };
    if member.status != "active" {
        return Err(ApiError::forbidden("AUTH_FORBIDDEN", "caller lacks workspace access"));
    }

    Ok(WorkspaceRecord {
        id: workspace.id,
        slug: workspace.slug.clone(),
        name: workspace.name.clone(),
        role: member.role.clone(),
        created_at: workspace.created_at,
        updated_at: workspace.updated_at,
    })
}

async fn workspace_role_for_user_memory(
    store: &Arc<RwLock<MemoryWorkspaceStore>>,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<Option<WorkspaceRole>, ApiError> {
    let state = store.read().await;
    let Some(workspace) = state.workspaces.get(&workspace_id) else {
        return Ok(None);
    };
    if workspace.deleted_at.is_some() {
        return Ok(None);
    }

    let Some(member) = state.memberships.get(&(workspace_id, user_id)) else {
        return Ok(None);
    };
    if member.status != "active" {
        return Ok(None);
    }

    let role = WorkspaceRole::from_db_value(&member.role).ok_or_else(|| {
        ApiError::internal(anyhow::anyhow!(
            "invalid workspace role '{}' in memory store",
            member.role
        ))
    })?;

    Ok(Some(role))
}

async fn create_share_link_memory(
    store: &Arc<RwLock<MemoryWorkspaceStore>>,
    workspace_id: Uuid,
    user_id: Uuid,
    payload: CreateShareLinkRequest,
    token_hash: Vec<u8>,
    password_hash: Option<String>,
) -> Result<ShareLinkRecord, ApiError> {
    let mut state = store.write().await;
    let Some(workspace) = state.workspaces.get(&workspace_id) else {
        return Err(ApiError::not_found("WORKSPACE_NOT_FOUND", "workspace does not exist"));
    };
    if workspace.deleted_at.is_some() {
        return Err(ApiError::not_found("WORKSPACE_NOT_FOUND", "workspace does not exist"));
    }

    let Some(member) = state.memberships.get(&(workspace_id, user_id)) else {
        return Err(ApiError::forbidden("AUTH_FORBIDDEN", "caller lacks workspace access"));
    };
    if member.status != "active" {
        return Err(ApiError::forbidden("AUTH_FORBIDDEN", "caller lacks workspace access"));
    }

    let share_link_id = Uuid::new_v4();
    let now = Utc::now();
    let share_link = MemoryShareLink {
        id: share_link_id,
        workspace_id,
        target_type: payload.target_type,
        target_id: payload.target_id,
        permission: payload.permission,
        token_hash,
        password_hash,
        expires_at: payload.expires_at,
        max_uses: payload.max_uses,
        use_count: 0,
        disabled: false,
        created_at: now,
        revoked_at: None,
    };
    state.share_links.insert(share_link_id, share_link.clone());

    Ok(ShareLinkRecord {
        id: share_link.id,
        target_type: share_link.target_type,
        target_id: share_link.target_id,
        permission: share_link.permission,
        expires_at: share_link.expires_at,
        max_uses: share_link.max_uses,
        use_count: share_link.use_count,
        disabled: share_link.disabled,
        created_at: share_link.created_at,
        revoked_at: share_link.revoked_at,
    })
}

async fn list_share_links_memory(
    store: &Arc<RwLock<MemoryWorkspaceStore>>,
    workspace_id: Uuid,
) -> Result<Vec<ShareLinkRecord>, ApiError> {
    let state = store.read().await;
    let Some(workspace) = state.workspaces.get(&workspace_id) else {
        return Err(ApiError::not_found("WORKSPACE_NOT_FOUND", "workspace does not exist"));
    };
    if workspace.deleted_at.is_some() {
        return Err(ApiError::not_found("WORKSPACE_NOT_FOUND", "workspace does not exist"));
    }

    let mut items = state
        .share_links
        .values()
        .filter(|share_link| share_link.workspace_id == workspace_id)
        .cloned()
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right.created_at.cmp(&left.created_at).then_with(|| right.id.cmp(&left.id))
    });

    Ok(items
        .into_iter()
        .map(|share_link| ShareLinkRecord {
            id: share_link.id,
            target_type: share_link.target_type,
            target_id: share_link.target_id,
            permission: share_link.permission,
            expires_at: share_link.expires_at,
            max_uses: share_link.max_uses,
            use_count: share_link.use_count,
            disabled: share_link.disabled,
            created_at: share_link.created_at,
            revoked_at: share_link.revoked_at,
        })
        .collect())
}

async fn update_share_link_memory(
    store: &Arc<RwLock<MemoryWorkspaceStore>>,
    workspace_id: Uuid,
    share_link_id: Uuid,
    if_match: &str,
    payload: UpdateShareLinkRequest,
) -> Result<ShareLinkRecord, ApiError> {
    let mut state = store.write().await;
    let Some(workspace) = state.workspaces.get(&workspace_id) else {
        return Err(ApiError::not_found("WORKSPACE_NOT_FOUND", "workspace does not exist"));
    };
    if workspace.deleted_at.is_some() {
        return Err(ApiError::not_found("WORKSPACE_NOT_FOUND", "workspace does not exist"));
    }

    let Some(share_link) = state.share_links.get_mut(&share_link_id) else {
        return Err(ApiError::not_found("SHARE_LINK_NOT_FOUND", "share link does not exist"));
    };
    if share_link.workspace_id != workspace_id {
        return Err(ApiError::not_found("SHARE_LINK_NOT_FOUND", "share link does not exist"));
    }

    let current = ShareLinkRecord {
        id: share_link.id,
        target_type: share_link.target_type.clone(),
        target_id: share_link.target_id,
        permission: share_link.permission.clone(),
        expires_at: share_link.expires_at,
        max_uses: share_link.max_uses,
        use_count: share_link.use_count,
        disabled: share_link.disabled,
        created_at: share_link.created_at,
        revoked_at: share_link.revoked_at,
    };
    if !etag_matches(if_match, &current.etag()) {
        return Err(ApiError::PreconditionFailed);
    }

    if let Some(permission) = payload.permission {
        share_link.permission = permission;
    }
    if let Some(expires_at) = payload.expires_at {
        share_link.expires_at = expires_at;
    }
    if let Some(max_uses) = payload.max_uses {
        share_link.max_uses = max_uses;
    }
    if let Some(disabled) = payload.disabled {
        share_link.disabled = disabled;
    }

    Ok(ShareLinkRecord {
        id: share_link.id,
        target_type: share_link.target_type.clone(),
        target_id: share_link.target_id,
        permission: share_link.permission.clone(),
        expires_at: share_link.expires_at,
        max_uses: share_link.max_uses,
        use_count: share_link.use_count,
        disabled: share_link.disabled,
        created_at: share_link.created_at,
        revoked_at: share_link.revoked_at,
    })
}

async fn revoke_share_link_memory(
    store: &Arc<RwLock<MemoryWorkspaceStore>>,
    workspace_id: Uuid,
    share_link_id: Uuid,
    if_match: &str,
) -> Result<(), ApiError> {
    let mut state = store.write().await;
    let Some(workspace) = state.workspaces.get(&workspace_id) else {
        return Err(ApiError::not_found("WORKSPACE_NOT_FOUND", "workspace does not exist"));
    };
    if workspace.deleted_at.is_some() {
        return Err(ApiError::not_found("WORKSPACE_NOT_FOUND", "workspace does not exist"));
    }

    let Some(share_link) = state.share_links.get_mut(&share_link_id) else {
        return Err(ApiError::not_found("SHARE_LINK_NOT_FOUND", "share link does not exist"));
    };
    if share_link.workspace_id != workspace_id {
        return Err(ApiError::not_found("SHARE_LINK_NOT_FOUND", "share link does not exist"));
    }

    let current = ShareLinkRecord {
        id: share_link.id,
        target_type: share_link.target_type.clone(),
        target_id: share_link.target_id,
        permission: share_link.permission.clone(),
        expires_at: share_link.expires_at,
        max_uses: share_link.max_uses,
        use_count: share_link.use_count,
        disabled: share_link.disabled,
        created_at: share_link.created_at,
        revoked_at: share_link.revoked_at,
    };
    if !etag_matches(if_match, &current.etag()) {
        return Err(ApiError::PreconditionFailed);
    }

    share_link.disabled = true;
    if share_link.revoked_at.is_none() {
        share_link.revoked_at = Some(Utc::now());
    }

    Ok(())
}

async fn find_share_link_by_token_hash_memory(
    store: &Arc<RwLock<MemoryWorkspaceStore>>,
    token_hash: Vec<u8>,
) -> Result<ShareLinkLookup, ApiError> {
    let state = store.read().await;
    let link = state
        .share_links
        .values()
        .find(|link| link.token_hash == token_hash)
        .ok_or_else(|| ApiError::not_found("SHARE_LINK_NOT_FOUND", "share link not found"))?;

    Ok(ShareLinkLookup {
        id: link.id,
        workspace_id: link.workspace_id,
        target_type: link.target_type.clone(),
        target_id: link.target_id,
        permission: link.permission.clone(),
        password_hash: link.password_hash.clone(),
        expires_at: link.expires_at,
        max_uses: link.max_uses,
        use_count: link.use_count,
        disabled: link.disabled,
        revoked_at: link.revoked_at,
    })
}

async fn increment_share_link_use_count_memory(
    store: &Arc<RwLock<MemoryWorkspaceStore>>,
    share_link_id: Uuid,
) -> Result<(), ApiError> {
    let mut state = store.write().await;
    let link = state
        .share_links
        .get_mut(&share_link_id)
        .ok_or_else(|| ApiError::not_found("SHARE_LINK_NOT_FOUND", "share link not found"))?;
    link.use_count += 1;
    Ok(())
}

async fn update_workspace_memory(
    store: &Arc<RwLock<MemoryWorkspaceStore>>,
    user_id: Uuid,
    workspace_id: Uuid,
    if_match: &str,
    payload: UpdateWorkspaceRequest,
) -> Result<WorkspaceRecord, ApiError> {
    let mut state = store.write().await;
    let Some(member) = state.memberships.get(&(workspace_id, user_id)).cloned() else {
        return Err(ApiError::forbidden("AUTH_FORBIDDEN", "caller lacks workspace access"));
    };
    if member.status != "active" {
        return Err(ApiError::forbidden("AUTH_FORBIDDEN", "caller lacks workspace access"));
    }
    if member.role != "owner" {
        return Err(ApiError::forbidden("AUTH_FORBIDDEN", "caller lacks required role"));
    }

    if let Some(slug) = payload.slug.as_deref() {
        if state.workspaces.values().any(|workspace| {
            workspace.id != workspace_id
                && workspace.deleted_at.is_none()
                && workspace.slug.eq_ignore_ascii_case(slug)
        }) {
            return Err(ApiError::conflict(
                "WORKSPACE_SLUG_CONFLICT",
                "workspace slug already exists",
            ));
        }
    }

    let Some(workspace) = state.workspaces.get_mut(&workspace_id) else {
        return Err(ApiError::not_found("WORKSPACE_NOT_FOUND", "workspace does not exist"));
    };
    if workspace.deleted_at.is_some() {
        return Err(ApiError::not_found("WORKSPACE_NOT_FOUND", "workspace does not exist"));
    }

    if !etag_matches(if_match, &workspace_etag(workspace.id, workspace.updated_at)) {
        return Err(ApiError::PreconditionFailed);
    }

    let mut changed = false;
    if let Some(name) = payload.name {
        if workspace.name != name {
            workspace.name = name;
            changed = true;
        }
    }
    if let Some(slug) = payload.slug {
        if !workspace.slug.eq_ignore_ascii_case(&slug) || workspace.slug != slug {
            workspace.slug = slug;
            changed = true;
        }
    }
    if changed {
        workspace.updated_at = Utc::now();
    }

    Ok(WorkspaceRecord {
        id: workspace.id,
        slug: workspace.slug.clone(),
        name: workspace.name.clone(),
        role: member.role,
        created_at: workspace.created_at,
        updated_at: workspace.updated_at,
    })
}

// ── Member implementations ───────────────────────────────────────────

async fn list_members_pg(
    pool: &PgPool,
    workspace_id: Uuid,
    limit: usize,
    cursor: Option<MemberCursor>,
) -> Result<MemberPage, ApiError> {
    let cursor_joined_at = cursor.as_ref().map(|c| c.joined_at);
    let cursor_user_id = cursor.as_ref().map(|c| c.user_id);

    let rows: Vec<MemberRow> = sqlx::query_as::<_, MemberRow>(
        r#"
        SELECT
            wm.user_id,
            u.email,
            u.display_name,
            wm.role,
            wm.status,
            wm.joined_at
        FROM workspace_members AS wm
        INNER JOIN users AS u ON u.id = wm.user_id
        WHERE wm.workspace_id = $1
          AND (
            $3::timestamptz IS NULL
            OR wm.joined_at > $3
            OR (wm.joined_at = $3 AND wm.user_id > $4)
          )
        ORDER BY wm.joined_at ASC, wm.user_id ASC
        LIMIT $2
        "#,
    )
    .bind(workspace_id)
    .bind((limit + 1) as i64)
    .bind(cursor_joined_at)
    .bind(cursor_user_id)
    .fetch_all(pool)
    .await
    .map_err(map_sqlx_error)?;

    let mut items: Vec<MemberRecord> = rows.into_iter().map(MemberRecord::from).collect();
    Ok(paginate_member_records(&mut items, limit))
}

async fn update_member_pg(
    pool: &PgPool,
    workspace_id: Uuid,
    member_id: Uuid,
    if_match: &str,
    payload: UpdateMemberRequest,
) -> Result<MemberRecord, ApiError> {
    let current = sqlx::query_as::<_, MemberRow>(
        r#"
        SELECT
            wm.user_id,
            u.email,
            u.display_name,
            wm.role,
            wm.status,
            wm.joined_at
        FROM workspace_members AS wm
        INNER JOIN users AS u ON u.id = wm.user_id
        WHERE wm.workspace_id = $1
          AND wm.user_id = $2
        "#,
    )
    .bind(workspace_id)
    .bind(member_id)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?
    .ok_or_else(|| ApiError::not_found("MEMBER_NOT_FOUND", "workspace member not found"))?;

    let current_record: MemberRecord = current.into();
    if !etag_matches(if_match, &current_record.etag()) {
        return Err(ApiError::PreconditionFailed);
    }

    if current_record.role == "owner" && payload.role.as_deref().is_some_and(|r| r != "owner") {
        return Err(ApiError::bad_request("VALIDATION_ERROR", "cannot demote the last owner"));
    }

    let row = sqlx::query_as::<_, MemberRow>(
        r#"
        UPDATE workspace_members AS wm
        SET
            role = COALESCE($3, wm.role),
            status = COALESCE($4, wm.status)
        FROM users AS u
        WHERE wm.workspace_id = $1
          AND wm.user_id = $2
          AND u.id = wm.user_id
        RETURNING wm.user_id, u.email, u.display_name, wm.role, wm.status, wm.joined_at
        "#,
    )
    .bind(workspace_id)
    .bind(member_id)
    .bind(payload.role)
    .bind(payload.status)
    .fetch_one(pool)
    .await
    .map_err(map_sqlx_error)?;

    Ok(row.into())
}

async fn delete_member_pg(
    pool: &PgPool,
    workspace_id: Uuid,
    member_id: Uuid,
    if_match: &str,
) -> Result<(), ApiError> {
    let current = sqlx::query_as::<_, MemberRow>(
        r#"
        SELECT
            wm.user_id,
            u.email,
            u.display_name,
            wm.role,
            wm.status,
            wm.joined_at
        FROM workspace_members AS wm
        INNER JOIN users AS u ON u.id = wm.user_id
        WHERE wm.workspace_id = $1
          AND wm.user_id = $2
        "#,
    )
    .bind(workspace_id)
    .bind(member_id)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?
    .ok_or_else(|| ApiError::not_found("MEMBER_NOT_FOUND", "workspace member not found"))?;

    let current_record: MemberRecord = current.into();
    if !etag_matches(if_match, &current_record.etag()) {
        return Err(ApiError::PreconditionFailed);
    }

    if current_record.role == "owner" {
        return Err(ApiError::bad_request("VALIDATION_ERROR", "cannot remove a workspace owner"));
    }

    sqlx::query("DELETE FROM workspace_members WHERE workspace_id = $1 AND user_id = $2")
        .bind(workspace_id)
        .bind(member_id)
        .execute(pool)
        .await
        .map_err(map_sqlx_error)?;

    Ok(())
}

async fn list_members_memory(
    store: &Arc<RwLock<MemoryWorkspaceStore>>,
    workspace_id: Uuid,
    limit: usize,
    cursor: Option<MemberCursor>,
) -> Result<MemberPage, ApiError> {
    let state = store.read().await;

    let mut items: Vec<MemberRecord> = state
        .memberships
        .iter()
        .filter_map(|((ws_id, user_id), membership)| {
            if *ws_id != workspace_id {
                return None;
            }
            Some(MemberRecord {
                user_id: *user_id,
                email: membership.email.clone(),
                display_name: membership.display_name.clone(),
                role: membership.role.clone(),
                status: membership.status.clone(),
                joined_at: membership.joined_at,
            })
        })
        .collect();

    items.sort_by(|left, right| {
        left.joined_at.cmp(&right.joined_at).then_with(|| left.user_id.cmp(&right.user_id))
    });

    if let Some(cursor) = cursor {
        items.retain(|item| {
            item.joined_at > cursor.joined_at
                || (item.joined_at == cursor.joined_at && item.user_id > cursor.user_id)
        });
    }

    Ok(paginate_member_records(&mut items, limit))
}

async fn update_member_memory(
    store: &Arc<RwLock<MemoryWorkspaceStore>>,
    workspace_id: Uuid,
    member_id: Uuid,
    if_match: &str,
    payload: UpdateMemberRequest,
) -> Result<MemberRecord, ApiError> {
    let mut state = store.write().await;
    let Some(member) = state.memberships.get(&(workspace_id, member_id)) else {
        return Err(ApiError::not_found("MEMBER_NOT_FOUND", "workspace member not found"));
    };

    let current_etag = member_etag(member_id, member.joined_at);
    if !etag_matches(if_match, &current_etag) {
        return Err(ApiError::PreconditionFailed);
    }

    if member.role == "owner" && payload.role.as_deref().is_some_and(|r| r != "owner") {
        return Err(ApiError::bad_request("VALIDATION_ERROR", "cannot demote the last owner"));
    }

    let member = state.memberships.get_mut(&(workspace_id, member_id)).unwrap();

    if let Some(role) = payload.role {
        member.role = role;
    }
    if let Some(status) = payload.status {
        member.status = status;
    }

    Ok(MemberRecord {
        user_id: member_id,
        email: member.email.clone(),
        display_name: member.display_name.clone(),
        role: member.role.clone(),
        status: member.status.clone(),
        joined_at: member.joined_at,
    })
}

async fn delete_member_memory(
    store: &Arc<RwLock<MemoryWorkspaceStore>>,
    workspace_id: Uuid,
    member_id: Uuid,
    if_match: &str,
) -> Result<(), ApiError> {
    let mut state = store.write().await;
    let Some(member) = state.memberships.get(&(workspace_id, member_id)) else {
        return Err(ApiError::not_found("MEMBER_NOT_FOUND", "workspace member not found"));
    };

    let current_etag = member_etag(member_id, member.joined_at);
    if !etag_matches(if_match, &current_etag) {
        return Err(ApiError::PreconditionFailed);
    }

    if member.role == "owner" {
        return Err(ApiError::bad_request("VALIDATION_ERROR", "cannot remove a workspace owner"));
    }

    state.memberships.remove(&(workspace_id, member_id));
    Ok(())
}

// ── Invite implementations ──────────────────────────────────────────

async fn create_invite_pg(
    pool: &PgPool,
    workspace_id: Uuid,
    invited_by: Uuid,
    payload: CreateInviteRequest,
    token_hash: Vec<u8>,
    expires_at: DateTime<Utc>,
) -> Result<InviteRecord, ApiError> {
    let row = sqlx::query_as::<_, InviteRow>(
        r#"
        INSERT INTO workspace_invites (workspace_id, email, role, token_hash, invited_by, expires_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, workspace_id, email, role, token_hash, expires_at, accepted_at, created_at
        "#,
    )
    .bind(workspace_id)
    .bind(payload.email)
    .bind(payload.role)
    .bind(token_hash)
    .bind(invited_by)
    .bind(expires_at)
    .fetch_one(pool)
    .await
    .map_err(map_sqlx_error)?;

    Ok(row.into())
}

async fn accept_invite_pg(
    pool: &PgPool,
    token_hash: Vec<u8>,
    user_id: Uuid,
) -> Result<InviteRecord, ApiError> {
    let mut tx = pool.begin().await.map_err(|e| ApiError::internal(e.into()))?;

    let invite = sqlx::query_as::<_, InviteRow>(
        r#"
        SELECT id, workspace_id, email, role, token_hash, expires_at, accepted_at, created_at
        FROM workspace_invites
        WHERE token_hash = $1
        "#,
    )
    .bind(&token_hash)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .ok_or_else(|| ApiError::not_found("INVITE_NOT_FOUND", "invite not found or already used"))?;

    if invite.accepted_at.is_some() {
        return Err(ApiError::bad_request(
            "INVITE_ALREADY_ACCEPTED",
            "this invite has already been accepted",
        ));
    }
    if invite.expires_at < Utc::now() {
        return Err(ApiError::bad_request("INVITE_EXPIRED", "this invite has expired"));
    }

    // Mark invite as accepted.
    sqlx::query("UPDATE workspace_invites SET accepted_at = now() WHERE id = $1")
        .bind(invite.id)
        .execute(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

    // Add user to workspace with invited role.
    sqlx::query(
        r#"
        INSERT INTO workspace_members (workspace_id, user_id, role, status)
        VALUES ($1, $2, $3, 'active')
        ON CONFLICT (workspace_id, user_id) DO UPDATE SET
            role = EXCLUDED.role,
            status = 'active'
        "#,
    )
    .bind(invite.workspace_id)
    .bind(user_id)
    .bind(&invite.role)
    .execute(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    tx.commit().await.map_err(|e| ApiError::internal(e.into()))?;

    Ok(InviteRecord {
        id: invite.id,
        workspace_id: invite.workspace_id,
        email: invite.email,
        role: invite.role,
        token_hash: invite.token_hash,
        expires_at: invite.expires_at,
        accepted_at: Some(Utc::now()),
        created_at: invite.created_at,
    })
}

async fn create_invite_memory(
    store: &Arc<RwLock<MemoryWorkspaceStore>>,
    workspace_id: Uuid,
    _invited_by: Uuid,
    payload: CreateInviteRequest,
    token_hash: Vec<u8>,
    expires_at: DateTime<Utc>,
) -> Result<InviteRecord, ApiError> {
    let mut state = store.write().await;

    let Some(workspace) = state.workspaces.get(&workspace_id) else {
        return Err(ApiError::not_found("WORKSPACE_NOT_FOUND", "workspace does not exist"));
    };
    if workspace.deleted_at.is_some() {
        return Err(ApiError::not_found("WORKSPACE_NOT_FOUND", "workspace does not exist"));
    }

    let invite_id = Uuid::new_v4();
    let now = Utc::now();
    let invite = MemoryInvite {
        id: invite_id,
        workspace_id,
        email: payload.email,
        role: payload.role,
        token_hash: token_hash.clone(),
        expires_at,
        accepted_at: None,
        created_at: now,
    };
    state.invites.insert(invite_id, invite.clone());

    Ok(InviteRecord {
        id: invite.id,
        workspace_id: invite.workspace_id,
        email: invite.email,
        role: invite.role,
        token_hash: invite.token_hash,
        expires_at: invite.expires_at,
        accepted_at: None,
        created_at: invite.created_at,
    })
}

async fn accept_invite_memory(
    store: &Arc<RwLock<MemoryWorkspaceStore>>,
    token_hash: Vec<u8>,
    user_id: Uuid,
) -> Result<InviteRecord, ApiError> {
    let mut state = store.write().await;

    let invite =
        state.invites.values().find(|inv| inv.token_hash == token_hash).cloned().ok_or_else(
            || ApiError::not_found("INVITE_NOT_FOUND", "invite not found or already used"),
        )?;

    if invite.accepted_at.is_some() {
        return Err(ApiError::bad_request(
            "INVITE_ALREADY_ACCEPTED",
            "this invite has already been accepted",
        ));
    }
    if invite.expires_at < Utc::now() {
        return Err(ApiError::bad_request("INVITE_EXPIRED", "this invite has expired"));
    }

    // Mark as accepted.
    let now = Utc::now();
    state.invites.get_mut(&invite.id).unwrap().accepted_at = Some(now);

    // Add user as workspace member.
    state.memberships.insert(
        (invite.workspace_id, user_id),
        MemoryMembership {
            role: invite.role.clone(),
            status: "active".to_owned(),
            email: invite.email.clone(),
            display_name: format!("User {}", &user_id.to_string()[..8]),
            joined_at: now,
        },
    );

    Ok(InviteRecord {
        id: invite.id,
        workspace_id: invite.workspace_id,
        email: invite.email,
        role: invite.role,
        token_hash: invite.token_hash,
        expires_at: invite.expires_at,
        accepted_at: Some(now),
        created_at: invite.created_at,
    })
}

fn paginate_records(items: &mut Vec<WorkspaceRecord>, limit: usize) -> WorkspacePage {
    let has_more = items.len() > limit;
    if has_more {
        items.truncate(limit);
    }

    let next_cursor = if has_more {
        items.last().map(|record| encode_cursor(record.created_at, record.id))
    } else {
        None
    };

    WorkspacePage { items: std::mem::take(items), next_cursor }
}

fn paginate_member_records(items: &mut Vec<MemberRecord>, limit: usize) -> MemberPage {
    let has_more = items.len() > limit;
    if has_more {
        items.truncate(limit);
    }

    let next_cursor = if has_more {
        items.last().map(|record| encode_member_cursor(record.joined_at, record.user_id))
    } else {
        None
    };

    MemberPage { items: std::mem::take(items), next_cursor }
}

fn encode_member_cursor(joined_at: DateTime<Utc>, user_id: Uuid) -> String {
    format!("{}|{user_id}", joined_at.timestamp_micros())
}

fn parse_member_cursor(value: &str) -> Result<MemberCursor, ApiError> {
    let (timestamp, id) = value
        .split_once('|')
        .ok_or_else(|| ApiError::bad_request("INVALID_CURSOR", "cursor format is invalid"))?;

    let timestamp = timestamp
        .parse::<i64>()
        .map_err(|_| ApiError::bad_request("INVALID_CURSOR", "cursor timestamp is invalid"))?;
    let joined_at = DateTime::<Utc>::from_timestamp_micros(timestamp)
        .ok_or_else(|| ApiError::bad_request("INVALID_CURSOR", "cursor timestamp is invalid"))?;
    let user_id = Uuid::parse_str(id)
        .map_err(|_| ApiError::bad_request("INVALID_CURSOR", "cursor id is invalid"))?;

    Ok(MemberCursor { joined_at, user_id })
}

fn member_etag(user_id: Uuid, joined_at: DateTime<Utc>) -> String {
    format!("\"mem-{user_id}-{}\"", joined_at.timestamp_micros())
}

fn share_link_etag(share_link: &ShareLinkRecord) -> String {
    format!(
        "\"sl-{}-{}-{}-{}-{}-{}-{}\"",
        share_link.id,
        share_link.permission,
        share_link.expires_at.map(|value| value.timestamp_micros()).unwrap_or_default(),
        share_link.max_uses.unwrap_or_default(),
        share_link.use_count,
        i32::from(share_link.disabled),
        share_link.revoked_at.map(|value| value.timestamp_micros()).unwrap_or_default()
    )
}

fn validate_member_update(payload: &UpdateMemberRequest) -> Result<(), ApiError> {
    if let Some(role) = payload.role.as_deref() {
        if role != "owner" && role != "editor" && role != "viewer" {
            return Err(ApiError::bad_request(
                "VALIDATION_ERROR",
                "role must be one of: owner, editor, viewer",
            ));
        }
    }
    if let Some(status) = payload.status.as_deref() {
        if status != "active" && status != "invited" && status != "suspended" {
            return Err(ApiError::bad_request(
                "VALIDATION_ERROR",
                "status must be one of: active, invited, suspended",
            ));
        }
    }

    Ok(())
}

fn validate_invite_request(payload: &CreateInviteRequest) -> Result<(), ApiError> {
    if payload.email.trim().is_empty() || !payload.email.contains('@') {
        return Err(ApiError::bad_request("VALIDATION_ERROR", "a valid email address is required"));
    }
    if payload.role != "editor" && payload.role != "viewer" {
        return Err(ApiError::bad_request(
            "VALIDATION_ERROR",
            "invite role must be one of: editor, viewer",
        ));
    }
    if let Some(hours) = payload.expires_in_hours {
        if hours == 0 || hours > MAX_INVITE_EXPIRY_HOURS {
            return Err(ApiError::bad_request(
                "VALIDATION_ERROR",
                format!("expires_in_hours must be between 1 and {MAX_INVITE_EXPIRY_HOURS}"),
            ));
        }
    }

    Ok(())
}

fn normalize_limit(limit: Option<usize>) -> usize {
    match limit {
        Some(0) => DEFAULT_PAGE_SIZE,
        Some(value) => value.min(MAX_PAGE_SIZE),
        None => DEFAULT_PAGE_SIZE,
    }
}

fn parse_cursor(value: &str) -> Result<WorkspaceCursor, ApiError> {
    let (timestamp, id) = value
        .split_once('|')
        .ok_or_else(|| ApiError::bad_request("INVALID_CURSOR", "cursor format is invalid"))?;

    let timestamp = timestamp
        .parse::<i64>()
        .map_err(|_| ApiError::bad_request("INVALID_CURSOR", "cursor timestamp is invalid"))?;
    let created_at = DateTime::<Utc>::from_timestamp_micros(timestamp)
        .ok_or_else(|| ApiError::bad_request("INVALID_CURSOR", "cursor timestamp is invalid"))?;
    let id = Uuid::parse_str(id)
        .map_err(|_| ApiError::bad_request("INVALID_CURSOR", "cursor id is invalid"))?;

    Ok(WorkspaceCursor { created_at, id })
}

fn encode_cursor(created_at: DateTime<Utc>, id: Uuid) -> String {
    format!("{}|{id}", created_at.timestamp_micros())
}

fn extract_if_match(headers: &HeaderMap) -> Result<&str, ApiError> {
    let value = headers.get(IF_MATCH).ok_or(ApiError::PreconditionRequired)?;
    value.to_str().map_err(|_| {
        ApiError::bad_request("INVALID_IF_MATCH", "If-Match header is not valid utf-8")
    })
}

fn etag_matches(if_match: &str, current_etag: &str) -> bool {
    if if_match.trim() == "*" {
        return true;
    }

    normalize_etag(if_match) == normalize_etag(current_etag)
}

fn normalize_etag(value: &str) -> &str {
    let value = value.trim();
    let value = value.strip_prefix("W/").unwrap_or(value);
    value.trim().trim_matches('"')
}

fn workspace_etag(workspace_id: Uuid, updated_at: DateTime<Utc>) -> String {
    format!("\"ws-{workspace_id}-{}\"", updated_at.timestamp_micros())
}

fn validate_name(name: &str) -> Result<(), ApiError> {
    if name.trim().is_empty() {
        return Err(ApiError::bad_request("VALIDATION_ERROR", "workspace name must not be empty"));
    }

    Ok(())
}

fn validate_slug(slug: &str) -> Result<(), ApiError> {
    if slug.trim().is_empty() {
        return Err(ApiError::bad_request("VALIDATION_ERROR", "workspace slug must not be empty"));
    }

    Ok(())
}

fn validate_share_link_request(
    workspace_id: Uuid,
    payload: &CreateShareLinkRequest,
) -> Result<(), ApiError> {
    if payload.target_type != "workspace" && payload.target_type != "document" {
        return Err(ApiError::bad_request(
            "VALIDATION_ERROR",
            "target_type must be one of: workspace, document",
        ));
    }
    if payload.permission != "view" && payload.permission != "edit" {
        return Err(ApiError::bad_request(
            "VALIDATION_ERROR",
            "permission must be one of: view, edit",
        ));
    }
    if payload.target_type == "workspace" && payload.target_id != workspace_id {
        return Err(ApiError::bad_request(
            "VALIDATION_ERROR",
            "workspace share links must target the workspace id from the route",
        ));
    }
    if payload.max_uses.is_some_and(|value| value <= 0) {
        return Err(ApiError::bad_request("VALIDATION_ERROR", "max_uses must be greater than 0"));
    }
    if payload.expires_at.is_some_and(|value| value <= Utc::now()) {
        return Err(ApiError::bad_request("VALIDATION_ERROR", "expires_at must be in the future"));
    }

    Ok(())
}

fn validate_share_link_update_request(payload: &UpdateShareLinkRequest) -> Result<(), ApiError> {
    if payload.permission.is_none()
        && payload.expires_at.is_none()
        && payload.max_uses.is_none()
        && payload.disabled.is_none()
    {
        return Err(ApiError::bad_request(
            "VALIDATION_ERROR",
            "at least one field must be provided for patch",
        ));
    }

    if let Some(permission) = payload.permission.as_deref() {
        if permission != "view" && permission != "edit" {
            return Err(ApiError::bad_request(
                "VALIDATION_ERROR",
                "permission must be one of: view, edit",
            ));
        }
    }
    if payload.max_uses.flatten().is_some_and(|value| value <= 0) {
        return Err(ApiError::bad_request("VALIDATION_ERROR", "max_uses must be greater than 0"));
    }
    if payload.expires_at.flatten().is_some_and(|value| value <= Utc::now()) {
        return Err(ApiError::bad_request("VALIDATION_ERROR", "expires_at must be in the future"));
    }

    Ok(())
}

fn generate_share_link_token() -> String {
    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn hash_share_link_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

fn hash_share_link_password(password: &str) -> Result<String, ApiError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| ApiError::internal(anyhow::anyhow!(error.to_string())))
}

fn build_share_link_url(token: &str) -> String {
    let base = env::var("SCRIPTUM_RELAY_SHARE_LINK_BASE_URL")
        .unwrap_or_else(|_| "https://scriptum.local/share".to_owned());
    format!("{}/{}", base.trim_end_matches('/'), token)
}

fn verify_share_link_password(password: &str, hash: &str) -> Result<(), ApiError> {
    let parsed = PasswordHash::new(hash)
        .map_err(|e| ApiError::internal(anyhow::anyhow!("invalid password hash: {e}")))?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| ApiError::bad_request("INVALID_PASSWORD", "incorrect share link password"))
}

fn map_sqlx_error(error: sqlx::Error) -> ApiError {
    if let sqlx::Error::Database(database_error) = &error {
        if database_error.code().as_deref() == Some("23505") {
            return ApiError::conflict("WORKSPACE_SLUG_CONFLICT", "workspace slug already exists");
        }
    }

    ApiError::internal(error.into())
}

#[cfg(test)]
mod tests {
    use super::{
        build_router_with_store, CreateWorkspaceRequest, InviteEnvelope, MemberEnvelope,
        MembersPageEnvelope, MemoryInvite, MemoryMembership, MemoryShareLink, MemoryWorkspace,
        MemoryWorkspaceStore, RedeemShareLinkResponse, ShareLinkEnvelope, ShareLinksEnvelope,
        WorkspaceEnvelope, WorkspaceStore, WorkspacesPageEnvelope,
    };
    use crate::auth::jwt::JwtAccessTokenService;
    use axum::{
        body::{to_bytes, Body},
        http::{header::AUTHORIZATION, Method, Request, StatusCode},
    };
    use chrono::Utc;
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use tower::ServiceExt;
    use uuid::Uuid;

    const TEST_SECRET: &str = "scriptum_test_secret_that_is_definitely_long_enough";

    fn test_router() -> (axum::Router, Arc<JwtAccessTokenService>, Uuid, Uuid) {
        let jwt_service = Arc::new(
            JwtAccessTokenService::new(TEST_SECRET).expect("jwt service should initialize"),
        );
        let user_id = Uuid::new_v4();
        let workspace_id_for_token = Uuid::new_v4();
        let router = build_router_with_store(
            WorkspaceStore::Memory(Default::default()),
            Arc::clone(&jwt_service),
        );

        (router, jwt_service, user_id, workspace_id_for_token)
    }

    fn bearer_token(
        jwt_service: &JwtAccessTokenService,
        user_id: Uuid,
        workspace_id: Uuid,
    ) -> String {
        jwt_service.issue_workspace_token(user_id, workspace_id).expect("token should be issued")
    }

    async fn read_json<T: serde::de::DeserializeOwned>(response: axum::response::Response) -> T {
        let body =
            to_bytes(response.into_body(), usize::MAX).await.expect("response body should read");
        serde_json::from_slice(&body).expect("response body should be valid json")
    }

    #[tokio::test]
    async fn workspace_routes_require_bearer_auth() {
        let (router, _, _, _) = test_router();

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/v1/workspaces")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should return response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn patch_workspace_requires_owner_role() {
        let jwt_service = Arc::new(
            JwtAccessTokenService::new(TEST_SECRET).expect("jwt service should initialize"),
        );
        let user_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let now = Utc::now();
        let store = Arc::new(RwLock::new(MemoryWorkspaceStore::default()));

        {
            let mut guard = store.write().await;
            guard.workspaces.insert(
                workspace_id,
                MemoryWorkspace {
                    id: workspace_id,
                    slug: "rbac".to_owned(),
                    name: "RBAC".to_owned(),
                    created_at: now,
                    updated_at: now,
                    deleted_at: None,
                },
            );
            guard.memberships.insert(
                (workspace_id, user_id),
                MemoryMembership {
                    role: "viewer".to_owned(),
                    status: "active".to_owned(),
                    email: "viewer@test.local".to_owned(),
                    display_name: "Viewer".to_owned(),
                    joined_at: now,
                },
            );
        }

        let router =
            build_router_with_store(WorkspaceStore::Memory(store), Arc::clone(&jwt_service));
        let token = bearer_token(&jwt_service, user_id, workspace_id);

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri(format!("/v1/workspaces/{workspace_id}"))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("if-match", "*")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "name": "RBAC Updated" }).to_string()))
                    .expect("request should build"),
            )
            .await
            .expect("patch request should return response");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn create_share_link_hashes_token_and_password() {
        let jwt_service = Arc::new(
            JwtAccessTokenService::new(TEST_SECRET).expect("jwt service should initialize"),
        );
        let user_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let now = Utc::now();
        let store = Arc::new(RwLock::new(MemoryWorkspaceStore::default()));

        {
            let mut guard = store.write().await;
            guard.workspaces.insert(
                workspace_id,
                MemoryWorkspace {
                    id: workspace_id,
                    slug: "shared".to_owned(),
                    name: "Shared".to_owned(),
                    created_at: now,
                    updated_at: now,
                    deleted_at: None,
                },
            );
            guard.memberships.insert(
                (workspace_id, user_id),
                MemoryMembership {
                    role: "editor".to_owned(),
                    status: "active".to_owned(),
                    email: "editor@test.local".to_owned(),
                    display_name: "Editor".to_owned(),
                    joined_at: now,
                },
            );
        }

        let router = build_router_with_store(
            WorkspaceStore::Memory(store.clone()),
            Arc::clone(&jwt_service),
        );
        let token = bearer_token(&jwt_service, user_id, workspace_id);

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/v1/workspaces/{workspace_id}/share-links"))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "target_type": "workspace",
                            "target_id": workspace_id,
                            "permission": "view",
                            "max_uses": 10,
                            "password": "super-secret"
                        })
                        .to_string(),
                    ))
                    .expect("request should build"),
            )
            .await
            .expect("share link request should return response");

        assert_eq!(response.status(), StatusCode::CREATED);
        let body: ShareLinkEnvelope = read_json(response).await;
        assert_eq!(body.share_link.target_type, "workspace");
        assert_eq!(body.share_link.target_id, workspace_id);
        assert_eq!(body.share_link.permission, "view");
        assert_eq!(body.share_link.max_uses, Some(10));
        assert!(!body.share_link.url_once.is_empty());

        let token = body
            .share_link
            .url_once
            .rsplit('/')
            .next()
            .expect("share link URL should contain token");
        let guard = store.read().await;
        let stored = guard
            .share_links
            .get(&body.share_link.id)
            .expect("share link should be persisted in memory store");
        assert_eq!(stored.workspace_id, workspace_id);
        assert_eq!(stored.token_hash, Sha256::digest(token.as_bytes()).to_vec());
        assert_eq!(stored.max_uses, Some(10));
        assert_eq!(stored.use_count, 0);
        assert!(!stored.disabled);
        assert!(stored
            .password_hash
            .as_deref()
            .is_some_and(|value| value.starts_with("$argon2id$")));
    }

    #[tokio::test]
    async fn share_link_management_supports_list_patch_and_revoke() {
        let jwt_service = Arc::new(
            JwtAccessTokenService::new(TEST_SECRET).expect("jwt service should initialize"),
        );
        let user_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let now = Utc::now();
        let store = Arc::new(RwLock::new(MemoryWorkspaceStore::default()));

        {
            let mut guard = store.write().await;
            guard.workspaces.insert(
                workspace_id,
                MemoryWorkspace {
                    id: workspace_id,
                    slug: "links".to_owned(),
                    name: "Links".to_owned(),
                    created_at: now,
                    updated_at: now,
                    deleted_at: None,
                },
            );
            guard.memberships.insert(
                (workspace_id, user_id),
                MemoryMembership {
                    role: "editor".to_owned(),
                    status: "active".to_owned(),
                    email: "editor@test.local".to_owned(),
                    display_name: "Editor".to_owned(),
                    joined_at: now,
                },
            );
        }

        let router = build_router_with_store(
            WorkspaceStore::Memory(store.clone()),
            Arc::clone(&jwt_service),
        );
        let token = bearer_token(&jwt_service, user_id, workspace_id);

        let create_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/v1/workspaces/{workspace_id}/share-links"))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "target_type": "workspace",
                            "target_id": workspace_id,
                            "permission": "view"
                        })
                        .to_string(),
                    ))
                    .expect("request should build"),
            )
            .await
            .expect("create share link should return response");
        assert_eq!(create_response.status(), StatusCode::CREATED);
        let created: ShareLinkEnvelope = read_json(create_response).await;

        let list_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/v1/workspaces/{workspace_id}/share-links"))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("list share links should return response");
        assert_eq!(list_response.status(), StatusCode::OK);
        let list_body: ShareLinksEnvelope = read_json(list_response).await;
        assert_eq!(list_body.items.len(), 1);
        assert_eq!(list_body.items[0].id, created.share_link.id);

        let missing_if_match = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri(format!(
                        "/v1/workspaces/{workspace_id}/share-links/{}",
                        created.share_link.id
                    ))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "permission": "edit" }).to_string()))
                    .expect("request should build"),
            )
            .await
            .expect("patch share link should return response");
        assert_eq!(missing_if_match.status(), StatusCode::PRECONDITION_REQUIRED);

        let stale_if_match = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri(format!(
                        "/v1/workspaces/{workspace_id}/share-links/{}",
                        created.share_link.id
                    ))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("if-match", "\"stale\"")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "permission": "edit" }).to_string()))
                    .expect("request should build"),
            )
            .await
            .expect("patch share link should return response");
        assert_eq!(stale_if_match.status(), StatusCode::PRECONDITION_FAILED);

        let new_expiry = (Utc::now() + chrono::Duration::days(14)).to_rfc3339();
        let update_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri(format!(
                        "/v1/workspaces/{workspace_id}/share-links/{}",
                        created.share_link.id
                    ))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("if-match", &created.share_link.etag)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "permission": "edit",
                            "max_uses": 25,
                            "expires_at": new_expiry,
                            "disabled": false
                        })
                        .to_string(),
                    ))
                    .expect("request should build"),
            )
            .await
            .expect("patch share link should return response");
        assert_eq!(update_response.status(), StatusCode::OK);
        let updated: ShareLinkEnvelope = read_json(update_response).await;
        assert_eq!(updated.share_link.permission, "edit");
        assert_eq!(updated.share_link.max_uses, Some(25));
        assert!(!updated.share_link.disabled);
        assert_ne!(updated.share_link.etag, created.share_link.etag);

        let revoke_missing_if_match = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!(
                        "/v1/workspaces/{workspace_id}/share-links/{}",
                        created.share_link.id
                    ))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("revoke share link should return response");
        assert_eq!(revoke_missing_if_match.status(), StatusCode::PRECONDITION_REQUIRED);

        let revoke_stale_if_match = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!(
                        "/v1/workspaces/{workspace_id}/share-links/{}",
                        created.share_link.id
                    ))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("if-match", "\"stale\"")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("revoke share link should return response");
        assert_eq!(revoke_stale_if_match.status(), StatusCode::PRECONDITION_FAILED);

        let revoke_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!(
                        "/v1/workspaces/{workspace_id}/share-links/{}",
                        created.share_link.id
                    ))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("if-match", &updated.share_link.etag)
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("revoke share link should return response");
        assert_eq!(revoke_response.status(), StatusCode::NO_CONTENT);

        let list_after_revoke_response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/v1/workspaces/{workspace_id}/share-links"))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("list share links should return response");
        assert_eq!(list_after_revoke_response.status(), StatusCode::OK);
        let list_after_revoke: ShareLinksEnvelope = read_json(list_after_revoke_response).await;
        assert_eq!(list_after_revoke.items.len(), 1);
        assert!(list_after_revoke.items[0].disabled);
        assert!(list_after_revoke.items[0].revoked_at.is_some());
    }

    #[tokio::test]
    async fn create_list_and_get_workspace_returns_caller_role() {
        let (router, jwt_service, user_id, workspace_id_for_token) = test_router();
        let token = bearer_token(&jwt_service, user_id, workspace_id_for_token);

        let create_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/workspaces")
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&CreateWorkspaceRequest {
                            name: "My Project".to_owned(),
                            slug: "my-project".to_owned(),
                        })
                        .expect("request should serialize"),
                    ))
                    .expect("request should build"),
            )
            .await
            .expect("create request should return response");
        assert_eq!(create_response.status(), StatusCode::CREATED);
        let create_body: WorkspaceEnvelope = read_json(create_response).await;
        assert_eq!(create_body.workspace.role.as_deref(), Some("owner"));
        assert!(!create_body.workspace.etag.is_empty());

        let list_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/workspaces?limit=10")
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("list request should return response");
        assert_eq!(list_response.status(), StatusCode::OK);
        let list_body: WorkspacesPageEnvelope = read_json(list_response).await;
        assert_eq!(list_body.items.len(), 1);
        assert_eq!(list_body.items[0].role.as_deref(), Some("owner"));

        let get_response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/v1/workspaces/{}", create_body.workspace.id))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("get request should return response");
        assert_eq!(get_response.status(), StatusCode::OK);
        let get_body: WorkspaceEnvelope = read_json(get_response).await;
        assert_eq!(get_body.workspace.id, create_body.workspace.id);
        assert_eq!(get_body.workspace.role.as_deref(), Some("owner"));
    }

    #[tokio::test]
    async fn workspace_rest_response_shape_matches_contract() {
        fn sorted_keys(value: &serde_json::Value) -> Vec<String> {
            let mut keys = value
                .as_object()
                .expect("value should be an object")
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            keys.sort();
            keys
        }

        let (router, jwt_service, user_id, workspace_id_for_token) = test_router();
        let token = bearer_token(&jwt_service, user_id, workspace_id_for_token);

        let create_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/workspaces")
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "name": "Contract", "slug": "contract" }).to_string()))
                    .expect("request should build"),
            )
            .await
            .expect("create request should return response");
        assert_eq!(create_response.status(), StatusCode::CREATED);
        let create_json: serde_json::Value = read_json(create_response).await;

        assert_eq!(sorted_keys(&create_json), vec!["workspace".to_string()]);
        let create_workspace = &create_json["workspace"];
        assert_eq!(
            sorted_keys(create_workspace),
            vec![
                "created_at".to_string(),
                "etag".to_string(),
                "id".to_string(),
                "name".to_string(),
                "role".to_string(),
                "slug".to_string(),
                "updated_at".to_string(),
            ]
        );

        let workspace_id =
            create_workspace["id"].as_str().expect("workspace id should be string").to_string();

        let list_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/workspaces?limit=10")
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("list request should return response");
        assert_eq!(list_response.status(), StatusCode::OK);
        let list_json: serde_json::Value = read_json(list_response).await;

        assert_eq!(sorted_keys(&list_json), vec!["items".to_string(), "next_cursor".to_string()]);
        let list_items = list_json["items"].as_array().expect("items should be an array");
        assert_eq!(list_items.len(), 1);
        assert_eq!(sorted_keys(&list_items[0]), sorted_keys(create_workspace));

        let get_response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/v1/workspaces/{workspace_id}"))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("get request should return response");
        assert_eq!(get_response.status(), StatusCode::OK);
        let get_json: serde_json::Value = read_json(get_response).await;
        assert_eq!(sorted_keys(&get_json), vec!["workspace".to_string()]);
        assert_eq!(sorted_keys(&get_json["workspace"]), sorted_keys(create_workspace));
    }

    #[tokio::test]
    async fn list_workspaces_supports_cursor_pagination() {
        let (router, jwt_service, user_id, workspace_id_for_token) = test_router();
        let token = bearer_token(&jwt_service, user_id, workspace_id_for_token);

        for (name, slug) in [("Alpha", "alpha"), ("Beta", "beta")] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method(Method::POST)
                        .uri("/v1/workspaces")
                        .header(AUTHORIZATION, format!("Bearer {token}"))
                        .header("content-type", "application/json")
                        .body(Body::from(json!({ "name": name, "slug": slug }).to_string()))
                        .expect("request should build"),
                )
                .await
                .expect("create request should return response");

            assert_eq!(response.status(), StatusCode::CREATED);
        }

        let first_page_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/workspaces?limit=1")
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("list request should return response");
        assert_eq!(first_page_response.status(), StatusCode::OK);
        let first_page: WorkspacesPageEnvelope = read_json(first_page_response).await;
        assert_eq!(first_page.items.len(), 1);
        assert!(first_page.next_cursor.is_some());

        let second_page_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!(
                        "/v1/workspaces?limit=1&cursor={}",
                        first_page
                            .next_cursor
                            .clone()
                            .expect("first page should include next cursor")
                    ))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("list request should return response");
        assert_eq!(second_page_response.status(), StatusCode::OK);
        let second_page: WorkspacesPageEnvelope = read_json(second_page_response).await;
        assert_eq!(second_page.items.len(), 1);
        assert_ne!(first_page.items[0].id, second_page.items[0].id);
    }

    #[tokio::test]
    async fn patch_workspace_enforces_if_match_header() {
        let (router, jwt_service, user_id, workspace_id_for_token) = test_router();
        let token = bearer_token(&jwt_service, user_id, workspace_id_for_token);

        let create_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/workspaces")
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "name": "Docs", "slug": "docs" }).to_string()))
                    .expect("request should build"),
            )
            .await
            .expect("create request should return response");
        let create_body: WorkspaceEnvelope = read_json(create_response).await;

        let missing_if_match = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri(format!("/v1/workspaces/{}", create_body.workspace.id))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "name": "Docs 2" }).to_string()))
                    .expect("request should build"),
            )
            .await
            .expect("patch request should return response");
        assert_eq!(missing_if_match.status(), StatusCode::PRECONDITION_REQUIRED);

        let stale_if_match = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri(format!("/v1/workspaces/{}", create_body.workspace.id))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("if-match", "\"stale\"")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "name": "Docs 2" }).to_string()))
                    .expect("request should build"),
            )
            .await
            .expect("patch request should return response");
        assert_eq!(stale_if_match.status(), StatusCode::PRECONDITION_FAILED);

        let successful_patch = router
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri(format!("/v1/workspaces/{}", create_body.workspace.id))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("if-match", &create_body.workspace.etag)
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "name": "Docs Updated" }).to_string()))
                    .expect("request should build"),
            )
            .await
            .expect("patch request should return response");
        assert_eq!(successful_patch.status(), StatusCode::OK);
        let updated_body: WorkspaceEnvelope = read_json(successful_patch).await;
        assert_eq!(updated_body.workspace.name, "Docs Updated");
        assert_ne!(updated_body.workspace.etag, create_body.workspace.etag);
    }

    // ── Member management ────────────────────────────────────────────

    fn setup_workspace_with_members() -> (
        axum::Router,
        Arc<JwtAccessTokenService>,
        Uuid,
        Uuid,
        Uuid,
        Arc<RwLock<MemoryWorkspaceStore>>,
    ) {
        let jwt_service = Arc::new(
            JwtAccessTokenService::new(TEST_SECRET).expect("jwt service should initialize"),
        );
        let owner_id = Uuid::new_v4();
        let member_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let now = Utc::now();

        let mut mem_store = MemoryWorkspaceStore::default();
        mem_store.workspaces.insert(
            workspace_id,
            MemoryWorkspace {
                id: workspace_id,
                slug: "team".to_owned(),
                name: "Team".to_owned(),
                created_at: now,
                updated_at: now,
                deleted_at: None,
            },
        );
        mem_store.memberships.insert(
            (workspace_id, owner_id),
            MemoryMembership {
                role: "owner".to_owned(),
                status: "active".to_owned(),
                email: "owner@test.local".to_owned(),
                display_name: "Owner".to_owned(),
                joined_at: now,
            },
        );
        mem_store.memberships.insert(
            (workspace_id, member_id),
            MemoryMembership {
                role: "editor".to_owned(),
                status: "active".to_owned(),
                email: "editor@test.local".to_owned(),
                display_name: "Editor User".to_owned(),
                joined_at: now,
            },
        );

        let store = Arc::new(RwLock::new(mem_store));
        let router = build_router_with_store(
            WorkspaceStore::Memory(store.clone()),
            Arc::clone(&jwt_service),
        );
        (router, jwt_service, owner_id, member_id, workspace_id, store)
    }

    #[tokio::test]
    async fn list_members_returns_workspace_members() {
        let (router, jwt_service, owner_id, _member_id, workspace_id, _store) =
            setup_workspace_with_members();
        let token = bearer_token(&jwt_service, owner_id, workspace_id);

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/v1/workspaces/{workspace_id}/members"))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("list members request should return response");

        assert_eq!(response.status(), StatusCode::OK);
        let body: MembersPageEnvelope = read_json(response).await;
        assert_eq!(body.items.len(), 2);
        assert!(body.items.iter().any(|m| m.role == "owner"));
        assert!(body.items.iter().any(|m| m.role == "editor"));
    }

    #[tokio::test]
    async fn update_member_role_with_if_match() {
        let (router, jwt_service, owner_id, member_id, workspace_id, store) =
            setup_workspace_with_members();
        let token = bearer_token(&jwt_service, owner_id, workspace_id);

        let etag = {
            let guard = store.read().await;
            let member = guard.memberships.get(&(workspace_id, member_id)).unwrap();
            super::member_etag(member_id, member.joined_at)
        };

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri(format!("/v1/workspaces/{workspace_id}/members/{member_id}"))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("if-match", &etag)
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "role": "viewer" }).to_string()))
                    .expect("request should build"),
            )
            .await
            .expect("update member request should return response");

        assert_eq!(response.status(), StatusCode::OK);
        let body: MemberEnvelope = read_json(response).await;
        assert_eq!(body.member.role, "viewer");
        assert_eq!(body.member.user_id, member_id);
    }

    #[tokio::test]
    async fn update_member_rejects_stale_etag() {
        let (router, jwt_service, owner_id, member_id, workspace_id, _store) =
            setup_workspace_with_members();
        let token = bearer_token(&jwt_service, owner_id, workspace_id);

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri(format!("/v1/workspaces/{workspace_id}/members/{member_id}"))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("if-match", "\"stale\"")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "role": "viewer" }).to_string()))
                    .expect("request should build"),
            )
            .await
            .expect("update member request should return response");

        assert_eq!(response.status(), StatusCode::PRECONDITION_FAILED);
    }

    #[tokio::test]
    async fn update_member_rejects_self_modification() {
        let (router, jwt_service, owner_id, _member_id, workspace_id, _store) =
            setup_workspace_with_members();
        let token = bearer_token(&jwt_service, owner_id, workspace_id);

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri(format!("/v1/workspaces/{workspace_id}/members/{owner_id}"))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("if-match", "*")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "role": "editor" }).to_string()))
                    .expect("request should build"),
            )
            .await
            .expect("update member request should return response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn update_member_validates_role_values() {
        let (router, jwt_service, owner_id, member_id, workspace_id, _store) =
            setup_workspace_with_members();
        let token = bearer_token(&jwt_service, owner_id, workspace_id);

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri(format!("/v1/workspaces/{workspace_id}/members/{member_id}"))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("if-match", "*")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "role": "admin" }).to_string()))
                    .expect("request should build"),
            )
            .await
            .expect("update member request should return response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn delete_member_removes_from_workspace() {
        let (router, jwt_service, owner_id, member_id, workspace_id, store) =
            setup_workspace_with_members();
        let token = bearer_token(&jwt_service, owner_id, workspace_id);

        let etag = {
            let guard = store.read().await;
            let member = guard.memberships.get(&(workspace_id, member_id)).unwrap();
            super::member_etag(member_id, member.joined_at)
        };

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/v1/workspaces/{workspace_id}/members/{member_id}"))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("if-match", &etag)
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("delete member request should return response");

        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let guard = store.read().await;
        assert!(guard.memberships.get(&(workspace_id, member_id)).is_none());
    }

    #[tokio::test]
    async fn delete_member_rejects_owner_removal() {
        let (router, jwt_service, owner_id, _member_id, workspace_id, _store) =
            setup_workspace_with_members();
        let token = bearer_token(&jwt_service, owner_id, workspace_id);

        let other_owner_id = Uuid::new_v4();
        // This test tries to delete self, which should fail with self-modification check
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/v1/workspaces/{workspace_id}/members/{owner_id}"))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("if-match", "*")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("delete member request should return response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let _ = other_owner_id;
    }

    #[tokio::test]
    async fn delete_member_rejects_self_removal() {
        let (router, jwt_service, owner_id, _member_id, workspace_id, _store) =
            setup_workspace_with_members();
        let token = bearer_token(&jwt_service, owner_id, workspace_id);

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/v1/workspaces/{workspace_id}/members/{owner_id}"))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("if-match", "*")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("delete member request should return response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn delete_nonexistent_member_returns_not_found() {
        let (router, jwt_service, owner_id, _member_id, workspace_id, _store) =
            setup_workspace_with_members();
        let token = bearer_token(&jwt_service, owner_id, workspace_id);
        let fake_id = Uuid::new_v4();

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/v1/workspaces/{workspace_id}/members/{fake_id}"))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("if-match", "*")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("delete member request should return response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn list_members_requires_auth() {
        let (router, _jwt_service, _owner_id, _member_id, workspace_id, _store) =
            setup_workspace_with_members();

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/v1/workspaces/{workspace_id}/members"))
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("list members request should return response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn update_member_status_to_suspended() {
        let (router, jwt_service, owner_id, member_id, workspace_id, store) =
            setup_workspace_with_members();
        let token = bearer_token(&jwt_service, owner_id, workspace_id);

        let etag = {
            let guard = store.read().await;
            let member = guard.memberships.get(&(workspace_id, member_id)).unwrap();
            super::member_etag(member_id, member.joined_at)
        };

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri(format!("/v1/workspaces/{workspace_id}/members/{member_id}"))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("if-match", &etag)
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "status": "suspended" }).to_string()))
                    .expect("request should build"),
            )
            .await
            .expect("update member request should return response");

        assert_eq!(response.status(), StatusCode::OK);
        let body: MemberEnvelope = read_json(response).await;
        assert_eq!(body.member.status, "suspended");
        assert_eq!(body.member.role, "editor"); // role unchanged
    }

    // ── Invite management ─────────────────────────────────────────────

    #[tokio::test]
    async fn create_invite_returns_invite_envelope() {
        let (router, jwt_service, owner_id, _member_id, workspace_id, _store) =
            setup_workspace_with_members();
        let token = bearer_token(&jwt_service, owner_id, workspace_id);

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/v1/workspaces/{workspace_id}/invites"))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "email": "invitee@example.com",
                            "role": "editor"
                        })
                        .to_string(),
                    ))
                    .expect("request should build"),
            )
            .await
            .expect("create invite request should return response");

        assert_eq!(response.status(), StatusCode::CREATED);
        let body: InviteEnvelope = read_json(response).await;
        assert_eq!(body.invite.email, "invitee@example.com");
        assert_eq!(body.invite.role, "editor");
        assert_eq!(body.invite.workspace_id, workspace_id);
    }

    #[tokio::test]
    async fn create_invite_rejects_owner_role() {
        let (router, jwt_service, owner_id, _member_id, workspace_id, _store) =
            setup_workspace_with_members();
        let token = bearer_token(&jwt_service, owner_id, workspace_id);

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/v1/workspaces/{workspace_id}/invites"))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "email": "invitee@example.com",
                            "role": "owner"
                        })
                        .to_string(),
                    ))
                    .expect("request should build"),
            )
            .await
            .expect("create invite request should return response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn create_invite_rejects_invalid_email() {
        let (router, jwt_service, owner_id, _member_id, workspace_id, _store) =
            setup_workspace_with_members();
        let token = bearer_token(&jwt_service, owner_id, workspace_id);

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/v1/workspaces/{workspace_id}/invites"))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "email": "not-an-email",
                            "role": "editor"
                        })
                        .to_string(),
                    ))
                    .expect("request should build"),
            )
            .await
            .expect("create invite request should return response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn create_invite_requires_owner_role() {
        let jwt_service = Arc::new(
            JwtAccessTokenService::new(TEST_SECRET).expect("jwt service should initialize"),
        );
        let editor_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let now = Utc::now();
        let store = Arc::new(RwLock::new(MemoryWorkspaceStore::default()));

        {
            let mut guard = store.write().await;
            guard.workspaces.insert(
                workspace_id,
                MemoryWorkspace {
                    id: workspace_id,
                    slug: "invite-rbac".to_owned(),
                    name: "Invite RBAC".to_owned(),
                    created_at: now,
                    updated_at: now,
                    deleted_at: None,
                },
            );
            guard.memberships.insert(
                (workspace_id, editor_id),
                MemoryMembership {
                    role: "editor".to_owned(),
                    status: "active".to_owned(),
                    email: "editor@test.local".to_owned(),
                    display_name: "Editor".to_owned(),
                    joined_at: now,
                },
            );
        }

        let router =
            build_router_with_store(WorkspaceStore::Memory(store), Arc::clone(&jwt_service));
        let token = bearer_token(&jwt_service, editor_id, workspace_id);

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/v1/workspaces/{workspace_id}/invites"))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "email": "invitee@example.com",
                            "role": "viewer"
                        })
                        .to_string(),
                    ))
                    .expect("request should build"),
            )
            .await
            .expect("create invite request should return response");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn accept_invite_creates_membership() {
        let (router, jwt_service, owner_id, _member_id, workspace_id, store) =
            setup_workspace_with_members();

        // Manually insert an invite with a known token hash.
        let raw_token = "test-invite-token-abc123";
        let token_hash = Sha256::digest(raw_token.as_bytes()).to_vec();
        let invite_id = Uuid::new_v4();
        let now = Utc::now();
        {
            let mut guard = store.write().await;
            guard.invites.insert(
                invite_id,
                MemoryInvite {
                    id: invite_id,
                    workspace_id,
                    email: "new-user@example.com".to_owned(),
                    role: "viewer".to_owned(),
                    token_hash,
                    expires_at: now + chrono::Duration::hours(72),
                    accepted_at: None,
                    created_at: now,
                },
            );
        }

        let acceptor_id = Uuid::new_v4();
        let token = bearer_token(&jwt_service, acceptor_id, workspace_id);

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/v1/invites/{raw_token}/accept"))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("accept invite request should return response");

        assert_eq!(response.status(), StatusCode::OK);
        let body: InviteEnvelope = read_json(response).await;
        assert_eq!(body.invite.workspace_id, workspace_id);
        assert_eq!(body.invite.role, "viewer");

        // Verify membership was created.
        let guard = store.read().await;
        let membership =
            guard.memberships.get(&(workspace_id, acceptor_id)).expect("membership should exist");
        assert_eq!(membership.role, "viewer");
        assert_eq!(membership.status, "active");
    }

    #[tokio::test]
    async fn accept_expired_invite_fails() {
        let (_router, jwt_service, owner_id, _member_id, workspace_id, store) =
            setup_workspace_with_members();

        let raw_token = "expired-invite-token";
        let token_hash = Sha256::digest(raw_token.as_bytes()).to_vec();
        let invite_id = Uuid::new_v4();
        let now = Utc::now();
        {
            let mut guard = store.write().await;
            guard.invites.insert(
                invite_id,
                MemoryInvite {
                    id: invite_id,
                    workspace_id,
                    email: "expired@example.com".to_owned(),
                    role: "editor".to_owned(),
                    token_hash,
                    expires_at: now - chrono::Duration::hours(1),
                    accepted_at: None,
                    created_at: now - chrono::Duration::hours(73),
                },
            );
        }

        let router =
            build_router_with_store(WorkspaceStore::Memory(store), Arc::clone(&jwt_service));
        let acceptor_id = Uuid::new_v4();
        let token = bearer_token(&jwt_service, acceptor_id, workspace_id);

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/v1/invites/{raw_token}/accept"))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("accept invite request should return response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn accept_already_accepted_invite_fails() {
        let (_router, jwt_service, owner_id, _member_id, workspace_id, store) =
            setup_workspace_with_members();

        let raw_token = "already-accepted-token";
        let token_hash = Sha256::digest(raw_token.as_bytes()).to_vec();
        let invite_id = Uuid::new_v4();
        let now = Utc::now();
        {
            let mut guard = store.write().await;
            guard.invites.insert(
                invite_id,
                MemoryInvite {
                    id: invite_id,
                    workspace_id,
                    email: "accepted@example.com".to_owned(),
                    role: "editor".to_owned(),
                    token_hash,
                    expires_at: now + chrono::Duration::hours(72),
                    accepted_at: Some(now - chrono::Duration::hours(1)),
                    created_at: now - chrono::Duration::hours(24),
                },
            );
        }

        let router =
            build_router_with_store(WorkspaceStore::Memory(store), Arc::clone(&jwt_service));
        let acceptor_id = Uuid::new_v4();
        let token = bearer_token(&jwt_service, acceptor_id, workspace_id);

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/v1/invites/{raw_token}/accept"))
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("accept invite request should return response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn accept_invalid_token_returns_not_found() {
        let (router, jwt_service, _owner_id, _member_id, workspace_id, _store) =
            setup_workspace_with_members();
        let acceptor_id = Uuid::new_v4();
        let token = bearer_token(&jwt_service, acceptor_id, workspace_id);

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/invites/nonexistent-token/accept")
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("accept invite request should return response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    // ── Share link redeem ────────────────────────────────────────────

    fn setup_workspace_with_share_link(
        password_hash: Option<String>,
        max_uses: Option<i32>,
        expires_at: Option<chrono::DateTime<Utc>>,
        disabled: bool,
    ) -> (
        axum::Router,
        Arc<JwtAccessTokenService>,
        Uuid,
        String,
        Uuid,
        Arc<RwLock<MemoryWorkspaceStore>>,
    ) {
        let jwt_service = Arc::new(
            JwtAccessTokenService::new(TEST_SECRET).expect("jwt service should initialize"),
        );
        let user_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let share_link_id = Uuid::new_v4();
        let now = Utc::now();

        let raw_token = "redeem-test-token-abc123";
        let token_hash = Sha256::digest(raw_token.as_bytes()).to_vec();

        let mut mem_store = MemoryWorkspaceStore::default();
        mem_store.workspaces.insert(
            workspace_id,
            MemoryWorkspace {
                id: workspace_id,
                slug: "redeem-ws".to_owned(),
                name: "Redeem WS".to_owned(),
                created_at: now,
                updated_at: now,
                deleted_at: None,
            },
        );
        mem_store.memberships.insert(
            (workspace_id, user_id),
            MemoryMembership {
                role: "owner".to_owned(),
                status: "active".to_owned(),
                email: "owner@test.local".to_owned(),
                display_name: "Owner".to_owned(),
                joined_at: now,
            },
        );
        mem_store.share_links.insert(
            share_link_id,
            MemoryShareLink {
                id: share_link_id,
                workspace_id,
                target_type: "workspace".to_owned(),
                target_id: workspace_id,
                permission: "view".to_owned(),
                token_hash,
                password_hash,
                expires_at,
                max_uses,
                use_count: 0,
                disabled,
                created_at: now,
                revoked_at: None,
            },
        );

        let store = Arc::new(RwLock::new(mem_store));
        let router = build_router_with_store(
            WorkspaceStore::Memory(store.clone()),
            Arc::clone(&jwt_service),
        );
        (router, jwt_service, workspace_id, raw_token.to_owned(), share_link_id, store)
    }

    #[tokio::test]
    async fn redeem_share_link_grants_access_without_auth() {
        let (router, _jwt_service, workspace_id, raw_token, share_link_id, store) =
            setup_workspace_with_share_link(None, Some(10), None, false);

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/share-links/redeem")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "token": raw_token }).to_string()))
                    .expect("request should build"),
            )
            .await
            .expect("redeem request should return response");

        assert_eq!(response.status(), StatusCode::OK);
        let body: RedeemShareLinkResponse = read_json(response).await;
        assert_eq!(body.workspace_id, workspace_id);
        assert_eq!(body.target_type, "workspace");
        assert_eq!(body.target_id, workspace_id);
        assert_eq!(body.permission, "view");
        assert_eq!(body.remaining_uses, Some(9));

        // Verify use_count was incremented.
        let guard = store.read().await;
        let link = guard.share_links.get(&share_link_id).unwrap();
        assert_eq!(link.use_count, 1);
    }

    #[tokio::test]
    async fn redeem_share_link_rejects_invalid_token() {
        let (router, _jwt_service, _workspace_id, _raw_token, _share_link_id, _store) =
            setup_workspace_with_share_link(None, None, None, false);

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/share-links/redeem")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "token": "nonexistent-token" }).to_string()))
                    .expect("request should build"),
            )
            .await
            .expect("redeem request should return response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn redeem_share_link_rejects_disabled_link() {
        let (router, _jwt_service, _workspace_id, raw_token, _share_link_id, _store) =
            setup_workspace_with_share_link(None, None, None, true);

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/share-links/redeem")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "token": raw_token }).to_string()))
                    .expect("request should build"),
            )
            .await
            .expect("redeem request should return response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn redeem_share_link_rejects_expired_link() {
        let expired = Utc::now() - chrono::Duration::hours(1);
        let (router, _jwt_service, _workspace_id, raw_token, _share_link_id, _store) =
            setup_workspace_with_share_link(None, None, Some(expired), false);

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/share-links/redeem")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "token": raw_token }).to_string()))
                    .expect("request should build"),
            )
            .await
            .expect("redeem request should return response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn redeem_share_link_rejects_exhausted_uses() {
        let (router, _jwt_service, _workspace_id, raw_token, share_link_id, store) =
            setup_workspace_with_share_link(None, Some(1), None, false);

        // Set use_count to max_uses so it's exhausted.
        {
            let mut guard = store.write().await;
            guard.share_links.get_mut(&share_link_id).unwrap().use_count = 1;
        }

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/share-links/redeem")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "token": raw_token }).to_string()))
                    .expect("request should build"),
            )
            .await
            .expect("redeem request should return response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn redeem_share_link_validates_password() {
        let password = "super-secret";
        let password_hash = super::hash_share_link_password(password).unwrap();
        let (router, _jwt_service, workspace_id, raw_token, _share_link_id, _store) =
            setup_workspace_with_share_link(Some(password_hash), None, None, false);

        // Wrong password.
        let bad_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/share-links/redeem")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({ "token": raw_token, "password": "wrong" }).to_string(),
                    ))
                    .expect("request should build"),
            )
            .await
            .expect("redeem request should return response");
        assert_eq!(bad_response.status(), StatusCode::BAD_REQUEST);

        // Correct password.
        let good_response = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/share-links/redeem")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({ "token": raw_token, "password": password }).to_string(),
                    ))
                    .expect("request should build"),
            )
            .await
            .expect("redeem request should return response");
        assert_eq!(good_response.status(), StatusCode::OK);
        let body: RedeemShareLinkResponse = read_json(good_response).await;
        assert_eq!(body.workspace_id, workspace_id);
    }

    #[tokio::test]
    async fn redeem_share_link_rate_limits_after_max_attempts() {
        let (router, _jwt_service, _workspace_id, _raw_token, _share_link_id, _store) =
            setup_workspace_with_share_link(None, None, None, false);

        // Send REDEEM_RATE_LIMIT_MAX + 1 requests with a bad token to trigger rate limit.
        let bad_token = "rate-limit-test-token";
        for i in 0..=super::REDEEM_RATE_LIMIT_MAX {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method(Method::POST)
                        .uri("/v1/share-links/redeem")
                        .header("content-type", "application/json")
                        .body(Body::from(json!({ "token": bad_token }).to_string()))
                        .expect("request should build"),
                )
                .await
                .expect("redeem request should return response");

            if i < super::REDEEM_RATE_LIMIT_MAX {
                // First N requests fail with NOT_FOUND (bad token).
                assert_eq!(response.status(), StatusCode::NOT_FOUND, "attempt {i}");
            } else {
                // The N+1 request should be rate limited.
                assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS, "attempt {i}");
            }
        }
    }

    #[tokio::test]
    async fn redeem_share_link_rejects_empty_token() {
        let (router, _jwt_service, _workspace_id, _raw_token, _share_link_id, _store) =
            setup_workspace_with_share_link(None, None, None, false);

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/share-links/redeem")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "token": "  " }).to_string()))
                    .expect("request should build"),
            )
            .await
            .expect("redeem request should return response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
