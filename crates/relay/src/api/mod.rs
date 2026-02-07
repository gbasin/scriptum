pub mod comments;
pub mod documents;

use std::{collections::HashMap, env, sync::Arc};

use anyhow::{Context, Result};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Argon2,
};
use axum::{
    extract::{Extension, FromRequestParts, Json, Path, Query, Request, State},
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
    },
    db::pool::{check_pool_health, create_pg_pool, PoolConfig},
    error::{ErrorCode, RelayError},
};

const DEFAULT_PAGE_SIZE: usize = 50;
const MAX_PAGE_SIZE: usize = 100;

#[derive(Clone)]
struct ApiState {
    store: WorkspaceStore,
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
    fn into_share_link(self, url_once: String) -> ShareLink {
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
            Self::Internal(error) => {
                tracing::error!(error = ?error, "workspace api internal error");
                RelayError::from_code(ErrorCode::InternalError).into_response()
            }
        }
    }
}

pub async fn build_router_from_env(jwt_service: Arc<JwtAccessTokenService>) -> Result<Router> {
    let database_url = env::var("SCRIPTUM_RELAY_DATABASE_URL")
        .context("SCRIPTUM_RELAY_DATABASE_URL must be set for workspace API")?;

    let pool = create_pg_pool(&database_url, PoolConfig::from_env())
        .await
        .context("failed to initialize relay PostgreSQL pool for workspace API")?;
    check_pool_health(&pool)
        .await
        .context("relay PostgreSQL health check failed for workspace API")?;

    Ok(build_router_with_store(WorkspaceStore::Postgres(pool.clone()), Arc::clone(&jwt_service))
        .merge(comments::router(pool, jwt_service)))
}

fn build_router_with_store(
    store: WorkspaceStore,
    jwt_service: Arc<JwtAccessTokenService>,
) -> Router {
    let state = ApiState { store };
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
        .route("/v1/workspaces", post(create_workspace).get(list_workspaces))
        .route("/v1/workspaces/{id}", get(get_workspace).route_layer(viewer_role_layer))
        .route("/v1/workspaces/{id}", patch(update_workspace).route_layer(owner_role_layer))
        .route(
            "/v1/workspaces/{id}/share-links",
            post(create_share_link).route_layer(editor_role_layer),
        )
        .route(
            "/v1/workspaces/{workspace_id}/members",
            get(list_members).route_layer(members_viewer_layer),
        )
        .route(
            "/v1/workspaces/{workspace_id}/members/{member_id}",
            patch(update_member).route_layer(members_owner_layer),
        )
        .route(
            "/v1/workspaces/{workspace_id}/members/{member_id}",
            delete(delete_member).route_layer(members_delete_owner_layer),
        )
        .with_state(state)
        .route_layer(middleware::from_fn_with_state(jwt_service, require_bearer_auth))
}

async fn create_workspace(
    State(state): State<ApiState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(payload): Json<CreateWorkspaceRequest>,
) -> Result<(StatusCode, Json<WorkspaceEnvelope>), ApiError> {
    validate_name(&payload.name)?;
    validate_slug(&payload.slug)?;

    let workspace = state
        .store
        .create_workspace(user.user_id, payload.name, payload.slug)
        .await?
        .into_workspace();

    Ok((StatusCode::CREATED, Json(WorkspaceEnvelope { workspace })))
}

async fn list_workspaces(
    State(state): State<ApiState>,
    Extension(user): Extension<AuthenticatedUser>,
    Query(query): Query<ListWorkspacesQuery>,
) -> Result<Json<WorkspacesPageEnvelope>, ApiError> {
    let limit = normalize_limit(query.limit);
    let cursor = match query.cursor {
        Some(raw_cursor) => Some(parse_cursor(&raw_cursor)?),
        None => None,
    };

    let page = state.store.list_workspaces(user.user_id, limit, cursor).await?;

    Ok(Json(WorkspacesPageEnvelope {
        items: page.items.into_iter().map(WorkspaceRecord::into_workspace).collect(),
        next_cursor: page.next_cursor,
    }))
}

async fn get_workspace(
    State(state): State<ApiState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<WorkspaceEnvelope>, ApiError> {
    let workspace = state.store.get_workspace(user.user_id, workspace_id).await?.into_workspace();

    Ok(Json(WorkspaceEnvelope { workspace }))
}

async fn update_workspace(
    State(state): State<ApiState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(workspace_id): Path<Uuid>,
    headers: HeaderMap,
    Json(payload): Json<UpdateWorkspaceRequest>,
) -> Result<Json<WorkspaceEnvelope>, ApiError> {
    if let Some(name) = payload.name.as_deref() {
        validate_name(name)?;
    }
    if let Some(slug) = payload.slug.as_deref() {
        validate_slug(slug)?;
    }

    let if_match = extract_if_match(&headers)?;
    let workspace = state
        .store
        .update_workspace(user.user_id, workspace_id, if_match, payload)
        .await?
        .into_workspace();

    Ok(Json(WorkspaceEnvelope { workspace }))
}

async fn create_share_link(
    State(state): State<ApiState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(workspace_id): Path<Uuid>,
    Json(mut payload): Json<CreateShareLinkRequest>,
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

async fn list_members(
    State(state): State<ApiState>,
    Path(workspace_id): Path<Uuid>,
    Query(query): Query<ListMembersQuery>,
) -> Result<Json<MembersPageEnvelope>, ApiError> {
    let limit = normalize_limit(query.limit);
    let cursor = match query.cursor {
        Some(raw_cursor) => Some(parse_member_cursor(&raw_cursor)?),
        None => None,
    };

    let page = state.store.list_members(workspace_id, limit, cursor).await?;

    Ok(Json(MembersPageEnvelope {
        items: page.items.into_iter().map(MemberRecord::into_member).collect(),
        next_cursor: page.next_cursor,
    }))
}

async fn update_member(
    State(state): State<ApiState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((workspace_id, member_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(payload): Json<UpdateMemberRequest>,
) -> Result<Json<MemberEnvelope>, ApiError> {
    validate_member_update(&payload)?;

    if member_id == user.user_id {
        return Err(ApiError::bad_request("VALIDATION_ERROR", "cannot modify your own membership"));
    }

    let if_match = extract_if_match(&headers)?;
    let record = state.store.update_member(workspace_id, member_id, if_match, payload).await?;

    Ok(Json(MemberEnvelope { member: record.into_member() }))
}

async fn delete_member(
    State(state): State<ApiState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((workspace_id, member_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    if member_id == user.user_id {
        return Err(ApiError::bad_request("VALIDATION_ERROR", "cannot remove your own membership"));
    }

    let if_match = extract_if_match(&headers)?;
    state.store.delete_member(workspace_id, member_id, if_match).await?;

    Ok(StatusCode::NO_CONTENT)
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
        return Err(ApiError::bad_request(
            "VALIDATION_ERROR",
            "cannot demote the last owner",
        ));
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
        return Err(ApiError::bad_request(
            "VALIDATION_ERROR",
            "cannot remove a workspace owner",
        ));
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
        return Err(ApiError::bad_request(
            "VALIDATION_ERROR",
            "cannot demote the last owner",
        ));
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
        return Err(ApiError::bad_request(
            "VALIDATION_ERROR",
            "cannot remove a workspace owner",
        ));
    }

    state.memberships.remove(&(workspace_id, member_id));
    Ok(())
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
        items
            .last()
            .map(|record| encode_member_cursor(record.joined_at, record.user_id))
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
        build_router_with_store, CreateWorkspaceRequest, MemberEnvelope, MembersPageEnvelope,
        MemoryMembership, MemoryWorkspace, MemoryWorkspaceStore, ShareLinkEnvelope,
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
                    .uri(format!(
                        "/v1/workspaces/{workspace_id}/members/{member_id}"
                    ))
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
                    .uri(format!(
                        "/v1/workspaces/{workspace_id}/members/{member_id}"
                    ))
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
                    .uri(format!(
                        "/v1/workspaces/{workspace_id}/members/{owner_id}"
                    ))
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
                    .uri(format!(
                        "/v1/workspaces/{workspace_id}/members/{member_id}"
                    ))
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
                    .uri(format!(
                        "/v1/workspaces/{workspace_id}/members/{member_id}"
                    ))
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
                    .uri(format!(
                        "/v1/workspaces/{workspace_id}/members/{owner_id}"
                    ))
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
                    .uri(format!(
                        "/v1/workspaces/{workspace_id}/members/{owner_id}"
                    ))
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
                    .uri(format!(
                        "/v1/workspaces/{workspace_id}/members/{fake_id}"
                    ))
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
                    .uri(format!(
                        "/v1/workspaces/{workspace_id}/members/{member_id}"
                    ))
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
}
