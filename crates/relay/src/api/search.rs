// Full-text-ish search endpoint for workspace documents.
//
// Route:
//   GET /v1/workspaces/{id}/search?q=...&limit=...&cursor=...

use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Extension, Json, Path, Query, State},
    middleware,
    response::{IntoResponse, Response},
    routing::get,
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
        middleware::{require_bearer_auth, AuthenticatedUser, WorkspaceRole},
    },
    error::{ErrorCode, RelayError},
};

const DEFAULT_PAGE_SIZE: usize = 50;
const MAX_PAGE_SIZE: usize = 100;
const MAX_QUERY_LEN: usize = 256;

#[derive(Debug, Clone, Serialize)]
struct SearchItem {
    doc_id: Uuid,
    path: String,
    title: Option<String>,
    snippet: String,
    score: f32,
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    q: Option<String>,
    limit: Option<usize>,
    cursor: Option<String>,
}

#[derive(Debug, Serialize)]
struct SearchResponse {
    items: Vec<SearchItem>,
    next_cursor: Option<String>,
}

#[derive(Clone)]
struct SearchApiState {
    store: SearchStore,
}

#[derive(Clone)]
enum SearchStore {
    Postgres(PgPool),
    #[cfg_attr(not(test), allow(dead_code))]
    Memory(Arc<RwLock<MemorySearchStore>>),
}

#[derive(Default)]
struct MemorySearchStore {
    documents: HashMap<Uuid, MemoryDocument>,
    workspace_members: HashMap<(Uuid, Uuid), WorkspaceRole>,
}

#[derive(Clone)]
struct MemoryDocument {
    id: Uuid,
    workspace_id: Uuid,
    path: String,
    title: Option<String>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
}

#[derive(sqlx::FromRow)]
struct SearchRow {
    doc_id: Uuid,
    path: String,
    title: Option<String>,
    snippet: String,
    score: f32,
}

#[derive(Debug)]
enum SearchApiError {
    BadRequest { message: String },
    Forbidden,
    Internal(anyhow::Error),
}

impl SearchApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest { message: message.into() }
    }

    fn internal(error: anyhow::Error) -> Self {
        Self::Internal(error)
    }
}

impl IntoResponse for SearchApiError {
    fn into_response(self) -> Response {
        match self {
            Self::BadRequest { message } => {
                RelayError::new(ErrorCode::ValidationFailed, message).into_response()
            }
            Self::Forbidden => {
                RelayError::new(ErrorCode::AuthForbidden, "caller lacks required role")
                    .into_response()
            }
            Self::Internal(error) => {
                tracing::error!(error = ?error, "search api internal error");
                RelayError::from_code(ErrorCode::InternalError).into_response()
            }
        }
    }
}

pub fn router(pool: PgPool, jwt_service: Arc<JwtAccessTokenService>) -> Router {
    build_router_with_store(SearchStore::Postgres(pool), jwt_service)
}

fn build_router_with_store(store: SearchStore, jwt_service: Arc<JwtAccessTokenService>) -> Router {
    let state = SearchApiState { store };

    Router::new()
        .route("/v1/workspaces/{id}/search", get(search_documents))
        .with_state(state)
        .route_layer(middleware::from_fn_with_state(jwt_service, require_bearer_auth))
}

async fn search_documents(
    State(state): State<SearchApiState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(workspace_id): Path<Uuid>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, SearchApiError> {
    require_workspace_role(&state.store, &user, workspace_id, WorkspaceRole::Viewer).await?;

    let raw_query =
        query.q.ok_or_else(|| SearchApiError::bad_request("missing query parameter: q"))?;
    let q = validate_query(&raw_query)?;
    let limit = normalize_limit(query.limit);
    let offset = match query.cursor {
        Some(cursor) => parse_cursor(&cursor)?,
        None => 0,
    };

    let (items, next_cursor) = state.store.search(workspace_id, q, limit, offset).await?;
    Ok(Json(SearchResponse { items, next_cursor }))
}

impl SearchStore {
    async fn search(
        &self,
        workspace_id: Uuid,
        q: &str,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<SearchItem>, Option<String>), SearchApiError> {
        match self {
            Self::Postgres(pool) => search_pg(pool, workspace_id, q, limit, offset).await,
            Self::Memory(store) => search_mem(store, workspace_id, q, limit, offset).await,
        }
    }

    async fn workspace_role_for_user(
        &self,
        user_id: Uuid,
        workspace_id: Uuid,
    ) -> Result<Option<WorkspaceRole>, SearchApiError> {
        match self {
            Self::Postgres(pool) => workspace_role_for_user_pg(pool, user_id, workspace_id).await,
            Self::Memory(store) => workspace_role_for_user_mem(store, user_id, workspace_id).await,
        }
    }
}

async fn search_pg(
    pool: &PgPool,
    workspace_id: Uuid,
    q: &str,
    limit: usize,
    offset: usize,
) -> Result<(Vec<SearchItem>, Option<String>), SearchApiError> {
    let rows = sqlx::query_as::<_, SearchRow>(
        r#"
        SELECT
            id AS doc_id,
            path,
            title,
            COALESCE(
                NULLIF(
                    ts_headline(
                        'english',
                        COALESCE(title, path),
                        plainto_tsquery('english', $2),
                        'StartSel=, StopSel=, MaxWords=20, MinWords=5, ShortWord=2, HighlightAll=false'
                    ),
                    ''
                ),
                COALESCE(title, path),
                path
            ) AS snippet,
            ts_rank_cd(
                to_tsvector('english', COALESCE(title, '') || ' ' || path),
                plainto_tsquery('english', $2)
            ) AS score
        FROM documents
        WHERE workspace_id = $1
          AND deleted_at IS NULL
          AND to_tsvector('english', COALESCE(title, '') || ' ' || path)
              @@ plainto_tsquery('english', $2)
        ORDER BY score DESC, updated_at DESC, id DESC
        LIMIT $3
        OFFSET $4
        "#,
    )
    .bind(workspace_id)
    .bind(q)
    .bind((limit + 1) as i64)
    .bind(offset as i64)
    .fetch_all(pool)
    .await
    .map_err(map_sqlx_error)?;

    let mut items = rows
        .into_iter()
        .map(|row| SearchItem {
            doc_id: row.doc_id,
            path: row.path,
            title: row.title,
            snippet: row.snippet,
            score: row.score,
        })
        .collect::<Vec<_>>();

    let next_cursor = paginate(&mut items, limit, offset);
    Ok((items, next_cursor))
}

async fn search_mem(
    store: &RwLock<MemorySearchStore>,
    workspace_id: Uuid,
    q: &str,
    limit: usize,
    offset: usize,
) -> Result<(Vec<SearchItem>, Option<String>), SearchApiError> {
    let store = store.read().await;
    let query = q.to_lowercase();

    let mut scored = store
        .documents
        .values()
        .filter(|doc| doc.workspace_id == workspace_id && doc.deleted_at.is_none())
        .filter_map(|doc| {
            let path_norm = doc.path.to_lowercase();
            let title = doc.title.clone();
            let title_norm = title.as_deref().unwrap_or_default().to_lowercase();

            let title_hits = title_norm.matches(&query).count();
            let path_hits = path_norm.matches(&query).count();
            if title_hits == 0 && path_hits == 0 {
                return None;
            }

            let score = (title_hits as f32 * 2.0) + (path_hits as f32);
            let snippet = match title {
                Some(ref value) if value.to_lowercase().contains(&query) => value.clone(),
                _ => doc.path.clone(),
            };

            Some((
                SearchItem {
                    doc_id: doc.id,
                    path: doc.path.clone(),
                    title: doc.title.clone(),
                    snippet,
                    score,
                },
                doc.updated_at,
            ))
        })
        .collect::<Vec<_>>();

    scored.sort_by(|(left_item, left_updated_at), (right_item, right_updated_at)| {
        right_item
            .score
            .partial_cmp(&left_item.score)
            .unwrap_or(Ordering::Equal)
            .then(right_updated_at.cmp(left_updated_at))
            .then(right_item.doc_id.cmp(&left_item.doc_id))
    });

    let mut items = scored.into_iter().map(|(item, _)| item).collect::<Vec<_>>();
    let next_cursor = paginate_with_offset(&mut items, limit, offset);
    Ok((items, next_cursor))
}

async fn workspace_role_for_user_pg(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<Option<WorkspaceRole>, SearchApiError> {
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
            SearchApiError::internal(anyhow::anyhow!(
                "invalid workspace role '{value}' in database"
            ))
        })
    })
    .transpose()
}

async fn workspace_role_for_user_mem(
    store: &RwLock<MemorySearchStore>,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<Option<WorkspaceRole>, SearchApiError> {
    let store = store.read().await;
    Ok(store.workspace_members.get(&(workspace_id, user_id)).copied())
}

async fn require_workspace_role(
    store: &SearchStore,
    user: &AuthenticatedUser,
    workspace_id: Uuid,
    required_role: WorkspaceRole,
) -> Result<(), SearchApiError> {
    if user.workspace_id != workspace_id {
        return Err(SearchApiError::Forbidden);
    }

    let role = store.workspace_role_for_user(user.user_id, workspace_id).await?;
    let Some(role) = role else {
        return Err(SearchApiError::Forbidden);
    };
    if !role.allows(required_role) {
        return Err(SearchApiError::Forbidden);
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

fn validate_query(value: &str) -> Result<&str, SearchApiError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(SearchApiError::bad_request("q must not be empty"));
    }
    if trimmed.len() > MAX_QUERY_LEN {
        return Err(SearchApiError::bad_request(format!(
            "q must not exceed {MAX_QUERY_LEN} characters"
        )));
    }
    Ok(trimmed)
}

fn parse_cursor(value: &str) -> Result<usize, SearchApiError> {
    value.parse::<usize>().map_err(|_| SearchApiError::bad_request("cursor format is invalid"))
}

fn paginate(items: &mut Vec<SearchItem>, limit: usize, offset: usize) -> Option<String> {
    if items.len() > limit {
        items.truncate(limit);
        Some((offset + limit).to_string())
    } else {
        None
    }
}

fn paginate_with_offset(
    items: &mut Vec<SearchItem>,
    limit: usize,
    offset: usize,
) -> Option<String> {
    if offset >= items.len() {
        items.clear();
        return None;
    }

    let mut sliced = items.split_off(offset.min(items.len()));
    if sliced.len() > limit {
        sliced.truncate(limit);
        *items = sliced;
        Some((offset + limit).to_string())
    } else {
        *items = sliced;
        None
    }
}

fn map_sqlx_error(error: sqlx::Error) -> SearchApiError {
    SearchApiError::internal(error.into())
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use chrono::Duration;
    use tower::ServiceExt;

    use super::*;
    use crate::auth::jwt::JwtAccessTokenService;

    fn test_jwt_service() -> Arc<JwtAccessTokenService> {
        Arc::new(
            JwtAccessTokenService::new("test-secret-that-is-at-least-32-chars-long!!")
                .expect("jwt service"),
        )
    }

    fn auth_token(jwt: &JwtAccessTokenService, user_id: Uuid, workspace_id: Uuid) -> String {
        jwt.issue_workspace_token(user_id, workspace_id).expect("token")
    }

    fn grant_workspace_role(
        store: &mut MemorySearchStore,
        workspace_id: Uuid,
        user_id: Uuid,
        role: WorkspaceRole,
    ) {
        store.workspace_members.insert((workspace_id, user_id), role);
    }

    fn test_router(store: SearchStore) -> Router {
        build_router_with_store(store, test_jwt_service())
    }

    fn get_request(uri: &str, token: &str) -> Request<Body> {
        Request::builder()
            .method("GET")
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
    async fn search_returns_paginated_matches() {
        let ws_id = Uuid::new_v4();
        let now = Utc::now();
        let user_id = Uuid::new_v4();

        let mut mem = MemorySearchStore::default();
        let first_doc_id = Uuid::new_v4();
        let second_doc_id = Uuid::new_v4();
        mem.documents.insert(
            first_doc_id,
            MemoryDocument {
                id: first_doc_id,
                workspace_id: ws_id,
                path: "docs/authentication.md".to_string(),
                title: Some("Authentication Guide".to_string()),
                updated_at: now + Duration::seconds(1),
                deleted_at: None,
            },
        );
        mem.documents.insert(
            second_doc_id,
            MemoryDocument {
                id: second_doc_id,
                workspace_id: ws_id,
                path: "docs/authz.md".to_string(),
                title: Some("AuthZ Patterns".to_string()),
                updated_at: now,
                deleted_at: None,
            },
        );
        grant_workspace_role(&mut mem, ws_id, user_id, WorkspaceRole::Viewer);

        let app = test_router(SearchStore::Memory(Arc::new(RwLock::new(mem))));
        let jwt = test_jwt_service();
        let token = auth_token(&jwt, user_id, ws_id);

        let first_page = app
            .clone()
            .oneshot(get_request(&format!("/v1/workspaces/{ws_id}/search?q=auth&limit=1"), &token))
            .await
            .unwrap();
        assert_eq!(first_page.status(), StatusCode::OK);
        let first_page_body = body_json(first_page).await;
        assert_eq!(first_page_body["items"].as_array().unwrap().len(), 1);
        assert_eq!(first_page_body["items"][0]["doc_id"], first_doc_id.to_string());
        assert_eq!(first_page_body["next_cursor"], "1");

        let second_page = app
            .oneshot(get_request(
                &format!("/v1/workspaces/{ws_id}/search?q=auth&limit=1&cursor=1"),
                &token,
            ))
            .await
            .unwrap();
        assert_eq!(second_page.status(), StatusCode::OK);
        let second_page_body = body_json(second_page).await;
        assert_eq!(second_page_body["items"].as_array().unwrap().len(), 1);
        assert_eq!(second_page_body["items"][0]["doc_id"], second_doc_id.to_string());
        assert!(second_page_body["next_cursor"].is_null());
    }

    #[tokio::test]
    async fn search_rejects_empty_query() {
        let ws_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let mut mem = MemorySearchStore::default();
        grant_workspace_role(&mut mem, ws_id, user_id, WorkspaceRole::Viewer);
        let app = test_router(SearchStore::Memory(Arc::new(RwLock::new(mem))));
        let jwt = test_jwt_service();
        let token = auth_token(&jwt, user_id, ws_id);

        let response = app
            .oneshot(get_request(&format!("/v1/workspaces/{ws_id}/search?q=%20%20%20"), &token))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn search_forbids_cross_workspace_token() {
        let ws_id = Uuid::new_v4();
        let other_ws_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let mut mem = MemorySearchStore::default();
        grant_workspace_role(&mut mem, ws_id, user_id, WorkspaceRole::Viewer);
        let app = test_router(SearchStore::Memory(Arc::new(RwLock::new(mem))));
        let jwt = test_jwt_service();
        let token = auth_token(&jwt, user_id, other_ws_id);

        let response = app
            .oneshot(get_request(&format!("/v1/workspaces/{ws_id}/search?q=auth"), &token))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn search_forbids_non_members() {
        let ws_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let app =
            test_router(SearchStore::Memory(Arc::new(RwLock::new(MemorySearchStore::default()))));
        let jwt = test_jwt_service();
        let token = auth_token(&jwt, user_id, ws_id);

        let response = app
            .oneshot(get_request(&format!("/v1/workspaces/{ws_id}/search?q=auth"), &token))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn search_requires_authentication() {
        let app =
            test_router(SearchStore::Memory(Arc::new(RwLock::new(MemorySearchStore::default()))));
        let ws_id = Uuid::new_v4();

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/v1/workspaces/{ws_id}/search?q=auth"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
