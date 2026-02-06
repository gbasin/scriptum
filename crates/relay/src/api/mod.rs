use std::{collections::HashMap, env, sync::Arc};

use anyhow::{Context, Result};
use axum::{
    extract::{Extension, Json, Path, Query, State},
    http::{header::IF_MATCH, HeaderMap, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use scriptum_common::types::Workspace;
use serde::{Deserialize, Serialize};
use sqlx::{
    types::chrono::{DateTime, Utc},
    PgPool,
};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{
    auth::{
        jwt::JwtAccessTokenService,
        middleware::{require_bearer_auth, AuthenticatedUser},
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

#[derive(Clone)]
struct WorkspaceCursor {
    created_at: DateTime<Utc>,
    id: Uuid,
}

struct WorkspacePage {
    items: Vec<WorkspaceRecord>,
    next_cursor: Option<String>,
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

    Ok(build_router_with_store(WorkspaceStore::Postgres(pool), jwt_service))
}

fn build_router_with_store(
    store: WorkspaceStore,
    jwt_service: Arc<JwtAccessTokenService>,
) -> Router {
    let state = ApiState { store };

    Router::new()
        .route("/v1/workspaces", post(create_workspace).get(list_workspaces))
        .route("/v1/workspaces/{id}", get(get_workspace).patch(update_workspace))
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
        MemoryMembership { role: "owner".to_owned(), status: "active".to_owned() },
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
        build_router_with_store, CreateWorkspaceRequest, WorkspaceEnvelope, WorkspaceStore,
        WorkspacesPageEnvelope,
    };
    use crate::auth::jwt::JwtAccessTokenService;
    use axum::{
        body::{to_bytes, Body},
        http::{header::AUTHORIZATION, Method, Request, StatusCode},
    };
    use serde_json::json;
    use std::sync::Arc;
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
}
