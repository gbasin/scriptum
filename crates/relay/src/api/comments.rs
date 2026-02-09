// Comment thread endpoints for the relay API.
//
// Routes:
//   GET  /v1/workspaces/{ws_id}/documents/{doc_id}/comments         — list (status filter, pagination)
//   POST /v1/workspaces/{ws_id}/documents/{doc_id}/comments         — create thread + first message
//   POST /v1/workspaces/{ws_id}/comments/{thread_id}/messages       — reply
//   POST /v1/workspaces/{ws_id}/comments/{thread_id}/resolve        — resolve
//   POST /v1/workspaces/{ws_id}/comments/{thread_id}/reopen         — reopen

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Extension, Json, Path, Query, State},
    http::StatusCode,
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{
    types::chrono::{DateTime, Utc},
    PgPool,
};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{
    audit::{self, AuditEventType, NewAuditEvent},
    auth::{
        jwt::JwtAccessTokenService,
        middleware::{require_bearer_auth, AuthenticatedUser, WorkspaceRole},
    },
    error::{current_request_id, ErrorCode, RelayError},
    validation::ValidatedJson,
};

const DEFAULT_PAGE_SIZE: usize = 50;
const MAX_PAGE_SIZE: usize = 100;

// ── Public API types ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct CommentThread {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub doc_id: Uuid,
    pub section_id: Option<String>,
    pub start_offset_utf16: Option<i32>,
    pub end_offset_utf16: Option<i32>,
    pub status: String,
    pub version: i32,
    pub created_by_user_id: Option<Uuid>,
    pub created_by_agent_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommentMessage {
    pub id: Uuid,
    pub thread_id: Uuid,
    pub author_user_id: Option<Uuid>,
    pub author_agent_id: Option<String>,
    pub body_md: String,
    pub created_at: DateTime<Utc>,
    pub edited_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
struct CommentThreadWithMessages {
    thread: CommentThread,
    messages: Vec<CommentMessage>,
}

#[derive(Deserialize)]
pub struct ListCommentsQuery {
    status: Option<String>,
    limit: Option<usize>,
    cursor: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateCommentThreadRequest {
    anchor: CommentAnchorRequest,
    message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommentAnchorRequest {
    section_id: Option<String>,
    start_offset_utf16: Option<i32>,
    end_offset_utf16: Option<i32>,
    head_seq: Option<i64>,
}

#[derive(Deserialize)]
pub struct CreateCommentMessageRequest {
    body_md: String,
}

#[derive(Deserialize)]
pub struct SetCommentStatusRequest {
    if_version: i32,
}

#[derive(Serialize)]
struct ListCommentsResponse {
    items: Vec<CommentThreadWithMessages>,
    next_cursor: Option<String>,
}

#[derive(Serialize)]
struct CreateThreadResponse {
    thread: CommentThread,
    messages: Vec<CommentMessage>,
}

#[derive(Serialize)]
struct CreateMessageResponse {
    thread: CommentThread,
    messages: Vec<CommentMessage>,
}

#[derive(Serialize)]
struct ThreadResponse {
    thread: CommentThread,
}

// ── State & Store ────────────────────────────────────────────────────────────

#[derive(Clone)]
struct CommentsApiState {
    store: CommentStore,
}

#[derive(Clone)]
enum CommentStore {
    Postgres(PgPool),
    #[cfg_attr(not(test), allow(dead_code))]
    Memory(Arc<RwLock<MemoryCommentStore>>),
}

#[derive(Default)]
struct MemoryCommentStore {
    threads: HashMap<Uuid, MemoryCommentThread>,
    messages: HashMap<Uuid, Vec<MemoryCommentMessage>>,
    workspace_members: HashMap<(Uuid, Uuid), WorkspaceRole>,
}

#[derive(Clone)]
struct MemoryCommentThread {
    id: Uuid,
    workspace_id: Uuid,
    doc_id: Uuid,
    section_id: Option<String>,
    start_offset_utf16: Option<i32>,
    end_offset_utf16: Option<i32>,
    status: String,
    version: i32,
    created_by_user_id: Option<Uuid>,
    created_by_agent_id: Option<String>,
    created_at: DateTime<Utc>,
    resolved_at: Option<DateTime<Utc>>,
}

#[derive(Clone)]
struct MemoryCommentMessage {
    id: Uuid,
    thread_id: Uuid,
    author_user_id: Option<Uuid>,
    author_agent_id: Option<String>,
    body_md: String,
    created_at: DateTime<Utc>,
    edited_at: Option<DateTime<Utc>>,
}

// ── SQL Rows ─────────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct CommentThreadRow {
    id: Uuid,
    workspace_id: Uuid,
    doc_id: Uuid,
    section_id: Option<String>,
    start_offset_utf16: Option<i32>,
    end_offset_utf16: Option<i32>,
    status: String,
    version: i32,
    created_by_user_id: Option<Uuid>,
    created_by_agent_id: Option<String>,
    created_at: DateTime<Utc>,
    resolved_at: Option<DateTime<Utc>>,
}

#[derive(sqlx::FromRow)]
struct CommentMessageRow {
    id: Uuid,
    thread_id: Uuid,
    author_user_id: Option<Uuid>,
    author_agent_id: Option<String>,
    body_md: String,
    created_at: DateTime<Utc>,
    edited_at: Option<DateTime<Utc>>,
}

#[derive(sqlx::FromRow)]
struct CommentThreadStatusRow {
    version: i32,
    status: String,
}

impl From<CommentThreadRow> for CommentThread {
    fn from(value: CommentThreadRow) -> Self {
        Self {
            id: value.id,
            workspace_id: value.workspace_id,
            doc_id: value.doc_id,
            section_id: value.section_id,
            start_offset_utf16: value.start_offset_utf16,
            end_offset_utf16: value.end_offset_utf16,
            status: value.status,
            version: value.version,
            created_by_user_id: value.created_by_user_id,
            created_by_agent_id: value.created_by_agent_id,
            created_at: value.created_at,
            resolved_at: value.resolved_at,
        }
    }
}

impl From<CommentMessageRow> for CommentMessage {
    fn from(value: CommentMessageRow) -> Self {
        Self {
            id: value.id,
            thread_id: value.thread_id,
            author_user_id: value.author_user_id,
            author_agent_id: value.author_agent_id,
            body_md: value.body_md,
            created_at: value.created_at,
            edited_at: value.edited_at,
        }
    }
}

// ── Error ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
enum CommentsApiError {
    BadRequest { message: String },
    Forbidden,
    NotFound { message: &'static str },
    PreconditionFailed,
    Internal(anyhow::Error),
}

impl CommentsApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest { message: message.into() }
    }

    fn internal(error: anyhow::Error) -> Self {
        Self::Internal(error)
    }
}

impl IntoResponse for CommentsApiError {
    fn into_response(self) -> Response {
        match self {
            Self::BadRequest { message } => {
                RelayError::new(ErrorCode::ValidationFailed, message).into_response()
            }
            Self::Forbidden => {
                RelayError::new(ErrorCode::AuthForbidden, "caller lacks required role")
                    .into_response()
            }
            Self::NotFound { message } => {
                RelayError::new(ErrorCode::NotFound, message).into_response()
            }
            Self::PreconditionFailed => RelayError::new(
                ErrorCode::EditPreconditionFailed,
                "if_version does not match current thread version",
            )
            .into_response(),
            Self::Internal(error) => {
                tracing::error!(error = ?error, "comments api internal error");
                RelayError::from_code(ErrorCode::InternalError).into_response()
            }
        }
    }
}

// ── Router ───────────────────────────────────────────────────────────────────

pub fn router(pool: PgPool, jwt_service: Arc<JwtAccessTokenService>) -> Router {
    build_router_with_store(CommentStore::Postgres(pool), jwt_service)
}

fn build_router_with_store(store: CommentStore, jwt_service: Arc<JwtAccessTokenService>) -> Router {
    let state = CommentsApiState { store };

    Router::new()
        .route(
            "/v1/workspaces/{ws_id}/documents/{doc_id}/comments",
            get(list_comments).post(create_comment_thread),
        )
        .route("/v1/workspaces/{ws_id}/comments/{thread_id}/messages", post(create_comment_message))
        .route("/v1/workspaces/{ws_id}/comments/{thread_id}/resolve", post(resolve_comment_thread))
        .route("/v1/workspaces/{ws_id}/comments/{thread_id}/reopen", post(reopen_comment_thread))
        .with_state(state)
        .route_layer(middleware::from_fn_with_state(jwt_service, require_bearer_auth))
}

// ── Handlers ─────────────────────────────────────────────────────────────────

async fn list_comments(
    State(state): State<CommentsApiState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((ws_id, doc_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<ListCommentsQuery>,
) -> Result<Json<ListCommentsResponse>, CommentsApiError> {
    require_workspace_role(&state.store, &user, ws_id, WorkspaceRole::Viewer).await?;

    let status_filter = parse_status_filter(query.status.as_deref())?;
    let limit = normalize_limit(query.limit);
    let cursor = match query.cursor {
        Some(raw) => Some(parse_cursor(&raw)?),
        None => None,
    };

    let (items, next_cursor) =
        state.store.list_threads(ws_id, doc_id, status_filter, limit, cursor).await?;

    Ok(Json(ListCommentsResponse { items, next_cursor }))
}

async fn create_comment_thread(
    State(state): State<CommentsApiState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((ws_id, doc_id)): Path<(Uuid, Uuid)>,
    ValidatedJson(payload): ValidatedJson<CreateCommentThreadRequest>,
) -> Result<(StatusCode, Json<CreateThreadResponse>), CommentsApiError> {
    require_workspace_role(&state.store, &user, ws_id, WorkspaceRole::Editor).await?;

    validate_anchor(&payload.anchor)?;
    validate_markdown_body("message", &payload.message)?;

    let (thread, message) = state
        .store
        .create_thread(ws_id, doc_id, user.user_id, payload.anchor, payload.message)
        .await?;

    try_record_comment_audit_event(
        &state,
        ws_id,
        user.user_id,
        thread.id,
        "create",
        json!({
            "doc_id": doc_id,
            "message_id": message.id,
            "section_id": thread.section_id,
        }),
    )
    .await;

    Ok((StatusCode::CREATED, Json(CreateThreadResponse { thread, messages: vec![message] })))
}

async fn create_comment_message(
    State(state): State<CommentsApiState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((ws_id, thread_id)): Path<(Uuid, Uuid)>,
    ValidatedJson(payload): ValidatedJson<CreateCommentMessageRequest>,
) -> Result<Json<CreateMessageResponse>, CommentsApiError> {
    require_workspace_role(&state.store, &user, ws_id, WorkspaceRole::Editor).await?;

    validate_markdown_body("body_md", &payload.body_md)?;

    let message =
        state.store.create_message(ws_id, thread_id, user.user_id, payload.body_md).await?;
    let thread_with_messages = state.store.get_thread_with_messages(ws_id, thread_id).await?;

    // Sanity check to ensure the newly-created message is reflected in the thread response.
    if !thread_with_messages.messages.iter().any(|existing| existing.id == message.id) {
        return Err(CommentsApiError::internal(anyhow::anyhow!(
            "created comment message missing from thread view"
        )));
    }

    Ok(Json(CreateMessageResponse {
        thread: thread_with_messages.thread,
        messages: thread_with_messages.messages,
    }))
}

async fn resolve_comment_thread(
    State(state): State<CommentsApiState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((ws_id, thread_id)): Path<(Uuid, Uuid)>,
    ValidatedJson(payload): ValidatedJson<SetCommentStatusRequest>,
) -> Result<Json<ThreadResponse>, CommentsApiError> {
    require_workspace_role(&state.store, &user, ws_id, WorkspaceRole::Editor).await?;

    if payload.if_version < 1 {
        return Err(CommentsApiError::bad_request("if_version must be >= 1"));
    }

    let thread = state
        .store
        .set_thread_status(ws_id, thread_id, payload.if_version, CommentStatus::Resolved)
        .await?;

    try_record_comment_audit_event(
        &state,
        ws_id,
        user.user_id,
        thread.id,
        "resolve",
        json!({
            "if_version": payload.if_version,
            "next_version": thread.version,
        }),
    )
    .await;

    Ok(Json(ThreadResponse { thread }))
}

async fn reopen_comment_thread(
    State(state): State<CommentsApiState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((ws_id, thread_id)): Path<(Uuid, Uuid)>,
    ValidatedJson(payload): ValidatedJson<SetCommentStatusRequest>,
) -> Result<Json<ThreadResponse>, CommentsApiError> {
    require_workspace_role(&state.store, &user, ws_id, WorkspaceRole::Editor).await?;

    if payload.if_version < 1 {
        return Err(CommentsApiError::bad_request("if_version must be >= 1"));
    }

    let thread = state
        .store
        .set_thread_status(ws_id, thread_id, payload.if_version, CommentStatus::Open)
        .await?;

    try_record_comment_audit_event(
        &state,
        ws_id,
        user.user_id,
        thread.id,
        "reopen",
        json!({
            "if_version": payload.if_version,
            "next_version": thread.version,
        }),
    )
    .await;

    Ok(Json(ThreadResponse { thread }))
}

// ── Store dispatch ───────────────────────────────────────────────────────────

impl CommentStore {
    async fn create_thread(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        created_by_user_id: Uuid,
        anchor: CommentAnchorRequest,
        body_md: String,
    ) -> Result<(CommentThread, CommentMessage), CommentsApiError> {
        match self {
            Self::Postgres(pool) => {
                create_thread_pg(pool, workspace_id, doc_id, created_by_user_id, anchor, body_md)
                    .await
            }
            Self::Memory(store) => {
                create_thread_mem(store, workspace_id, doc_id, created_by_user_id, anchor, body_md)
                    .await
            }
        }
    }

    async fn list_threads(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        status_filter: Option<CommentStatus>,
        limit: usize,
        cursor: Option<CommentCursor>,
    ) -> Result<(Vec<CommentThreadWithMessages>, Option<String>), CommentsApiError> {
        match self {
            Self::Postgres(pool) => {
                list_threads_pg(pool, workspace_id, doc_id, status_filter, limit, cursor).await
            }
            Self::Memory(store) => {
                list_threads_mem(store, workspace_id, doc_id, status_filter, limit, cursor).await
            }
        }
    }

    async fn create_message(
        &self,
        workspace_id: Uuid,
        thread_id: Uuid,
        author_user_id: Uuid,
        body_md: String,
    ) -> Result<CommentMessage, CommentsApiError> {
        match self {
            Self::Postgres(pool) => {
                create_message_pg(pool, workspace_id, thread_id, author_user_id, body_md).await
            }
            Self::Memory(store) => {
                create_message_mem(store, workspace_id, thread_id, author_user_id, body_md).await
            }
        }
    }

    async fn get_thread_with_messages(
        &self,
        workspace_id: Uuid,
        thread_id: Uuid,
    ) -> Result<CommentThreadWithMessages, CommentsApiError> {
        match self {
            Self::Postgres(pool) => {
                get_thread_with_messages_pg(pool, workspace_id, thread_id).await
            }
            Self::Memory(store) => {
                get_thread_with_messages_mem(store, workspace_id, thread_id).await
            }
        }
    }

    async fn set_thread_status(
        &self,
        workspace_id: Uuid,
        thread_id: Uuid,
        if_version: i32,
        next_status: CommentStatus,
    ) -> Result<CommentThread, CommentsApiError> {
        match self {
            Self::Postgres(pool) => {
                set_thread_status_pg(pool, workspace_id, thread_id, if_version, next_status).await
            }
            Self::Memory(store) => {
                set_thread_status_mem(store, workspace_id, thread_id, if_version, next_status).await
            }
        }
    }

    async fn workspace_role_for_user(
        &self,
        user_id: Uuid,
        workspace_id: Uuid,
    ) -> Result<Option<WorkspaceRole>, CommentsApiError> {
        match self {
            Self::Postgres(pool) => workspace_role_for_user_pg(pool, user_id, workspace_id).await,
            Self::Memory(store) => workspace_role_for_user_mem(store, user_id, workspace_id).await,
        }
    }

    fn postgres_pool(&self) -> Option<&PgPool> {
        match self {
            Self::Postgres(pool) => Some(pool),
            Self::Memory(_) => None,
        }
    }

    #[cfg(test)]
    async fn grant_workspace_role_for_tests(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
        role: WorkspaceRole,
    ) {
        if let Self::Memory(store) = self {
            store.write().await.workspace_members.insert((workspace_id, user_id), role);
        }
    }
}

// ── Postgres store ───────────────────────────────────────────────────────────

async fn ensure_document_exists_pg(
    pool: &PgPool,
    workspace_id: Uuid,
    doc_id: Uuid,
) -> Result<(), CommentsApiError> {
    let exists = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM documents
            WHERE id = $1
              AND workspace_id = $2
              AND deleted_at IS NULL
        )
        "#,
    )
    .bind(doc_id)
    .bind(workspace_id)
    .fetch_one(pool)
    .await
    .map_err(map_sqlx_error)?;

    if exists {
        Ok(())
    } else {
        Err(CommentsApiError::NotFound { message: "document not found" })
    }
}

async fn create_thread_pg(
    pool: &PgPool,
    workspace_id: Uuid,
    doc_id: Uuid,
    created_by_user_id: Uuid,
    anchor: CommentAnchorRequest,
    body_md: String,
) -> Result<(CommentThread, CommentMessage), CommentsApiError> {
    ensure_document_exists_pg(pool, workspace_id, doc_id).await?;

    let mut tx = pool.begin().await.map_err(|error| CommentsApiError::internal(error.into()))?;

    let thread_row = sqlx::query_as::<_, CommentThreadRow>(
        r#"
        INSERT INTO comment_threads (
            workspace_id,
            doc_id,
            section_id,
            start_offset_utf16,
            end_offset_utf16,
            created_by_user_id
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING
            id,
            workspace_id,
            doc_id,
            section_id,
            start_offset_utf16,
            end_offset_utf16,
            status,
            version,
            created_by_user_id,
            created_by_agent_id,
            created_at,
            resolved_at
        "#,
    )
    .bind(workspace_id)
    .bind(doc_id)
    .bind(anchor.section_id)
    .bind(anchor.start_offset_utf16)
    .bind(anchor.end_offset_utf16)
    .bind(created_by_user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    let message_row = sqlx::query_as::<_, CommentMessageRow>(
        r#"
        INSERT INTO comment_messages (thread_id, author_user_id, body_md)
        VALUES ($1, $2, $3)
        RETURNING
            id,
            thread_id,
            author_user_id,
            author_agent_id,
            body_md,
            created_at,
            edited_at
        "#,
    )
    .bind(thread_row.id)
    .bind(created_by_user_id)
    .bind(body_md)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_sqlx_error)?;

    tx.commit().await.map_err(|error| CommentsApiError::internal(error.into()))?;

    Ok((thread_row.into(), message_row.into()))
}

async fn list_threads_pg(
    pool: &PgPool,
    workspace_id: Uuid,
    doc_id: Uuid,
    status_filter: Option<CommentStatus>,
    limit: usize,
    cursor: Option<CommentCursor>,
) -> Result<(Vec<CommentThreadWithMessages>, Option<String>), CommentsApiError> {
    ensure_document_exists_pg(pool, workspace_id, doc_id).await?;

    let cursor_created_at = cursor.as_ref().map(|value| value.created_at);
    let cursor_id = cursor.as_ref().map(|value| value.id);
    let status_value = status_filter.map(CommentStatus::as_str);

    let mut thread_rows = sqlx::query_as::<_, CommentThreadRow>(
        r#"
        SELECT
            id,
            workspace_id,
            doc_id,
            section_id,
            start_offset_utf16,
            end_offset_utf16,
            status,
            version,
            created_by_user_id,
            created_by_agent_id,
            created_at,
            resolved_at
        FROM comment_threads
        WHERE workspace_id = $1
          AND doc_id = $2
          AND ($3::text IS NULL OR status = $3)
          AND (
            $4::timestamptz IS NULL
            OR created_at < $4
            OR (created_at = $4 AND id < $5)
          )
        ORDER BY created_at DESC, id DESC
        LIMIT $6
        "#,
    )
    .bind(workspace_id)
    .bind(doc_id)
    .bind(status_value)
    .bind(cursor_created_at)
    .bind(cursor_id)
    .bind((limit + 1) as i64)
    .fetch_all(pool)
    .await
    .map_err(map_sqlx_error)?;

    let next_cursor = paginate_comment_threads(&mut thread_rows, limit);
    if thread_rows.is_empty() {
        return Ok((Vec::new(), next_cursor));
    }

    let thread_ids: Vec<Uuid> = thread_rows.iter().map(|row| row.id).collect();
    let message_rows = sqlx::query_as::<_, CommentMessageRow>(
        r#"
        SELECT
            id,
            thread_id,
            author_user_id,
            author_agent_id,
            body_md,
            created_at,
            edited_at
        FROM comment_messages
        WHERE thread_id = ANY($1::uuid[])
        ORDER BY created_at ASC, id ASC
        "#,
    )
    .bind(&thread_ids)
    .fetch_all(pool)
    .await
    .map_err(map_sqlx_error)?;

    let mut messages_by_thread: HashMap<Uuid, Vec<CommentMessage>> = HashMap::new();
    for message_row in message_rows {
        messages_by_thread.entry(message_row.thread_id).or_default().push(message_row.into());
    }

    let mut items = Vec::with_capacity(thread_rows.len());
    for row in thread_rows {
        items.push(CommentThreadWithMessages {
            messages: messages_by_thread.remove(&row.id).unwrap_or_default(),
            thread: row.into(),
        });
    }

    Ok((items, next_cursor))
}

async fn get_thread_with_messages_pg(
    pool: &PgPool,
    workspace_id: Uuid,
    thread_id: Uuid,
) -> Result<CommentThreadWithMessages, CommentsApiError> {
    let thread_row = sqlx::query_as::<_, CommentThreadRow>(
        r#"
        SELECT
            id,
            workspace_id,
            doc_id,
            section_id,
            start_offset_utf16,
            end_offset_utf16,
            status,
            version,
            created_by_user_id,
            created_by_agent_id,
            created_at,
            resolved_at
        FROM comment_threads
        WHERE id = $1
          AND workspace_id = $2
        "#,
    )
    .bind(thread_id)
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?
    .ok_or(CommentsApiError::NotFound { message: "comment thread not found" })?;

    let message_rows = sqlx::query_as::<_, CommentMessageRow>(
        r#"
        SELECT
            id,
            thread_id,
            author_user_id,
            author_agent_id,
            body_md,
            created_at,
            edited_at
        FROM comment_messages
        WHERE thread_id = $1
        ORDER BY created_at ASC, id ASC
        "#,
    )
    .bind(thread_id)
    .fetch_all(pool)
    .await
    .map_err(map_sqlx_error)?;

    Ok(CommentThreadWithMessages {
        thread: thread_row.into(),
        messages: message_rows.into_iter().map(CommentMessage::from).collect(),
    })
}

async fn create_message_pg(
    pool: &PgPool,
    workspace_id: Uuid,
    thread_id: Uuid,
    author_user_id: Uuid,
    body_md: String,
) -> Result<CommentMessage, CommentsApiError> {
    let row = sqlx::query_as::<_, CommentMessageRow>(
        r#"
        INSERT INTO comment_messages (thread_id, author_user_id, body_md)
        SELECT id, $3, $4
        FROM comment_threads
        WHERE id = $1
          AND workspace_id = $2
          AND status = 'open'
        RETURNING
            id,
            thread_id,
            author_user_id,
            author_agent_id,
            body_md,
            created_at,
            edited_at
        "#,
    )
    .bind(thread_id)
    .bind(workspace_id)
    .bind(author_user_id)
    .bind(body_md)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?;

    if let Some(row) = row {
        return Ok(row.into());
    }

    let status = sqlx::query_scalar::<_, String>(
        r#"
        SELECT status
        FROM comment_threads
        WHERE id = $1
          AND workspace_id = $2
        "#,
    )
    .bind(thread_id)
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?;

    match status.as_deref() {
        None => Err(CommentsApiError::NotFound { message: "comment thread not found" }),
        Some("resolved") => Err(CommentsApiError::bad_request("comment thread is resolved")),
        Some("open") => Err(CommentsApiError::internal(anyhow::anyhow!(
            "failed to insert comment message for open thread"
        ))),
        Some(other) => Err(CommentsApiError::internal(anyhow::anyhow!(
            "invalid comment thread status '{other}'"
        ))),
    }
}

async fn set_thread_status_pg(
    pool: &PgPool,
    workspace_id: Uuid,
    thread_id: Uuid,
    if_version: i32,
    next_status: CommentStatus,
) -> Result<CommentThread, CommentsApiError> {
    let expected_status = next_status.required_current_status();
    let row = sqlx::query_as::<_, CommentThreadRow>(
        r#"
        UPDATE comment_threads
        SET
            status = $3,
            version = version + 1,
            resolved_at = CASE
                WHEN $3 = 'resolved' THEN now()
                ELSE NULL
            END
        WHERE id = $1
          AND workspace_id = $2
          AND version = $4
          AND status = $5
        RETURNING
            id,
            workspace_id,
            doc_id,
            section_id,
            start_offset_utf16,
            end_offset_utf16,
            status,
            version,
            created_by_user_id,
            created_by_agent_id,
            created_at,
            resolved_at
        "#,
    )
    .bind(thread_id)
    .bind(workspace_id)
    .bind(next_status.as_str())
    .bind(if_version)
    .bind(expected_status.as_str())
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?;

    if let Some(row) = row {
        return Ok(row.into());
    }

    let thread_metadata = sqlx::query_as::<_, CommentThreadStatusRow>(
        r#"
        SELECT version, status
        FROM comment_threads
        WHERE id = $1
          AND workspace_id = $2
        "#,
    )
    .bind(thread_id)
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?;

    let Some(thread_metadata) = thread_metadata else {
        return Err(CommentsApiError::NotFound { message: "comment thread not found" });
    };

    if thread_metadata.version != if_version {
        return Err(CommentsApiError::PreconditionFailed);
    }

    let current_status =
        CommentStatus::parse(thread_metadata.status.as_str()).ok_or_else(|| {
            CommentsApiError::internal(anyhow::anyhow!(
                "invalid comment thread status '{}' in database",
                thread_metadata.status
            ))
        })?;

    if current_status != expected_status {
        return Err(CommentsApiError::bad_request(
            next_status.transition_error_message(current_status),
        ));
    }

    Err(CommentsApiError::internal(anyhow::anyhow!(
        "failed to transition thread status without a matching row"
    )))
}

async fn workspace_role_for_user_pg(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<Option<WorkspaceRole>, CommentsApiError> {
    let role = sqlx::query_scalar::<_, String>(
        r#"
        SELECT role
        FROM workspace_members
        WHERE workspace_id = $1
          AND user_id = $2
          AND status = 'active'
        LIMIT 1
        "#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(map_sqlx_error)?;

    role.map(|value| {
        WorkspaceRole::from_db_value(&value).ok_or_else(|| {
            CommentsApiError::internal(anyhow::anyhow!(
                "invalid workspace role '{value}' in database"
            ))
        })
    })
    .transpose()
}

// ── In-memory store ──────────────────────────────────────────────────────────

async fn create_thread_mem(
    store: &RwLock<MemoryCommentStore>,
    workspace_id: Uuid,
    doc_id: Uuid,
    created_by_user_id: Uuid,
    anchor: CommentAnchorRequest,
    body_md: String,
) -> Result<(CommentThread, CommentMessage), CommentsApiError> {
    let mut store = store.write().await;
    let now = Utc::now();
    let thread_id = Uuid::new_v4();

    let thread = MemoryCommentThread {
        id: thread_id,
        workspace_id,
        doc_id,
        section_id: anchor.section_id,
        start_offset_utf16: anchor.start_offset_utf16,
        end_offset_utf16: anchor.end_offset_utf16,
        status: CommentStatus::Open.as_str().to_string(),
        version: 1,
        created_by_user_id: Some(created_by_user_id),
        created_by_agent_id: None,
        created_at: now,
        resolved_at: None,
    };

    let message = MemoryCommentMessage {
        id: Uuid::new_v4(),
        thread_id,
        author_user_id: Some(created_by_user_id),
        author_agent_id: None,
        body_md,
        created_at: now,
        edited_at: None,
    };

    let response_thread = mem_thread_to_public(&thread);
    let response_message = mem_message_to_public(&message);

    store.threads.insert(thread.id, thread);
    store.messages.insert(thread_id, vec![message]);

    Ok((response_thread, response_message))
}

async fn list_threads_mem(
    store: &RwLock<MemoryCommentStore>,
    workspace_id: Uuid,
    doc_id: Uuid,
    status_filter: Option<CommentStatus>,
    limit: usize,
    _cursor: Option<CommentCursor>,
) -> Result<(Vec<CommentThreadWithMessages>, Option<String>), CommentsApiError> {
    let store = store.read().await;

    let mut thread_rows: Vec<MemoryCommentThread> = store
        .threads
        .values()
        .filter(|thread| thread.workspace_id == workspace_id && thread.doc_id == doc_id)
        .filter(|thread| match status_filter {
            Some(status) => thread.status == status.as_str(),
            None => true,
        })
        .cloned()
        .collect();

    thread_rows
        .sort_by(|left, right| right.created_at.cmp(&left.created_at).then(right.id.cmp(&left.id)));

    let next_cursor = paginate_memory_threads(&mut thread_rows, limit);
    let mut items = Vec::with_capacity(thread_rows.len());
    for thread in thread_rows {
        let messages = store
            .messages
            .get(&thread.id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|message| mem_message_to_public(&message))
            .collect();

        items.push(CommentThreadWithMessages { thread: mem_thread_to_public(&thread), messages });
    }

    Ok((items, next_cursor))
}

async fn create_message_mem(
    store: &RwLock<MemoryCommentStore>,
    workspace_id: Uuid,
    thread_id: Uuid,
    author_user_id: Uuid,
    body_md: String,
) -> Result<CommentMessage, CommentsApiError> {
    let mut store = store.write().await;
    let Some(thread) = store.threads.get(&thread_id) else {
        return Err(CommentsApiError::NotFound { message: "comment thread not found" });
    };
    if thread.workspace_id != workspace_id {
        return Err(CommentsApiError::NotFound { message: "comment thread not found" });
    }
    let status = CommentStatus::parse(thread.status.as_str()).ok_or_else(|| {
        CommentsApiError::internal(anyhow::anyhow!(
            "invalid comment thread status '{}' in memory store",
            thread.status
        ))
    })?;
    if status != CommentStatus::Open {
        return Err(CommentsApiError::bad_request("comment thread is resolved"));
    }

    let message = MemoryCommentMessage {
        id: Uuid::new_v4(),
        thread_id,
        author_user_id: Some(author_user_id),
        author_agent_id: None,
        body_md,
        created_at: Utc::now(),
        edited_at: None,
    };
    let response = mem_message_to_public(&message);
    store.messages.entry(thread_id).or_default().push(message);

    Ok(response)
}

async fn get_thread_with_messages_mem(
    store: &RwLock<MemoryCommentStore>,
    workspace_id: Uuid,
    thread_id: Uuid,
) -> Result<CommentThreadWithMessages, CommentsApiError> {
    let store = store.read().await;
    let thread = store
        .threads
        .get(&thread_id)
        .ok_or(CommentsApiError::NotFound { message: "comment thread not found" })?;
    if thread.workspace_id != workspace_id {
        return Err(CommentsApiError::NotFound { message: "comment thread not found" });
    }

    let messages = store
        .messages
        .get(&thread_id)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|message| mem_message_to_public(&message))
        .collect();

    Ok(CommentThreadWithMessages { thread: mem_thread_to_public(thread), messages })
}

async fn set_thread_status_mem(
    store: &RwLock<MemoryCommentStore>,
    workspace_id: Uuid,
    thread_id: Uuid,
    if_version: i32,
    next_status: CommentStatus,
) -> Result<CommentThread, CommentsApiError> {
    let mut store = store.write().await;
    let thread = store
        .threads
        .get_mut(&thread_id)
        .ok_or(CommentsApiError::NotFound { message: "comment thread not found" })?;
    if thread.workspace_id != workspace_id {
        return Err(CommentsApiError::NotFound { message: "comment thread not found" });
    }
    if thread.version != if_version {
        return Err(CommentsApiError::PreconditionFailed);
    }
    let current_status = CommentStatus::parse(thread.status.as_str()).ok_or_else(|| {
        CommentsApiError::internal(anyhow::anyhow!(
            "invalid comment thread status '{}' in memory store",
            thread.status
        ))
    })?;
    let expected_status = next_status.required_current_status();
    if current_status != expected_status {
        return Err(CommentsApiError::bad_request(
            next_status.transition_error_message(current_status),
        ));
    }

    thread.status = next_status.as_str().to_string();
    thread.version += 1;
    thread.resolved_at =
        if next_status == CommentStatus::Resolved { Some(Utc::now()) } else { None };

    Ok(mem_thread_to_public(thread))
}

async fn workspace_role_for_user_mem(
    store: &RwLock<MemoryCommentStore>,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<Option<WorkspaceRole>, CommentsApiError> {
    let store = store.read().await;
    Ok(store.workspace_members.get(&(workspace_id, user_id)).copied())
}

fn mem_thread_to_public(value: &MemoryCommentThread) -> CommentThread {
    CommentThread {
        id: value.id,
        workspace_id: value.workspace_id,
        doc_id: value.doc_id,
        section_id: value.section_id.clone(),
        start_offset_utf16: value.start_offset_utf16,
        end_offset_utf16: value.end_offset_utf16,
        status: value.status.clone(),
        version: value.version,
        created_by_user_id: value.created_by_user_id,
        created_by_agent_id: value.created_by_agent_id.clone(),
        created_at: value.created_at,
        resolved_at: value.resolved_at,
    }
}

fn mem_message_to_public(value: &MemoryCommentMessage) -> CommentMessage {
    CommentMessage {
        id: value.id,
        thread_id: value.thread_id,
        author_user_id: value.author_user_id,
        author_agent_id: value.author_agent_id.clone(),
        body_md: value.body_md.clone(),
        created_at: value.created_at,
        edited_at: value.edited_at,
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommentStatus {
    Open,
    Resolved,
}

impl CommentStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Resolved => "resolved",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "open" => Some(Self::Open),
            "resolved" => Some(Self::Resolved),
            _ => None,
        }
    }

    fn required_current_status(self) -> Self {
        match self {
            Self::Resolved => Self::Open,
            Self::Open => Self::Resolved,
        }
    }

    fn transition_error_message(self, current_status: Self) -> &'static str {
        match (self, current_status) {
            (Self::Resolved, Self::Resolved) => "comment thread is already resolved",
            (Self::Open, Self::Open) => "comment thread is already open",
            (Self::Resolved, Self::Open) | (Self::Open, Self::Resolved) => {
                "comment thread cannot transition to requested status"
            }
        }
    }
}

#[derive(Debug, Clone)]
struct CommentCursor {
    created_at: DateTime<Utc>,
    id: Uuid,
}

fn normalize_limit(limit: Option<usize>) -> usize {
    match limit {
        Some(0) => DEFAULT_PAGE_SIZE,
        Some(value) => value.min(MAX_PAGE_SIZE),
        None => DEFAULT_PAGE_SIZE,
    }
}

fn parse_status_filter(value: Option<&str>) -> Result<Option<CommentStatus>, CommentsApiError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }

    CommentStatus::parse(value)
        .map(Some)
        .ok_or_else(|| CommentsApiError::bad_request("status must be one of: open, resolved"))
}

fn parse_cursor(value: &str) -> Result<CommentCursor, CommentsApiError> {
    let (ts, id) = value
        .split_once('|')
        .ok_or_else(|| CommentsApiError::bad_request("cursor format is invalid"))?;

    let ts = ts
        .parse::<i64>()
        .map_err(|_| CommentsApiError::bad_request("cursor timestamp is invalid"))?;
    let created_at = DateTime::<Utc>::from_timestamp_micros(ts)
        .ok_or_else(|| CommentsApiError::bad_request("cursor timestamp is invalid"))?;
    let id =
        Uuid::parse_str(id).map_err(|_| CommentsApiError::bad_request("cursor id is invalid"))?;

    Ok(CommentCursor { created_at, id })
}

fn encode_cursor(thread: &CommentThreadRow) -> String {
    format!("{}|{}", thread.created_at.timestamp_micros(), thread.id)
}

fn encode_memory_cursor(thread: &MemoryCommentThread) -> String {
    format!("{}|{}", thread.created_at.timestamp_micros(), thread.id)
}

fn paginate_comment_threads(items: &mut Vec<CommentThreadRow>, limit: usize) -> Option<String> {
    if items.len() > limit {
        items.truncate(limit);
        items.last().map(encode_cursor)
    } else {
        None
    }
}

fn paginate_memory_threads(items: &mut Vec<MemoryCommentThread>, limit: usize) -> Option<String> {
    if items.len() > limit {
        items.truncate(limit);
        items.last().map(encode_memory_cursor)
    } else {
        None
    }
}

fn validate_anchor(anchor: &CommentAnchorRequest) -> Result<(), CommentsApiError> {
    if let Some(section_id) = anchor.section_id.as_deref() {
        if section_id.trim().is_empty() {
            return Err(CommentsApiError::bad_request(
                "anchor.section_id must not be empty when provided",
            ));
        }
    }

    match (anchor.start_offset_utf16, anchor.end_offset_utf16) {
        (None, None) => {}
        (Some(start), Some(end)) => {
            if start < 0 || end < 0 {
                return Err(CommentsApiError::bad_request(
                    "anchor offsets must be positive integers",
                ));
            }
            if end < start {
                return Err(CommentsApiError::bad_request(
                    "anchor.end_offset_utf16 must be >= anchor.start_offset_utf16",
                ));
            }
        }
        _ => {
            return Err(CommentsApiError::bad_request(
                "anchor.start_offset_utf16 and anchor.end_offset_utf16 must both be set or both be omitted",
            ));
        }
    }

    if anchor.head_seq.is_some_and(|value| value < 0) {
        return Err(CommentsApiError::bad_request("anchor.head_seq must be >= 0 when provided"));
    }

    Ok(())
}

fn validate_markdown_body(field_name: &str, value: &str) -> Result<(), CommentsApiError> {
    if value.trim().is_empty() {
        return Err(CommentsApiError::bad_request(format!("{field_name} must not be empty")));
    }
    Ok(())
}

fn map_sqlx_error(error: sqlx::Error) -> CommentsApiError {
    CommentsApiError::internal(error.into())
}

async fn require_workspace_role(
    store: &CommentStore,
    user: &AuthenticatedUser,
    workspace_id: Uuid,
    required_role: WorkspaceRole,
) -> Result<(), CommentsApiError> {
    if user.workspace_id != workspace_id {
        return Err(CommentsApiError::Forbidden);
    }

    let role = store.workspace_role_for_user(user.user_id, workspace_id).await?;
    let Some(role) = role else {
        return Err(CommentsApiError::Forbidden);
    };
    if !role.allows(required_role) {
        return Err(CommentsApiError::Forbidden);
    }

    Ok(())
}

async fn try_record_comment_audit_event(
    state: &CommentsApiState,
    workspace_id: Uuid,
    actor_user_id: Uuid,
    thread_id: Uuid,
    action: &'static str,
    details: serde_json::Value,
) {
    let Some(pool) = state.store.postgres_pool() else {
        return;
    };

    let event = NewAuditEvent {
        workspace_id: Some(workspace_id),
        actor_user_id: Some(actor_user_id),
        actor_agent_id: None,
        event_type: AuditEventType::AdminAction,
        entity_type: "comment_thread".to_owned(),
        entity_id: thread_id.to_string(),
        request_id: current_request_id(),
        ip_address: None,
        user_agent: None,
        details: Some(json!({
            "action": action,
            "comment_thread": details,
        })),
    };

    if let Err(error) = audit::record_event(pool, event).await {
        tracing::warn!(error = ?error, "failed to record comment audit event");
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

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

    fn test_store() -> CommentStore {
        CommentStore::Memory(Arc::new(RwLock::new(MemoryCommentStore::default())))
    }

    fn test_app() -> (Router, CommentStore) {
        let store = test_store();
        let app = build_router_with_store(store.clone(), test_jwt_service());
        (app, store)
    }

    async fn grant_workspace_role(
        store: &CommentStore,
        workspace_id: Uuid,
        user_id: Uuid,
        role: WorkspaceRole,
    ) {
        store.grant_workspace_role_for_tests(workspace_id, user_id, role).await;
    }

    fn auth_token(jwt: &JwtAccessTokenService, user_id: Uuid, workspace_id: Uuid) -> String {
        jwt.issue_workspace_token(user_id, workspace_id).expect("token should be issued")
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
            .body(Body::from(serde_json::to_vec(&body).expect("request json body")))
            .expect("request should build")
    }

    fn get_request(uri: &str, token: &str) -> Request<Body> {
        Request::builder()
            .method("GET")
            .uri(uri)
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .expect("request should build")
    }

    async fn body_json(response: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("response body should be readable");
        serde_json::from_slice(&bytes).expect("response body should be valid json")
    }

    #[tokio::test]
    async fn create_list_resolve_and_filter_comments() {
        let (app, store) = test_app();
        let jwt = test_jwt_service();
        let ws_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let token = auth_token(&jwt, user_id, ws_id);
        grant_workspace_role(&store, ws_id, user_id, WorkspaceRole::Editor).await;

        let create_response = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents/{doc_id}/comments"),
                serde_json::json!({
                    "anchor": {
                        "section_id": "h2:authentication",
                        "start_offset_utf16": 12,
                        "end_offset_utf16": 24,
                        "head_seq": 3
                    },
                    "message": "Should we use PKCE here?"
                }),
                &token,
            ))
            .await
            .expect("create request should return response");
        assert_eq!(create_response.status(), StatusCode::CREATED);

        let create_body = body_json(create_response).await;
        let thread_id =
            create_body["thread"]["id"].as_str().expect("thread id should be present").to_string();
        assert_eq!(create_body["thread"]["status"], "open");
        assert_eq!(create_body["thread"]["version"], 1);
        assert_eq!(create_body["messages"].as_array().expect("messages should be array").len(), 1);

        let list_open_response = app
            .clone()
            .oneshot(get_request(
                &format!("/v1/workspaces/{ws_id}/documents/{doc_id}/comments?status=open"),
                &token,
            ))
            .await
            .expect("list open request should return response");
        assert_eq!(list_open_response.status(), StatusCode::OK);

        let list_open_body = body_json(list_open_response).await;
        assert_eq!(list_open_body["items"].as_array().expect("items should be array").len(), 1);
        assert_eq!(
            list_open_body["items"][0]["messages"]
                .as_array()
                .expect("messages should be array")
                .len(),
            1
        );

        let resolve_response = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/comments/{thread_id}/resolve"),
                serde_json::json!({ "if_version": 1 }),
                &token,
            ))
            .await
            .expect("resolve request should return response");
        assert_eq!(resolve_response.status(), StatusCode::OK);
        let resolve_body = body_json(resolve_response).await;
        assert_eq!(resolve_body["thread"]["status"], "resolved");
        assert_eq!(resolve_body["thread"]["version"], 2);

        let list_open_response = app
            .clone()
            .oneshot(get_request(
                &format!("/v1/workspaces/{ws_id}/documents/{doc_id}/comments?status=open"),
                &token,
            ))
            .await
            .expect("list open request should return response");
        let list_open_body = body_json(list_open_response).await;
        assert_eq!(list_open_body["items"].as_array().expect("items should be array").len(), 0);

        let list_resolved_response = app
            .oneshot(get_request(
                &format!("/v1/workspaces/{ws_id}/documents/{doc_id}/comments?status=resolved"),
                &token,
            ))
            .await
            .expect("list resolved request should return response");
        let list_resolved_body = body_json(list_resolved_response).await;
        assert_eq!(list_resolved_body["items"].as_array().expect("items should be array").len(), 1);
    }

    #[tokio::test]
    async fn reply_and_reopen_comment_thread() {
        let (app, store) = test_app();
        let jwt = test_jwt_service();
        let ws_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let token = auth_token(&jwt, user_id, ws_id);
        grant_workspace_role(&store, ws_id, user_id, WorkspaceRole::Editor).await;

        let create_response = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents/{doc_id}/comments"),
                serde_json::json!({
                    "anchor": {
                        "section_id": "h1:intro",
                        "start_offset_utf16": 0,
                        "end_offset_utf16": 8,
                        "head_seq": 1
                    },
                    "message": "Initial comment"
                }),
                &token,
            ))
            .await
            .expect("create request should return response");
        let create_body = body_json(create_response).await;
        let thread_id =
            create_body["thread"]["id"].as_str().expect("thread id should be present").to_string();
        assert_eq!(create_body["messages"].as_array().expect("messages should be array").len(), 1);

        let reply_response = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/comments/{thread_id}/messages"),
                serde_json::json!({ "body_md": "Follow-up" }),
                &token,
            ))
            .await
            .expect("reply request should return response");
        assert_eq!(reply_response.status(), StatusCode::OK);
        let reply_body = body_json(reply_response).await;
        assert_eq!(reply_body["thread"]["id"], thread_id);
        assert_eq!(reply_body["messages"].as_array().expect("messages should be array").len(), 2);
        assert_eq!(reply_body["messages"][1]["body_md"], "Follow-up");

        let resolve_response = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/comments/{thread_id}/resolve"),
                serde_json::json!({ "if_version": 1 }),
                &token,
            ))
            .await
            .expect("resolve request should return response");
        let resolve_body = body_json(resolve_response).await;
        assert_eq!(resolve_body["thread"]["version"], 2);

        let reopen_response = app
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/comments/{thread_id}/reopen"),
                serde_json::json!({ "if_version": 2 }),
                &token,
            ))
            .await
            .expect("reopen request should return response");
        assert_eq!(reopen_response.status(), StatusCode::OK);
        let reopen_body = body_json(reopen_response).await;
        assert_eq!(reopen_body["thread"]["status"], "open");
        assert_eq!(reopen_body["thread"]["version"], 3);
        assert!(reopen_body["thread"]["resolved_at"].is_null());
    }

    #[tokio::test]
    async fn resolve_with_stale_if_version_returns_412() {
        let (app, store) = test_app();
        let jwt = test_jwt_service();
        let ws_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let token = auth_token(&jwt, user_id, ws_id);
        grant_workspace_role(&store, ws_id, user_id, WorkspaceRole::Editor).await;

        let create_response = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents/{doc_id}/comments"),
                serde_json::json!({
                    "anchor": {
                        "section_id": "h2:stale",
                        "start_offset_utf16": 1,
                        "end_offset_utf16": 2,
                        "head_seq": 1
                    },
                    "message": "Versioned"
                }),
                &token,
            ))
            .await
            .expect("create request should return response");
        let create_body = body_json(create_response).await;
        let thread_id =
            create_body["thread"]["id"].as_str().expect("thread id should be present").to_string();

        let resolve_response = app
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/comments/{thread_id}/resolve"),
                serde_json::json!({ "if_version": 9 }),
                &token,
            ))
            .await
            .expect("resolve request should return response");

        assert_eq!(resolve_response.status(), StatusCode::PRECONDITION_FAILED);
    }

    #[tokio::test]
    async fn viewer_role_can_list_but_cannot_mutate_comment_threads() {
        let (app, store) = test_app();
        let jwt = test_jwt_service();
        let ws_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        let owner_id = Uuid::new_v4();
        let viewer_id = Uuid::new_v4();
        let owner_token = auth_token(&jwt, owner_id, ws_id);
        let viewer_token = auth_token(&jwt, viewer_id, ws_id);
        grant_workspace_role(&store, ws_id, owner_id, WorkspaceRole::Owner).await;
        grant_workspace_role(&store, ws_id, viewer_id, WorkspaceRole::Viewer).await;

        let create_response = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents/{doc_id}/comments"),
                serde_json::json!({
                    "anchor": { "section_id": "h1:rbac" },
                    "message": "Owner-created thread"
                }),
                &owner_token,
            ))
            .await
            .expect("owner create request should return response");
        assert_eq!(create_response.status(), StatusCode::CREATED);
        let create_body = body_json(create_response).await;
        let thread_id =
            create_body["thread"]["id"].as_str().expect("thread id should be present").to_string();

        let list_response = app
            .clone()
            .oneshot(get_request(
                &format!("/v1/workspaces/{ws_id}/documents/{doc_id}/comments"),
                &viewer_token,
            ))
            .await
            .expect("viewer list request should return response");
        assert_eq!(list_response.status(), StatusCode::OK);

        let create_forbidden = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents/{doc_id}/comments"),
                serde_json::json!({
                    "anchor": { "section_id": "h1:rbac" },
                    "message": "Viewer create attempt"
                }),
                &viewer_token,
            ))
            .await
            .expect("viewer create request should return response");
        assert_eq!(create_forbidden.status(), StatusCode::FORBIDDEN);

        let message_forbidden = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/comments/{thread_id}/messages"),
                serde_json::json!({ "body_md": "Viewer reply attempt" }),
                &viewer_token,
            ))
            .await
            .expect("viewer reply request should return response");
        assert_eq!(message_forbidden.status(), StatusCode::FORBIDDEN);

        let resolve_forbidden = app
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/comments/{thread_id}/resolve"),
                serde_json::json!({ "if_version": 1 }),
                &viewer_token,
            ))
            .await
            .expect("viewer resolve request should return response");
        assert_eq!(resolve_forbidden.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn status_transitions_enforce_open_and_resolved_guards() {
        let (app, store) = test_app();
        let jwt = test_jwt_service();
        let ws_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let token = auth_token(&jwt, user_id, ws_id);
        grant_workspace_role(&store, ws_id, user_id, WorkspaceRole::Editor).await;

        let create_response = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/documents/{doc_id}/comments"),
                serde_json::json!({
                    "anchor": { "section_id": "h2:guards" },
                    "message": "Transition guard"
                }),
                &token,
            ))
            .await
            .expect("create request should return response");
        let create_body = body_json(create_response).await;
        let thread_id =
            create_body["thread"]["id"].as_str().expect("thread id should be present").to_string();

        let reopen_open_response = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/comments/{thread_id}/reopen"),
                serde_json::json!({ "if_version": 1 }),
                &token,
            ))
            .await
            .expect("reopen request should return response");
        assert_eq!(reopen_open_response.status(), StatusCode::BAD_REQUEST);

        let resolve_response = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/comments/{thread_id}/resolve"),
                serde_json::json!({ "if_version": 1 }),
                &token,
            ))
            .await
            .expect("resolve request should return response");
        assert_eq!(resolve_response.status(), StatusCode::OK);

        let second_resolve_response = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/comments/{thread_id}/resolve"),
                serde_json::json!({ "if_version": 2 }),
                &token,
            ))
            .await
            .expect("second resolve request should return response");
        assert_eq!(second_resolve_response.status(), StatusCode::BAD_REQUEST);

        let reply_resolved_response = app
            .oneshot(json_request(
                "POST",
                &format!("/v1/workspaces/{ws_id}/comments/{thread_id}/messages"),
                serde_json::json!({ "body_md": "Resolved reply should fail" }),
                &token,
            ))
            .await
            .expect("reply request should return response");
        assert_eq!(reply_resolved_response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn unauthenticated_request_returns_401() {
        let (app, _) = test_app();
        let ws_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/workspaces/{ws_id}/documents/{doc_id}/comments"))
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should return response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
