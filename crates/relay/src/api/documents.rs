// Document CRUD endpoints for the relay API.
//
// Routes:
//   GET    /v1/workspaces/{ws_id}/documents          — list (filters, pagination)
//   POST   /v1/workspaces/{ws_id}/documents          — create
//   GET    /v1/workspaces/{ws_id}/documents/{doc_id}  — get single
//   PATCH  /v1/workspaces/{ws_id}/documents/{doc_id}  — update (If-Match)
//   DELETE /v1/workspaces/{ws_id}/documents/{doc_id}  — soft/hard delete

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::{
    extract::{Extension, Json, Path, Query, State},
    http::{header::IF_MATCH, HeaderMap, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Router,
};
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
    error::{ErrorCode, RelayError},
    validation::ValidatedJson,
};

// ── Types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Document {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub path: String,
    pub title: Option<String>,
    pub head_seq: i64,
    pub etag: String,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

#[derive(sqlx::FromRow)]
struct DocumentRow {
    id: Uuid,
    workspace_id: Uuid,
    path: String,
    title: Option<String>,
    head_seq: i64,
    etag: String,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    archived_at: Option<DateTime<Utc>>,
}

impl From<DocumentRow> for Document {
    fn from(row: DocumentRow) -> Self {
        Self {
            id: row.id,
            workspace_id: row.workspace_id,
            path: row.path,
            title: row.title,
            head_seq: row.head_seq,
            etag: row.etag,
            created_by: row.created_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
            archived_at: row.archived_at,
        }
    }
}

// ── Request / Response types ───────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateDocumentRequest {
    pub path: String,
    pub title: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateDocumentRequest {
    pub path: Option<String>,
    pub title: Option<String>,
    pub archived: Option<bool>,
}

#[derive(Deserialize)]
pub struct ListDocumentsQuery {
    pub path_prefix: Option<String>,
    pub archived: Option<bool>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

#[derive(Deserialize)]
pub struct DeleteDocumentQuery {
    pub hard: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentTagOp {
    Add,
    Remove,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateDocumentTagsRequest {
    pub op: DocumentTagOp,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AclOverride {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub doc_id: Uuid,
    pub subject_type: String,
    pub subject_id: String,
    pub role: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct AclOverrideRow {
    id: Uuid,
    workspace_id: Uuid,
    doc_id: Uuid,
    subject_type: String,
    subject_id: String,
    role: String,
    expires_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl From<AclOverrideRow> for AclOverride {
    fn from(row: AclOverrideRow) -> Self {
        Self {
            id: row.id,
            workspace_id: row.workspace_id,
            doc_id: row.doc_id,
            subject_type: row.subject_type,
            subject_id: row.subject_id,
            role: row.role,
            expires_at: row.expires_at,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateAclOverrideRequest {
    pub subject_type: String,
    pub subject_id: String,
    pub role: String,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
struct DocumentEnvelope {
    document: Document,
}

#[derive(Serialize)]
struct AclOverrideEnvelope {
    acl_override: AclOverride,
}

#[derive(Serialize)]
struct DocumentsPageEnvelope {
    items: Vec<Document>,
    next_cursor: Option<String>,
}

// ── State & Store ──────────────────────────────────────────────────

#[derive(Clone)]
struct DocApiState {
    store: DocumentStore,
}

#[derive(Clone)]
enum DocumentStore {
    Postgres(PgPool),
    #[cfg_attr(not(test), allow(dead_code))]
    Memory(Arc<RwLock<MemoryDocumentStore>>),
}

#[derive(Default)]
struct MemoryDocumentStore {
    documents: HashMap<Uuid, MemoryDocument>,
    acl_overrides: HashMap<Uuid, MemoryAclOverride>,
    workspace_owners: HashSet<(Uuid, Uuid)>,
}

#[derive(Clone)]
struct MemoryDocument {
    id: Uuid,
    workspace_id: Uuid,
    path: String,
    path_norm: String,
    title: Option<String>,
    head_seq: i64,
    etag: String,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    archived_at: Option<DateTime<Utc>>,
    deleted_at: Option<DateTime<Utc>>,
    tags: HashSet<String>,
}

#[derive(Clone)]
struct MemoryAclOverride {
    id: Uuid,
    workspace_id: Uuid,
    doc_id: Uuid,
    subject_type: String,
    subject_id: String,
    role: String,
    expires_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

// ── Error ──────────────────────────────────────────────────────────

#[derive(Debug)]
enum DocApiError {
    BadRequest { message: String },
    Forbidden,
    NotFound,
    Conflict,
    PreconditionRequired,
    PreconditionFailed,
    Internal(anyhow::Error),
}

impl DocApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest { message: message.into() }
    }

    fn internal(error: anyhow::Error) -> Self {
        Self::Internal(error)
    }
}

impl IntoResponse for DocApiError {
    fn into_response(self) -> Response {
        match self {
            Self::BadRequest { message } => {
                RelayError::new(ErrorCode::ValidationFailed, message).into_response()
            }
            Self::Forbidden => {
                RelayError::new(ErrorCode::AuthForbidden, "caller lacks required role")
                    .into_response()
            }
            Self::NotFound => {
                RelayError::new(ErrorCode::NotFound, "document not found").into_response()
            }
            Self::Conflict => {
                RelayError::new(ErrorCode::DocPathConflict, "document path already exists")
                    .into_response()
            }
            Self::PreconditionRequired => {
                RelayError::new(ErrorCode::PreconditionRequired, "missing If-Match header")
                    .into_response()
            }
            Self::PreconditionFailed => RelayError::new(
                ErrorCode::EditPreconditionFailed,
                "If-Match does not match current document state",
            )
            .into_response(),
            Self::Internal(error) => {
                tracing::error!(error = ?error, "document api internal error");
                RelayError::from_code(ErrorCode::InternalError).into_response()
            }
        }
    }
}

// ── Router ─────────────────────────────────────────────────────────

pub fn router(pool: PgPool, jwt_service: Arc<JwtAccessTokenService>) -> Router {
    build_router_with_store(DocumentStore::Postgres(pool), jwt_service)
}

fn build_router_with_store(
    store: DocumentStore,
    jwt_service: Arc<JwtAccessTokenService>,
) -> Router {
    let state = DocApiState { store };

    Router::new()
        .route("/v1/workspaces/{ws_id}/documents", post(create_document).get(list_documents))
        .route(
            "/v1/workspaces/{ws_id}/documents/{doc_id}",
            get(get_document).patch(update_document).delete(delete_document),
        )
        .route("/v1/workspaces/{ws_id}/documents/{doc_id}/tags", post(update_document_tags))
        .route("/v1/workspaces/{ws_id}/documents/{doc_id}/acl-overrides", post(create_acl_override))
        .route(
            "/v1/workspaces/{ws_id}/documents/{doc_id}/acl-overrides/{override_id}",
            delete(delete_acl_override),
        )
        .with_state(state)
        .route_layer(middleware::from_fn_with_state(jwt_service, require_bearer_auth))
}

// ── Handlers ───────────────────────────────────────────────────────

async fn create_document(
    State(state): State<DocApiState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(ws_id): Path<Uuid>,
    ValidatedJson(payload): ValidatedJson<CreateDocumentRequest>,
) -> Result<(StatusCode, Json<DocumentEnvelope>), DocApiError> {
    validate_path(&payload.path)?;

    let document =
        state.store.create(ws_id, user.user_id, &payload.path, payload.title.as_deref()).await?;

    Ok((StatusCode::CREATED, Json(DocumentEnvelope { document })))
}

async fn list_documents(
    State(state): State<DocApiState>,
    Extension(_user): Extension<AuthenticatedUser>,
    Path(ws_id): Path<Uuid>,
    Query(query): Query<ListDocumentsQuery>,
) -> Result<Json<DocumentsPageEnvelope>, DocApiError> {
    let limit = normalize_limit(query.limit);
    let cursor = match query.cursor {
        Some(raw) => Some(parse_cursor(&raw)?),
        None => None,
    };

    let (items, next_cursor) = state
        .store
        .list(ws_id, query.path_prefix.as_deref(), query.archived, limit, cursor)
        .await?;

    Ok(Json(DocumentsPageEnvelope { items, next_cursor }))
}

async fn get_document(
    State(state): State<DocApiState>,
    Extension(_user): Extension<AuthenticatedUser>,
    Path((ws_id, doc_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<DocumentEnvelope>, DocApiError> {
    let document = state.store.get(ws_id, doc_id).await?;
    Ok(Json(DocumentEnvelope { document }))
}

async fn update_document(
    State(state): State<DocApiState>,
    Extension(_user): Extension<AuthenticatedUser>,
    Path((ws_id, doc_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    ValidatedJson(payload): ValidatedJson<UpdateDocumentRequest>,
) -> Result<Json<DocumentEnvelope>, DocApiError> {
    if let Some(path) = payload.path.as_deref() {
        validate_path(path)?;
    }

    let if_match = extract_if_match(&headers)?;
    let document = state.store.update(ws_id, doc_id, if_match, &payload).await?;

    Ok(Json(DocumentEnvelope { document }))
}

async fn delete_document(
    State(state): State<DocApiState>,
    Extension(_user): Extension<AuthenticatedUser>,
    Path((ws_id, doc_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<DeleteDocumentQuery>,
) -> Result<StatusCode, DocApiError> {
    let hard = query.hard.unwrap_or(false);
    state.store.delete(ws_id, doc_id, hard).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn update_document_tags(
    State(state): State<DocApiState>,
    Extension(_user): Extension<AuthenticatedUser>,
    Path((ws_id, doc_id)): Path<(Uuid, Uuid)>,
    ValidatedJson(payload): ValidatedJson<UpdateDocumentTagsRequest>,
) -> Result<Json<DocumentEnvelope>, DocApiError> {
    let tags = normalize_tag_names(&payload.tags)?;
    let document = state.store.update_tags(ws_id, doc_id, payload.op, &tags).await?;
    Ok(Json(DocumentEnvelope { document }))
}

async fn create_acl_override(
    State(state): State<DocApiState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((ws_id, doc_id)): Path<(Uuid, Uuid)>,
    ValidatedJson(payload): ValidatedJson<CreateAclOverrideRequest>,
) -> Result<(StatusCode, Json<AclOverrideEnvelope>), DocApiError> {
    if ws_id != user.workspace_id {
        return Err(DocApiError::Forbidden);
    }
    ensure_acl_override_manager_access(&state.store, ws_id, doc_id, user.user_id).await?;

    validate_acl_override_request(&payload)?;

    let acl_override = state.store.create_acl_override(ws_id, doc_id, payload).await?;
    Ok((StatusCode::CREATED, Json(AclOverrideEnvelope { acl_override })))
}

async fn delete_acl_override(
    State(state): State<DocApiState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((ws_id, doc_id, override_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, DocApiError> {
    if ws_id != user.workspace_id {
        return Err(DocApiError::Forbidden);
    }
    ensure_acl_override_manager_access(&state.store, ws_id, doc_id, user.user_id).await?;

    state.store.delete_acl_override(ws_id, doc_id, override_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Store implementation ───────────────────────────────────────────

impl DocumentStore {
    async fn create(
        &self,
        workspace_id: Uuid,
        created_by: Uuid,
        path: &str,
        title: Option<&str>,
    ) -> Result<Document, DocApiError> {
        match self {
            Self::Postgres(pool) => create_pg(pool, workspace_id, created_by, path, title).await,
            Self::Memory(store) => create_mem(store, workspace_id, created_by, path, title).await,
        }
    }

    async fn list(
        &self,
        workspace_id: Uuid,
        path_prefix: Option<&str>,
        archived: Option<bool>,
        limit: usize,
        cursor: Option<DocCursor>,
    ) -> Result<(Vec<Document>, Option<String>), DocApiError> {
        match self {
            Self::Postgres(pool) => {
                list_pg(pool, workspace_id, path_prefix, archived, limit, cursor).await
            }
            Self::Memory(store) => {
                list_mem(store, workspace_id, path_prefix, archived, limit, cursor).await
            }
        }
    }

    async fn get(&self, workspace_id: Uuid, doc_id: Uuid) -> Result<Document, DocApiError> {
        match self {
            Self::Postgres(pool) => get_pg(pool, workspace_id, doc_id).await,
            Self::Memory(store) => get_mem(store, workspace_id, doc_id).await,
        }
    }

    async fn update(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        if_match: &str,
        payload: &UpdateDocumentRequest,
    ) -> Result<Document, DocApiError> {
        match self {
            Self::Postgres(pool) => update_pg(pool, workspace_id, doc_id, if_match, payload).await,
            Self::Memory(store) => update_mem(store, workspace_id, doc_id, if_match, payload).await,
        }
    }

    async fn delete(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        hard: bool,
    ) -> Result<(), DocApiError> {
        match self {
            Self::Postgres(pool) => delete_pg(pool, workspace_id, doc_id, hard).await,
            Self::Memory(store) => delete_mem(store, workspace_id, doc_id, hard).await,
        }
    }

    async fn update_tags(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        op: DocumentTagOp,
        tags: &[String],
    ) -> Result<Document, DocApiError> {
        match self {
            Self::Postgres(pool) => update_tags_pg(pool, workspace_id, doc_id, op, tags).await,
            Self::Memory(store) => update_tags_mem(store, workspace_id, doc_id, op, tags).await,
        }
    }

    async fn create_acl_override(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        payload: CreateAclOverrideRequest,
    ) -> Result<AclOverride, DocApiError> {
        match self {
            Self::Postgres(pool) => {
                create_acl_override_pg(pool, workspace_id, doc_id, payload).await
            }
            Self::Memory(store) => {
                create_acl_override_mem(store, workspace_id, doc_id, payload).await
            }
        }
    }

    async fn delete_acl_override(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        override_id: Uuid,
    ) -> Result<(), DocApiError> {
        match self {
            Self::Postgres(pool) => {
                delete_acl_override_pg(pool, workspace_id, doc_id, override_id).await
            }
            Self::Memory(store) => {
                delete_acl_override_mem(store, workspace_id, doc_id, override_id).await
            }
        }
    }
}

// ── Postgres implementations ───────────────────────────────────────

async fn create_pg(
    pool: &PgPool,
    workspace_id: Uuid,
    created_by: Uuid,
    path: &str,
    title: Option<&str>,
) -> Result<Document, DocApiError> {
    let path_norm = normalize_doc_path(path);
    let etag = generate_etag();

    let row = sqlx::query_as::<_, DocumentRow>(
        r#"
        INSERT INTO documents (workspace_id, path, path_norm, title, etag, created_by)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, workspace_id, path, title, head_seq, etag, created_by,
                  created_at, updated_at, archived_at
        "#,
    )
    .bind(workspace_id)
    .bind(path)
    .bind(&path_norm)
    .bind(title)
    .bind(&etag)
    .bind(created_by)
    .fetch_one(pool)
    .await
    .map_err(map_sqlx_error)?;

    Ok(row.into())
}

async fn list_pg(
    pool: &PgPool,
    workspace_id: Uuid,
    path_prefix: Option<&str>,
    archived: Option<bool>,
    limit: usize,
    cursor: Option<DocCursor>,
) -> Result<(Vec<Document>, Option<String>), DocApiError> {
    let cursor_updated_at = cursor.as_ref().map(|c| c.updated_at);
    let cursor_id = cursor.as_ref().map(|c| c.id);
    let path_prefix_pattern = path_prefix.map(|p| format!("{}%", normalize_doc_path(p)));

    let rows = sqlx::query_as::<_, DocumentRow>(
        r#"
        SELECT id, workspace_id, path, title, head_seq, etag, created_by,
               created_at, updated_at, archived_at
        FROM documents
        WHERE workspace_id = $1
          AND deleted_at IS NULL
          AND ($3::text IS NULL OR path_norm LIKE $3)
          AND (
            $4::bool IS NULL
            OR ($4 = true AND archived_at IS NOT NULL)
            OR ($4 = false AND archived_at IS NULL)
          )
          AND (
            $5::timestamptz IS NULL
            OR updated_at < $5
            OR (updated_at = $5 AND id < $6)
          )
        ORDER BY updated_at DESC, id DESC
        LIMIT $2
        "#,
    )
    .bind(workspace_id)
    .bind((limit + 1) as i64)
    .bind(&path_prefix_pattern)
    .bind(archived)
    .bind(cursor_updated_at)
    .bind(cursor_id)
    .fetch_all(pool)
    .await
    .map_err(map_sqlx_error)?;

    let mut items: Vec<Document> = rows.into_iter().map(Document::from).collect();
    let next_cursor = paginate(&mut items, limit);

    Ok((items, next_cursor))
}

async fn get_pg(pool: &PgPool, workspace_id: Uuid, doc_id: Uuid) -> Result<Document, DocApiError> {
    let row = sqlx::query_as::<_, DocumentRow>(
        r#"
        SELECT id, workspace_id, path, title, head_seq, etag, created_by,
               created_at, updated_at, archived_at
        FROM documents
        WHERE id = $1 AND workspace_id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(doc_id)
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?
    .ok_or(DocApiError::NotFound)?;

    Ok(row.into())
}

async fn update_pg(
    pool: &PgPool,
    workspace_id: Uuid,
    doc_id: Uuid,
    if_match: &str,
    payload: &UpdateDocumentRequest,
) -> Result<Document, DocApiError> {
    // Fetch current to compare etag.
    let current = get_pg(pool, workspace_id, doc_id).await?;
    if !etag_matches(if_match, &current.etag) {
        return Err(DocApiError::PreconditionFailed);
    }

    let new_path = payload.path.as_deref();
    let new_path_norm = new_path.map(normalize_doc_path);
    let new_title = payload.title.as_deref();
    let new_etag = generate_etag();

    let archived_at = match payload.archived {
        Some(true) if current.archived_at.is_none() => Some(Some(Utc::now())),
        Some(false) if current.archived_at.is_some() => Some(None),
        _ => None,
    };

    let row = sqlx::query_as::<_, DocumentRow>(
        r#"
        UPDATE documents
        SET path = COALESCE($3, path),
            path_norm = COALESCE($4, path_norm),
            title = COALESCE($5, title),
            etag = $6,
            archived_at = CASE
                WHEN $7::bool THEN $8::timestamptz
                ELSE archived_at
            END,
            updated_at = now()
        WHERE id = $1 AND workspace_id = $2 AND deleted_at IS NULL
        RETURNING id, workspace_id, path, title, head_seq, etag, created_by,
                  created_at, updated_at, archived_at
        "#,
    )
    .bind(doc_id)
    .bind(workspace_id)
    .bind(new_path)
    .bind(new_path_norm.as_deref())
    .bind(new_title)
    .bind(&new_etag)
    .bind(archived_at.is_some())
    .bind(archived_at.flatten())
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?
    .ok_or(DocApiError::NotFound)?;

    Ok(row.into())
}

async fn delete_pg(
    pool: &PgPool,
    workspace_id: Uuid,
    doc_id: Uuid,
    hard: bool,
) -> Result<(), DocApiError> {
    let affected = if hard {
        sqlx::query("DELETE FROM documents WHERE id = $1 AND workspace_id = $2")
            .bind(doc_id)
            .bind(workspace_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_error)?
            .rows_affected()
    } else {
        sqlx::query(
            "UPDATE documents SET deleted_at = now() WHERE id = $1 AND workspace_id = $2 AND deleted_at IS NULL",
        )
        .bind(doc_id)
        .bind(workspace_id)
        .execute(pool)
        .await
        .map_err(map_sqlx_error)?
        .rows_affected()
    };

    if affected == 0 {
        return Err(DocApiError::NotFound);
    }

    Ok(())
}

async fn update_tags_pg(
    pool: &PgPool,
    workspace_id: Uuid,
    doc_id: Uuid,
    op: DocumentTagOp,
    tags: &[String],
) -> Result<Document, DocApiError> {
    let mut tx = pool.begin().await.map_err(map_sqlx_error)?;

    let exists = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT 1
        FROM documents
        WHERE id = $1 AND workspace_id = $2 AND deleted_at IS NULL
        FOR UPDATE
        "#,
    )
    .bind(doc_id)
    .bind(workspace_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .is_some();

    if !exists {
        return Err(DocApiError::NotFound);
    }

    match op {
        DocumentTagOp::Add => {
            for tag in tags {
                let tag_id = sqlx::query_scalar::<_, Uuid>(
                    r#"
                    INSERT INTO tags (workspace_id, name)
                    VALUES ($1, $2)
                    ON CONFLICT (workspace_id, name) DO UPDATE SET name = EXCLUDED.name
                    RETURNING id
                    "#,
                )
                .bind(workspace_id)
                .bind(tag)
                .fetch_one(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;

                sqlx::query(
                    r#"
                    INSERT INTO document_tags (document_id, tag_id)
                    VALUES ($1, $2)
                    ON CONFLICT (document_id, tag_id) DO NOTHING
                    "#,
                )
                .bind(doc_id)
                .bind(tag_id)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;
            }
        }
        DocumentTagOp::Remove => {
            for tag in tags {
                let tag_id = sqlx::query_scalar::<_, Uuid>(
                    r#"
                    SELECT id
                    FROM tags
                    WHERE workspace_id = $1 AND name = $2
                    "#,
                )
                .bind(workspace_id)
                .bind(tag)
                .fetch_optional(&mut *tx)
                .await
                .map_err(map_sqlx_error)?;

                if let Some(tag_id) = tag_id {
                    sqlx::query("DELETE FROM document_tags WHERE document_id = $1 AND tag_id = $2")
                        .bind(doc_id)
                        .bind(tag_id)
                        .execute(&mut *tx)
                        .await
                        .map_err(map_sqlx_error)?;
                }
            }
        }
    }

    let new_etag = generate_etag();
    let row = sqlx::query_as::<_, DocumentRow>(
        r#"
        UPDATE documents
        SET etag = $3, updated_at = now()
        WHERE id = $1 AND workspace_id = $2 AND deleted_at IS NULL
        RETURNING id, workspace_id, path, title, head_seq, etag, created_by,
                  created_at, updated_at, archived_at
        "#,
    )
    .bind(doc_id)
    .bind(workspace_id)
    .bind(new_etag)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_sqlx_error)?
    .ok_or(DocApiError::NotFound)?;

    tx.commit().await.map_err(map_sqlx_error)?;
    Ok(row.into())
}

async fn create_acl_override_pg(
    pool: &PgPool,
    workspace_id: Uuid,
    doc_id: Uuid,
    payload: CreateAclOverrideRequest,
) -> Result<AclOverride, DocApiError> {
    let doc_exists = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT 1
        FROM documents
        WHERE id = $1 AND workspace_id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(doc_id)
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?
    .is_some();

    if !doc_exists {
        return Err(DocApiError::NotFound);
    }

    let row = sqlx::query_as::<_, AclOverrideRow>(
        r#"
        INSERT INTO acl_overrides (workspace_id, doc_id, subject_type, subject_id, role, expires_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, workspace_id, doc_id, subject_type, subject_id, role, expires_at, created_at
        "#,
    )
    .bind(workspace_id)
    .bind(doc_id)
    .bind(payload.subject_type)
    .bind(payload.subject_id)
    .bind(payload.role)
    .bind(payload.expires_at)
    .fetch_one(pool)
    .await
    .map_err(map_sqlx_error)?;

    Ok(row.into())
}

async fn delete_acl_override_pg(
    pool: &PgPool,
    workspace_id: Uuid,
    doc_id: Uuid,
    override_id: Uuid,
) -> Result<(), DocApiError> {
    let deleted = sqlx::query(
        r#"
        DELETE FROM acl_overrides
        WHERE id = $1 AND workspace_id = $2 AND doc_id = $3
        "#,
    )
    .bind(override_id)
    .bind(workspace_id)
    .bind(doc_id)
    .execute(pool)
    .await
    .map_err(map_sqlx_error)?
    .rows_affected();

    if deleted == 0 {
        return Err(DocApiError::NotFound);
    }

    Ok(())
}

// ── Memory implementations (for testing) ───────────────────────────

async fn create_mem(
    store: &RwLock<MemoryDocumentStore>,
    workspace_id: Uuid,
    created_by: Uuid,
    path: &str,
    title: Option<&str>,
) -> Result<Document, DocApiError> {
    let mut store = store.write().await;
    let path_norm = normalize_doc_path(path);

    if !store
        .workspace_owners
        .iter()
        .any(|(existing_workspace_id, _)| *existing_workspace_id == workspace_id)
    {
        store.workspace_owners.insert((workspace_id, created_by));
    }

    // Check uniqueness.
    let conflict = store.documents.values().any(|d| {
        d.workspace_id == workspace_id && d.path_norm == path_norm && d.deleted_at.is_none()
    });
    if conflict {
        return Err(DocApiError::Conflict);
    }

    let now = Utc::now();
    let id = Uuid::new_v4();
    let doc = MemoryDocument {
        id,
        workspace_id,
        path: path.to_string(),
        path_norm,
        title: title.map(|s| s.to_string()),
        head_seq: 0,
        etag: generate_etag(),
        created_by,
        created_at: now,
        updated_at: now,
        archived_at: None,
        deleted_at: None,
        tags: HashSet::new(),
    };
    let result = mem_to_document(&doc);
    store.documents.insert(id, doc);

    Ok(result)
}

async fn list_mem(
    store: &RwLock<MemoryDocumentStore>,
    workspace_id: Uuid,
    path_prefix: Option<&str>,
    archived: Option<bool>,
    limit: usize,
    _cursor: Option<DocCursor>,
) -> Result<(Vec<Document>, Option<String>), DocApiError> {
    let store = store.read().await;
    let prefix_norm = path_prefix.map(normalize_doc_path);

    let mut items: Vec<Document> = store
        .documents
        .values()
        .filter(|d| d.workspace_id == workspace_id && d.deleted_at.is_none())
        .filter(|d| match &prefix_norm {
            Some(p) => d.path_norm.starts_with(p.as_str()),
            None => true,
        })
        .filter(|d| match archived {
            Some(true) => d.archived_at.is_some(),
            Some(false) => d.archived_at.is_none(),
            None => true,
        })
        .map(mem_to_document)
        .collect();

    items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(b.id.cmp(&a.id)));
    let next_cursor = paginate(&mut items, limit);

    Ok((items, next_cursor))
}

async fn get_mem(
    store: &RwLock<MemoryDocumentStore>,
    workspace_id: Uuid,
    doc_id: Uuid,
) -> Result<Document, DocApiError> {
    let store = store.read().await;
    let doc = store
        .documents
        .get(&doc_id)
        .filter(|d| d.workspace_id == workspace_id && d.deleted_at.is_none())
        .ok_or(DocApiError::NotFound)?;

    Ok(mem_to_document(doc))
}

async fn update_mem(
    store: &RwLock<MemoryDocumentStore>,
    workspace_id: Uuid,
    doc_id: Uuid,
    if_match: &str,
    payload: &UpdateDocumentRequest,
) -> Result<Document, DocApiError> {
    let mut store = store.write().await;
    let doc = store
        .documents
        .get_mut(&doc_id)
        .filter(|d| d.workspace_id == workspace_id && d.deleted_at.is_none())
        .ok_or(DocApiError::NotFound)?;

    if !etag_matches(if_match, &doc.etag) {
        return Err(DocApiError::PreconditionFailed);
    }

    if let Some(path) = &payload.path {
        doc.path = path.clone();
        doc.path_norm = normalize_doc_path(path);
    }
    if let Some(title) = &payload.title {
        doc.title = Some(title.clone());
    }
    if let Some(archived) = payload.archived {
        doc.archived_at = if archived { Some(Utc::now()) } else { None };
    }
    doc.etag = generate_etag();
    doc.updated_at = Utc::now();

    Ok(mem_to_document(doc))
}

async fn delete_mem(
    store: &RwLock<MemoryDocumentStore>,
    workspace_id: Uuid,
    doc_id: Uuid,
    hard: bool,
) -> Result<(), DocApiError> {
    let mut store = store.write().await;

    if hard {
        let existed =
            store.documents.remove(&doc_id).filter(|d| d.workspace_id == workspace_id).is_some();
        if !existed {
            return Err(DocApiError::NotFound);
        }
    } else {
        let doc = store
            .documents
            .get_mut(&doc_id)
            .filter(|d| d.workspace_id == workspace_id && d.deleted_at.is_none())
            .ok_or(DocApiError::NotFound)?;
        doc.deleted_at = Some(Utc::now());
    }

    Ok(())
}

async fn update_tags_mem(
    store: &RwLock<MemoryDocumentStore>,
    workspace_id: Uuid,
    doc_id: Uuid,
    op: DocumentTagOp,
    tags: &[String],
) -> Result<Document, DocApiError> {
    let mut store = store.write().await;
    let doc = store
        .documents
        .get_mut(&doc_id)
        .filter(|d| d.workspace_id == workspace_id && d.deleted_at.is_none())
        .ok_or(DocApiError::NotFound)?;

    match op {
        DocumentTagOp::Add => {
            for tag in tags {
                doc.tags.insert(tag.clone());
            }
        }
        DocumentTagOp::Remove => {
            for tag in tags {
                doc.tags.remove(tag);
            }
        }
    }

    doc.etag = generate_etag();
    doc.updated_at = Utc::now();

    Ok(mem_to_document(doc))
}

async fn create_acl_override_mem(
    store: &RwLock<MemoryDocumentStore>,
    workspace_id: Uuid,
    doc_id: Uuid,
    payload: CreateAclOverrideRequest,
) -> Result<AclOverride, DocApiError> {
    let mut store = store.write().await;
    let doc_exists = store.documents.get(&doc_id).is_some_and(|document| {
        document.workspace_id == workspace_id && document.deleted_at.is_none()
    });
    if !doc_exists {
        return Err(DocApiError::NotFound);
    }

    let now = Utc::now();
    let override_id = Uuid::new_v4();
    let acl_override = MemoryAclOverride {
        id: override_id,
        workspace_id,
        doc_id,
        subject_type: payload.subject_type,
        subject_id: payload.subject_id,
        role: payload.role,
        expires_at: payload.expires_at,
        created_at: now,
    };
    store.acl_overrides.insert(override_id, acl_override.clone());
    Ok(AclOverride {
        id: acl_override.id,
        workspace_id: acl_override.workspace_id,
        doc_id: acl_override.doc_id,
        subject_type: acl_override.subject_type,
        subject_id: acl_override.subject_id,
        role: acl_override.role,
        expires_at: acl_override.expires_at,
        created_at: acl_override.created_at,
    })
}

async fn delete_acl_override_mem(
    store: &RwLock<MemoryDocumentStore>,
    workspace_id: Uuid,
    doc_id: Uuid,
    override_id: Uuid,
) -> Result<(), DocApiError> {
    let mut store = store.write().await;
    let Some(existing) = store.acl_overrides.get(&override_id) else {
        return Err(DocApiError::NotFound);
    };
    if existing.workspace_id != workspace_id || existing.doc_id != doc_id {
        return Err(DocApiError::NotFound);
    }

    store.acl_overrides.remove(&override_id);
    Ok(())
}

fn mem_to_document(doc: &MemoryDocument) -> Document {
    Document {
        id: doc.id,
        workspace_id: doc.workspace_id,
        path: doc.path.clone(),
        title: doc.title.clone(),
        head_seq: doc.head_seq,
        etag: doc.etag.clone(),
        created_by: doc.created_by,
        created_at: doc.created_at,
        updated_at: doc.updated_at,
        archived_at: doc.archived_at,
    }
}

async fn ensure_acl_override_manager_access(
    store: &DocumentStore,
    workspace_id: Uuid,
    doc_id: Uuid,
    user_id: Uuid,
) -> Result<(), DocApiError> {
    match store {
        DocumentStore::Postgres(pool) => {
            ensure_acl_override_manager_access_pg(pool, workspace_id, doc_id, user_id).await
        }
        DocumentStore::Memory(store) => {
            ensure_acl_override_manager_access_mem(store, workspace_id, doc_id, user_id).await
        }
    }
}

async fn ensure_acl_override_manager_access_pg(
    pool: &PgPool,
    workspace_id: Uuid,
    doc_id: Uuid,
    user_id: Uuid,
) -> Result<(), DocApiError> {
    let row = sqlx::query_as::<_, (bool, bool)>(
        r#"
        SELECT
            d.created_by = $3 AS is_document_owner,
            EXISTS(
                SELECT 1
                FROM workspace_members AS wm
                WHERE wm.workspace_id = $1
                  AND wm.user_id = $3
                  AND wm.status = 'active'
                  AND wm.role = 'owner'
            ) AS is_workspace_owner
        FROM documents AS d
        WHERE d.workspace_id = $1
          AND d.id = $2
          AND d.deleted_at IS NULL
        "#,
    )
    .bind(workspace_id)
    .bind(doc_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?;

    let Some((is_document_owner, is_workspace_owner)) = row else {
        return Err(DocApiError::NotFound);
    };

    if is_document_owner || is_workspace_owner {
        Ok(())
    } else {
        Err(DocApiError::Forbidden)
    }
}

async fn ensure_acl_override_manager_access_mem(
    store: &RwLock<MemoryDocumentStore>,
    workspace_id: Uuid,
    doc_id: Uuid,
    user_id: Uuid,
) -> Result<(), DocApiError> {
    let store = store.read().await;
    let doc = store.documents.get(&doc_id).ok_or(DocApiError::NotFound)?;
    if doc.workspace_id != workspace_id || doc.deleted_at.is_some() {
        return Err(DocApiError::NotFound);
    }

    let is_document_owner = doc.created_by == user_id;
    let is_workspace_owner = store.workspace_owners.contains(&(workspace_id, user_id));
    if is_document_owner || is_workspace_owner {
        Ok(())
    } else {
        Err(DocApiError::Forbidden)
    }
}

// ── Helpers ────────────────────────────────────────────────────────

const DEFAULT_PAGE_SIZE: usize = 50;
const MAX_PAGE_SIZE: usize = 100;

struct DocCursor {
    updated_at: DateTime<Utc>,
    id: Uuid,
}

fn normalize_limit(limit: Option<usize>) -> usize {
    match limit {
        Some(0) => DEFAULT_PAGE_SIZE,
        Some(value) => value.min(MAX_PAGE_SIZE),
        None => DEFAULT_PAGE_SIZE,
    }
}

fn parse_cursor(value: &str) -> Result<DocCursor, DocApiError> {
    let (ts, id) = value
        .split_once('|')
        .ok_or_else(|| DocApiError::bad_request("cursor format is invalid"))?;

    let ts =
        ts.parse::<i64>().map_err(|_| DocApiError::bad_request("cursor timestamp is invalid"))?;
    let updated_at = DateTime::<Utc>::from_timestamp_micros(ts)
        .ok_or_else(|| DocApiError::bad_request("cursor timestamp is invalid"))?;
    let id = Uuid::parse_str(id).map_err(|_| DocApiError::bad_request("cursor id is invalid"))?;

    Ok(DocCursor { updated_at, id })
}

fn encode_cursor(doc: &Document) -> String {
    format!("{}|{}", doc.updated_at.timestamp_micros(), doc.id)
}

fn paginate(items: &mut Vec<Document>, limit: usize) -> Option<String> {
    if items.len() > limit {
        items.truncate(limit);
        items.last().map(encode_cursor)
    } else {
        None
    }
}

fn normalize_doc_path(path: &str) -> String {
    path.trim().to_lowercase()
}

fn validate_path(path: &str) -> Result<(), DocApiError> {
    if path.trim().is_empty() {
        return Err(DocApiError::bad_request("document path must not be empty"));
    }
    if path.len() > 512 {
        return Err(DocApiError::bad_request("document path must not exceed 512 characters"));
    }
    Ok(())
}

fn normalize_tag_names(raw_tags: &[String]) -> Result<Vec<String>, DocApiError> {
    if raw_tags.is_empty() {
        return Err(DocApiError::bad_request("tags must contain at least one tag"));
    }

    let mut out = Vec::with_capacity(raw_tags.len());
    let mut seen = HashSet::with_capacity(raw_tags.len());

    for raw in raw_tags {
        let tag = raw.trim().to_lowercase();
        if tag.is_empty() {
            return Err(DocApiError::bad_request("tag names must not be empty"));
        }
        if tag.len() > 64 {
            return Err(DocApiError::bad_request("tag names must not exceed 64 characters"));
        }
        if seen.insert(tag.clone()) {
            out.push(tag);
        }
    }

    Ok(out)
}

fn validate_acl_override_request(payload: &CreateAclOverrideRequest) -> Result<(), DocApiError> {
    if payload.subject_type != "user"
        && payload.subject_type != "agent"
        && payload.subject_type != "share_link"
    {
        return Err(DocApiError::bad_request(
            "subject_type must be one of: user, agent, share_link",
        ));
    }

    if payload.subject_id.trim().is_empty() {
        return Err(DocApiError::bad_request("subject_id must not be empty"));
    }

    if payload.role != "editor" && payload.role != "viewer" {
        return Err(DocApiError::bad_request("role must be one of: editor, viewer"));
    }

    if payload.expires_at.is_some_and(|value| value <= Utc::now()) {
        return Err(DocApiError::bad_request("expires_at must be in the future"));
    }

    Ok(())
}

fn extract_if_match(headers: &HeaderMap) -> Result<&str, DocApiError> {
    let value = headers.get(IF_MATCH).ok_or(DocApiError::PreconditionRequired)?;
    value.to_str().map_err(|_| DocApiError::bad_request("If-Match header is not valid utf-8"))
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

fn generate_etag() -> String {
    format!("\"doc-{}\"", Uuid::new_v4())
}

fn map_sqlx_error(error: sqlx::Error) -> DocApiError {
    if let sqlx::Error::Database(db_error) = &error {
        if db_error.code().as_deref() == Some("23505") {
            return DocApiError::Conflict;
        }
    }
    DocApiError::internal(error.into())
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    use super::*;
    use crate::auth::jwt::JwtAccessTokenService;

    fn test_jwt_service() -> Arc<JwtAccessTokenService> {
        Arc::new(
            JwtAccessTokenService::new("test-secret-that-is-at-least-32-chars-long!!")
                .expect("jwt service"),
        )
    }

    fn test_store() -> DocumentStore {
        DocumentStore::Memory(Arc::new(RwLock::new(MemoryDocumentStore::default())))
    }

    fn test_router() -> Router {
        build_router_with_store(test_store(), test_jwt_service())
    }

    fn auth_token(jwt: &JwtAccessTokenService, user_id: Uuid, workspace_id: Uuid) -> String {
        jwt.issue_workspace_token(user_id, workspace_id).expect("token")
    }

    fn json_request(
        method: &str,
        uri: &str,
        body: serde_json::Value,
        token: &str,
    ) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap()
    }

    fn get_request(uri: &str, token: &str) -> Request<Body> {
        Request::builder()
            .method("GET")
            .uri(uri)
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap()
    }

    fn delete_request(uri: &str, token: &str) -> Request<Body> {
        Request::builder()
            .method("DELETE")
            .uri(uri)
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap()
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn create_document_returns_201() {
        let app = test_router();
        let jwt = test_jwt_service();
        let ws_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let token = auth_token(&jwt, user_id, ws_id);

        let resp = app
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents"),
                serde_json::json!({"path": "notes/hello.md", "title": "Hello"}),
                &token,
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::CREATED);

        let body = body_json(resp).await;
        assert_eq!(body["document"]["path"], "notes/hello.md");
        assert_eq!(body["document"]["title"], "Hello");
        assert!(body["document"]["id"].is_string());
    }

    #[tokio::test]
    async fn create_document_rejects_empty_path() {
        let app = test_router();
        let jwt = test_jwt_service();
        let ws_id = Uuid::new_v4();
        let token = auth_token(&jwt, Uuid::new_v4(), ws_id);

        let resp = app
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents"),
                serde_json::json!({"path": "  "}),
                &token,
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn create_document_rejects_duplicate_path() {
        let store = test_store();
        let jwt = test_jwt_service();
        let app = build_router_with_store(store, jwt.clone());

        let ws_id = Uuid::new_v4();
        let token = auth_token(&jwt, Uuid::new_v4(), ws_id);

        // First create succeeds.
        let resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents"),
                serde_json::json!({"path": "notes/hello.md"}),
                &token,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // Duplicate path fails.
        let resp = app
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents"),
                serde_json::json!({"path": "notes/hello.md"}),
                &token,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn list_documents_empty() {
        let app = test_router();
        let jwt = test_jwt_service();
        let ws_id = Uuid::new_v4();
        let token = auth_token(&jwt, Uuid::new_v4(), ws_id);

        let resp = app
            .oneshot(get_request(&format!("/v1/workspaces/{ws_id}/documents"), &token))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        assert_eq!(body["items"].as_array().unwrap().len(), 0);
        assert!(body["next_cursor"].is_null());
    }

    #[tokio::test]
    async fn get_document_returns_created_doc() {
        let store = test_store();
        let jwt = test_jwt_service();
        let app = build_router_with_store(store, jwt.clone());

        let ws_id = Uuid::new_v4();
        let token = auth_token(&jwt, Uuid::new_v4(), ws_id);

        // Create.
        let resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents"),
                serde_json::json!({"path": "readme.md"}),
                &token,
            ))
            .await
            .unwrap();
        let body = body_json(resp).await;
        let doc_id = body["document"]["id"].as_str().unwrap();

        // Get.
        let resp = app
            .oneshot(get_request(&format!("/v1/workspaces/{ws_id}/documents/{doc_id}"), &token))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        assert_eq!(body["document"]["id"], doc_id);
        assert_eq!(body["document"]["path"], "readme.md");
    }

    #[tokio::test]
    async fn get_nonexistent_returns_404() {
        let app = test_router();
        let jwt = test_jwt_service();
        let ws_id = Uuid::new_v4();
        let token = auth_token(&jwt, Uuid::new_v4(), ws_id);

        let resp = app
            .oneshot(get_request(
                &format!("/v1/workspaces/{ws_id}/documents/{}", Uuid::new_v4()),
                &token,
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn update_document_with_matching_etag() {
        let store = test_store();
        let jwt = test_jwt_service();
        let app = build_router_with_store(store, jwt.clone());

        let ws_id = Uuid::new_v4();
        let token = auth_token(&jwt, Uuid::new_v4(), ws_id);

        // Create.
        let resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents"),
                serde_json::json!({"path": "old.md"}),
                &token,
            ))
            .await
            .unwrap();
        let body = body_json(resp).await;
        let doc_id = body["document"]["id"].as_str().unwrap();
        let etag = body["document"]["etag"].as_str().unwrap();

        // Update with matching etag.
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(&format!("/v1/workspaces/{ws_id}/documents/{doc_id}"))
                    .header("Content-Type", "application/json")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("If-Match", etag)
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({"path": "new.md"})).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        assert_eq!(body["document"]["path"], "new.md");
    }

    #[tokio::test]
    async fn update_document_without_if_match_returns_428() {
        let store = test_store();
        let jwt = test_jwt_service();
        let app = build_router_with_store(store, jwt.clone());

        let ws_id = Uuid::new_v4();
        let token = auth_token(&jwt, Uuid::new_v4(), ws_id);

        // Create.
        let resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents"),
                serde_json::json!({"path": "test.md"}),
                &token,
            ))
            .await
            .unwrap();
        let body = body_json(resp).await;
        let doc_id = body["document"]["id"].as_str().unwrap();

        // Update without If-Match.
        let resp = app
            .oneshot(json_request(
                "PATCH",
                &format!("/v1/workspaces/{ws_id}/documents/{doc_id}"),
                serde_json::json!({"title": "New Title"}),
                &token,
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::PRECONDITION_REQUIRED);
    }

    #[tokio::test]
    async fn update_document_tags_add_and_remove_returns_document() {
        let store = test_store();
        let jwt = test_jwt_service();
        let app = build_router_with_store(store, jwt.clone());

        let ws_id = Uuid::new_v4();
        let token = auth_token(&jwt, Uuid::new_v4(), ws_id);

        let create_resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents"),
                serde_json::json!({"path": "tagged.md"}),
                &token,
            ))
            .await
            .unwrap();
        let create_body = body_json(create_resp).await;
        let doc_id = create_body["document"]["id"].as_str().unwrap();
        let etag_before = create_body["document"]["etag"].as_str().unwrap().to_string();

        let add_resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents/{doc_id}/tags"),
                serde_json::json!({"op": "add", "tags": ["approved", "rfc"]}),
                &token,
            ))
            .await
            .unwrap();
        assert_eq!(add_resp.status(), StatusCode::OK);
        let add_body = body_json(add_resp).await;
        let etag_after_add = add_body["document"]["etag"].as_str().unwrap().to_string();
        assert_ne!(etag_after_add, etag_before);
        assert_eq!(add_body["document"]["path"], "tagged.md");

        let remove_resp = app
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents/{doc_id}/tags"),
                serde_json::json!({"op": "remove", "tags": ["approved"]}),
                &token,
            ))
            .await
            .unwrap();
        assert_eq!(remove_resp.status(), StatusCode::OK);
        let remove_body = body_json(remove_resp).await;
        let etag_after_remove = remove_body["document"]["etag"].as_str().unwrap().to_string();
        assert_ne!(etag_after_remove, etag_after_add);
        assert_eq!(remove_body["document"]["id"], doc_id);
    }

    #[tokio::test]
    async fn update_document_tags_rejects_empty_tag_list() {
        let app = test_router();
        let jwt = test_jwt_service();
        let ws_id = Uuid::new_v4();
        let token = auth_token(&jwt, Uuid::new_v4(), ws_id);

        let create_resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents"),
                serde_json::json!({"path": "tagged.md"}),
                &token,
            ))
            .await
            .unwrap();
        let create_body = body_json(create_resp).await;
        let doc_id = create_body["document"]["id"].as_str().unwrap();

        let resp = app
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents/{doc_id}/tags"),
                serde_json::json!({"op": "add", "tags": []}),
                &token,
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn create_and_delete_acl_override() {
        let app = test_router();
        let jwt = test_jwt_service();
        let ws_id = Uuid::new_v4();
        let token = auth_token(&jwt, Uuid::new_v4(), ws_id);

        let create_doc_resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents"),
                serde_json::json!({"path": "acl.md"}),
                &token,
            ))
            .await
            .unwrap();
        let create_doc_body = body_json(create_doc_resp).await;
        let doc_id = create_doc_body["document"]["id"].as_str().unwrap();

        let create_override_resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents/{doc_id}/acl-overrides"),
                serde_json::json!({
                    "subject_type": "agent",
                    "subject_id": "cursor",
                    "role": "editor"
                }),
                &token,
            ))
            .await
            .unwrap();
        assert_eq!(create_override_resp.status(), StatusCode::CREATED);
        let create_override_body = body_json(create_override_resp).await;
        assert_eq!(create_override_body["acl_override"]["subject_type"], "agent");
        assert_eq!(create_override_body["acl_override"]["subject_id"], "cursor");
        assert_eq!(create_override_body["acl_override"]["role"], "editor");
        let override_id = create_override_body["acl_override"]["id"].as_str().unwrap();

        let delete_override_resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/v1/workspaces/{ws_id}/documents/{doc_id}/acl-overrides/{override_id}"
                    ))
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(delete_override_resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn create_acl_override_rejects_non_owner_and_non_document_owner() {
        let app = test_router();
        let jwt = test_jwt_service();
        let ws_id = Uuid::new_v4();
        let owner_id = Uuid::new_v4();
        let owner_token = auth_token(&jwt, owner_id, ws_id);
        let viewer_token = auth_token(&jwt, Uuid::new_v4(), ws_id);

        let create_doc_resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents"),
                serde_json::json!({"path": "acl-owner.md"}),
                &owner_token,
            ))
            .await
            .unwrap();
        let create_doc_body = body_json(create_doc_resp).await;
        let doc_id = create_doc_body["document"]["id"].as_str().unwrap();

        let create_override_resp = app
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents/{doc_id}/acl-overrides"),
                serde_json::json!({
                    "subject_type": "agent",
                    "subject_id": "cursor",
                    "role": "editor"
                }),
                &viewer_token,
            ))
            .await
            .unwrap();

        assert_eq!(create_override_resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn workspace_owner_can_manage_acl_overrides_for_other_document_owner() {
        let app = test_router();
        let jwt = test_jwt_service();
        let ws_id = Uuid::new_v4();
        let owner_id = Uuid::new_v4();
        let owner_token = auth_token(&jwt, owner_id, ws_id);
        let other_author_token = auth_token(&jwt, Uuid::new_v4(), ws_id);

        // First document creation seeds owner_id as workspace owner in memory store.
        let _ = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents"),
                serde_json::json!({"path": "seed-owner.md"}),
                &owner_token,
            ))
            .await
            .unwrap();

        let create_doc_resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents"),
                serde_json::json!({"path": "other-author.md"}),
                &other_author_token,
            ))
            .await
            .unwrap();
        let create_doc_body = body_json(create_doc_resp).await;
        let doc_id = create_doc_body["document"]["id"].as_str().unwrap();

        let create_override_resp = app
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents/{doc_id}/acl-overrides"),
                serde_json::json!({
                    "subject_type": "agent",
                    "subject_id": "cursor",
                    "role": "viewer"
                }),
                &owner_token,
            ))
            .await
            .unwrap();

        assert_eq!(create_override_resp.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn create_acl_override_rejects_invalid_role() {
        let app = test_router();
        let jwt = test_jwt_service();
        let ws_id = Uuid::new_v4();
        let token = auth_token(&jwt, Uuid::new_v4(), ws_id);

        let create_doc_resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents"),
                serde_json::json!({"path": "acl.md"}),
                &token,
            ))
            .await
            .unwrap();
        let create_doc_body = body_json(create_doc_resp).await;
        let doc_id = create_doc_body["document"]["id"].as_str().unwrap();

        let resp = app
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents/{doc_id}/acl-overrides"),
                serde_json::json!({
                    "subject_type": "user",
                    "subject_id": "user-1",
                    "role": "owner"
                }),
                &token,
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn create_acl_override_returns_not_found_for_missing_document() {
        let app = test_router();
        let jwt = test_jwt_service();
        let ws_id = Uuid::new_v4();
        let token = auth_token(&jwt, Uuid::new_v4(), ws_id);

        let missing_doc_id = Uuid::new_v4();
        let resp = app
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents/{missing_doc_id}/acl-overrides"),
                serde_json::json!({
                    "subject_type": "agent",
                    "subject_id": "cursor",
                    "role": "viewer"
                }),
                &token,
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn soft_delete_hides_from_list() {
        let store = test_store();
        let jwt = test_jwt_service();
        let app = build_router_with_store(store, jwt.clone());

        let ws_id = Uuid::new_v4();
        let token = auth_token(&jwt, Uuid::new_v4(), ws_id);

        // Create.
        let resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents"),
                serde_json::json!({"path": "to-delete.md"}),
                &token,
            ))
            .await
            .unwrap();
        let body = body_json(resp).await;
        let doc_id = body["document"]["id"].as_str().unwrap();

        // Soft delete.
        let resp = app
            .clone()
            .oneshot(delete_request(&format!("/v1/workspaces/{ws_id}/documents/{doc_id}"), &token))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // List should be empty.
        let resp = app
            .oneshot(get_request(&format!("/v1/workspaces/{ws_id}/documents"), &token))
            .await
            .unwrap();
        let body = body_json(resp).await;
        assert_eq!(body["items"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn unauthenticated_request_returns_401() {
        let app = test_router();
        let ws_id = Uuid::new_v4();

        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(&format!("/v1/workspaces/{ws_id}/documents"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
