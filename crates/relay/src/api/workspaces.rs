use axum::{
    extract::{Extension, Json, Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use uuid::Uuid;

use crate::auth::middleware::AuthenticatedUser;

use super::{
    extract_if_match, normalize_limit, parse_cursor, validate_name, validate_slug, ApiError,
    ApiState, CreateWorkspaceRequest, ListWorkspacesQuery, UpdateWorkspaceRequest,
    WorkspaceEnvelope, WorkspaceRecord, WorkspacesPageEnvelope,
};

pub(super) async fn create_workspace(
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

pub(super) async fn list_workspaces(
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

pub(super) async fn get_workspace(
    State(state): State<ApiState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<WorkspaceEnvelope>, ApiError> {
    let workspace = state.store.get_workspace(user.user_id, workspace_id).await?.into_workspace();

    Ok(Json(WorkspaceEnvelope { workspace }))
}

pub(super) async fn update_workspace(
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
