use axum::{
    extract::{Extension, Json, Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use sqlx::types::chrono::Utc;
use uuid::Uuid;

use crate::{
    audit::AuditEventType,
    auth::middleware::AuthenticatedUser,
    validation::ValidatedJson,
};

use super::{
    extract_if_match, generate_share_link_token, hash_share_link_token, normalize_limit,
    parse_member_cursor, validate_invite_request, validate_member_update, ApiError, ApiState,
    CreateInviteRequest, InviteEnvelope, ListMembersQuery, MemberEnvelope, MemberRecord,
    MembersPageEnvelope, UpdateMemberRequest, DEFAULT_INVITE_EXPIRY_HOURS, try_record_audit_event,
};

pub(super) async fn list_members(
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

pub(super) async fn update_member(
    State(state): State<ApiState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((workspace_id, member_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    ValidatedJson(payload): ValidatedJson<UpdateMemberRequest>,
) -> Result<Json<MemberEnvelope>, ApiError> {
    validate_member_update(&payload)?;
    let audit_role = payload.role.clone();
    let audit_status = payload.status.clone();

    if member_id == user.user_id {
        return Err(ApiError::bad_request("VALIDATION_ERROR", "cannot modify your own membership"));
    }

    let if_match = extract_if_match(&headers)?;
    let record = state.store.update_member(workspace_id, member_id, if_match, payload).await?;
    try_record_audit_event(
        &state,
        Some(workspace_id),
        Some(user.user_id),
        AuditEventType::PermissionChange,
        "membership",
        member_id.to_string(),
        Some(serde_json::json!({
            "action": "update",
            "role": audit_role,
            "status": audit_status,
        })),
    )
    .await;

    Ok(Json(MemberEnvelope { member: record.into_member() }))
}

pub(super) async fn delete_member(
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
    try_record_audit_event(
        &state,
        Some(workspace_id),
        Some(user.user_id),
        AuditEventType::PermissionChange,
        "membership",
        member_id.to_string(),
        Some(serde_json::json!({ "action": "delete" })),
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn create_invite(
    State(state): State<ApiState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(workspace_id): Path<Uuid>,
    ValidatedJson(payload): ValidatedJson<CreateInviteRequest>,
) -> Result<(StatusCode, Json<InviteEnvelope>), ApiError> {
    validate_invite_request(&payload)?;
    let audit_email = payload.email.clone();
    let audit_role = payload.role.clone();
    let audit_expiry_hours = payload.expires_in_hours;

    let expiry_hours = payload.expires_in_hours.unwrap_or(DEFAULT_INVITE_EXPIRY_HOURS);
    let expires_at = Utc::now() + chrono::Duration::hours(i64::from(expiry_hours));

    let token = generate_share_link_token();
    let token_hash = hash_share_link_token(&token);

    let record = state
        .store
        .create_invite(workspace_id, user.user_id, payload, token_hash, expires_at)
        .await?;
    try_record_audit_event(
        &state,
        Some(workspace_id),
        Some(user.user_id),
        AuditEventType::PermissionChange,
        "invite",
        record.id.to_string(),
        Some(serde_json::json!({
            "action": "create",
            "email": audit_email,
            "role": audit_role,
            "expires_in_hours": audit_expiry_hours,
        })),
    )
    .await;

    let invite = record.into_invite();
    let envelope = InviteEnvelope { invite };

    Ok((StatusCode::CREATED, Json(envelope)))
}

pub(super) async fn accept_invite(
    State(state): State<ApiState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(token): Path<String>,
) -> Result<Json<InviteEnvelope>, ApiError> {
    let token_hash = hash_share_link_token(&token);

    let record = state.store.accept_invite(token_hash, user.user_id).await?;
    try_record_audit_event(
        &state,
        Some(record.workspace_id),
        Some(user.user_id),
        AuditEventType::PermissionChange,
        "invite",
        record.id.to_string(),
        Some(serde_json::json!({ "action": "accept" })),
    )
    .await;

    Ok(Json(InviteEnvelope { invite: record.into_invite() }))
}
