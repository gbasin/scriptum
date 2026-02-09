use std::collections::BTreeSet;

const API_MOD_SOURCE: &str = include_str!("../src/api/mod.rs");
const AUTH_SOURCE: &str = include_str!("../src/auth/oauth.rs");
const DOCUMENTS_SOURCE: &str = include_str!("../src/api/documents.rs");
const COMMENTS_SOURCE: &str = include_str!("../src/api/comments.rs");
const MEMBERS_SOURCE: &str = include_str!("../src/api/members.rs");
const SEARCH_SOURCE: &str = include_str!("../src/api/search.rs");
const WORKSPACES_SOURCE: &str = include_str!("../src/api/workspaces.rs");
const WS_SOURCE: &str = include_str!("../src/ws/mod.rs");
const WS_HANDLER_SOURCE: &str = include_str!("../src/ws/handler.rs");

#[test]
fn rest_contract_declares_part3_endpoint_matrix() {
    let expected_paths = [
        "/v1/auth/oauth/github/start",
        "/v1/auth/oauth/github/callback",
        "/v1/auth/token/refresh",
        "/v1/auth/logout",
        "/v1/workspaces",
        "/v1/workspaces/{id}",
        "/v1/workspaces/{workspace_id}/members",
        "/v1/workspaces/{workspace_id}/members/{member_id}",
        "/v1/workspaces/{workspace_id}/invites",
        "/v1/invites/{token}/accept",
        "/v1/workspaces/{id}/share-links",
        "/v1/workspaces/{id}/share-links/{share_link_id}",
        "/v1/share-links/redeem",
        "/v1/workspaces/{ws_id}/documents",
        "/v1/workspaces/{ws_id}/documents/{doc_id}",
        "/v1/workspaces/{ws_id}/documents/{doc_id}/tags",
        "/v1/workspaces/{ws_id}/documents/{doc_id}/acl-overrides",
        "/v1/workspaces/{ws_id}/documents/{doc_id}/acl-overrides/{override_id}",
        "/v1/workspaces/{ws_id}/documents/{doc_id}/comments",
        "/v1/workspaces/{ws_id}/comments/{thread_id}/messages",
        "/v1/workspaces/{ws_id}/comments/{thread_id}/resolve",
        "/v1/workspaces/{ws_id}/comments/{thread_id}/reopen",
        "/v1/workspaces/{id}/search",
        "/v1/workspaces/{workspace_id}/sync-sessions",
        "/v1/ws/{session_id}",
    ];

    let contract_surface = [
        API_MOD_SOURCE,
        AUTH_SOURCE,
        DOCUMENTS_SOURCE,
        COMMENTS_SOURCE,
        SEARCH_SOURCE,
        WS_SOURCE,
        WS_HANDLER_SOURCE,
    ]
    .join("\n");

    let mut missing = BTreeSet::new();
    for path in expected_paths {
        if !contract_surface.contains(path) {
            missing.insert(path);
        }
    }

    assert!(missing.is_empty(), "missing route declarations for: {missing:?}",);
}

#[test]
fn rest_contract_declares_expected_http_method_bindings() {
    let expectations = [
        (AUTH_SOURCE, "/v1/auth/oauth/github/start", &["post(start_github_oauth)"][..]),
        (AUTH_SOURCE, "/v1/auth/oauth/github/callback", &["post(callback_github_oauth)"][..]),
        (AUTH_SOURCE, "/v1/auth/token/refresh", &["post(handle_token_refresh)"][..]),
        (AUTH_SOURCE, "/v1/auth/logout", &["post(handle_logout)"][..]),
        (
            API_MOD_SOURCE,
            "/v1/workspaces",
            &["post(workspaces::create_workspace)", ".get(workspaces::list_workspaces)"][..],
        ),
        (API_MOD_SOURCE, "/v1/workspaces/{id}", &["get(workspaces::get_workspace)"][..]),
        (API_MOD_SOURCE, "/v1/workspaces/{id}", &["patch(workspaces::update_workspace)"][..]),
        (
            API_MOD_SOURCE,
            "/v1/workspaces/{workspace_id}/members/{member_id}",
            &["patch(members::update_member)"][..],
        ),
        (
            API_MOD_SOURCE,
            "/v1/workspaces/{workspace_id}/members/{member_id}",
            &["delete(members::delete_member)"][..],
        ),
        (
            API_MOD_SOURCE,
            "/v1/workspaces/{workspace_id}/invites",
            &["post(members::create_invite)"][..],
        ),
        (API_MOD_SOURCE, "/v1/invites/{token}/accept", &["post(members::accept_invite)"][..]),
        (
            API_MOD_SOURCE,
            "/v1/workspaces/{id}/share-links",
            &["post(create_share_link)", ".get(list_share_links)"][..],
        ),
        (
            API_MOD_SOURCE,
            "/v1/workspaces/{id}/share-links/{share_link_id}",
            &["patch(update_share_link)", "delete(revoke_share_link)"][..],
        ),
        (API_MOD_SOURCE, "/v1/share-links/redeem", &["post(redeem_share_link)"][..]),
        (
            DOCUMENTS_SOURCE,
            "/v1/workspaces/{ws_id}/documents",
            &["post(create_document)", ".get(list_documents)"][..],
        ),
        (
            DOCUMENTS_SOURCE,
            "/v1/workspaces/{ws_id}/documents/{doc_id}",
            &["get(get_document)", "patch(update_document)", "delete(delete_document)"][..],
        ),
        (
            DOCUMENTS_SOURCE,
            "/v1/workspaces/{ws_id}/documents/{doc_id}/tags",
            &["post(update_document_tags)"][..],
        ),
        (
            DOCUMENTS_SOURCE,
            "/v1/workspaces/{ws_id}/documents/{doc_id}/acl-overrides",
            &["post(create_acl_override)"][..],
        ),
        (
            DOCUMENTS_SOURCE,
            "/v1/workspaces/{ws_id}/documents/{doc_id}/acl-overrides/{override_id}",
            &["delete(delete_acl_override)"][..],
        ),
        (
            COMMENTS_SOURCE,
            "/v1/workspaces/{ws_id}/documents/{doc_id}/comments",
            &["get(list_comments)", "post(create_comment_thread)"][..],
        ),
        (
            COMMENTS_SOURCE,
            "/v1/workspaces/{ws_id}/comments/{thread_id}/messages",
            &["post(create_comment_message)"][..],
        ),
        (
            COMMENTS_SOURCE,
            "/v1/workspaces/{ws_id}/comments/{thread_id}/resolve",
            &["post(resolve_comment_thread)"][..],
        ),
        (
            COMMENTS_SOURCE,
            "/v1/workspaces/{ws_id}/comments/{thread_id}/reopen",
            &["post(reopen_comment_thread)"][..],
        ),
        (SEARCH_SOURCE, "/v1/workspaces/{id}/search", &["get(search_documents)"][..]),
        (
            WS_HANDLER_SOURCE,
            "/v1/workspaces/{workspace_id}/sync-sessions",
            &["post(create_sync_session)"][..],
        ),
        (WS_HANDLER_SOURCE, "/v1/ws/{session_id}", &["get(ws_upgrade)"][..]),
    ];

    for (source, endpoint, required_tokens) in expectations {
        assert!(source.contains(endpoint), "route `{endpoint}` must exist");
        for token in required_tokens {
            assert!(source.contains(token), "route `{endpoint}` must include token `{token}`",);
        }
    }
}

#[test]
fn rest_contract_sources_include_precondition_enforcement() {
    let sources = [API_MOD_SOURCE, DOCUMENTS_SOURCE];

    for source in sources {
        assert!(
            source.contains("extract_if_match(&headers)?"),
            "PATCH/DELETE handlers must extract If-Match headers",
        );
        assert!(
            source.contains("PreconditionRequired"),
            "missing If-Match must map to PRECONDITION_REQUIRED",
        );
        assert!(
            source.contains("EditPreconditionFailed"),
            "stale If-Match must map to EDIT_PRECONDITION_FAILED",
        );
    }
}

#[test]
fn rest_contract_mounts_documents_router_in_api_builder() {
    assert!(
        API_MOD_SOURCE.contains(".merge(documents::router(pool.clone(), Arc::clone(&jwt_service)))"),
        "build_router_from_env must merge the documents router so document CRUD routes are reachable",
    );
}

#[test]
fn rest_contract_applies_idempotency_middleware_for_api_posts() {
    assert!(
        API_MOD_SOURCE.contains("IdempotencyDbState::new(pool.clone())"),
        "build_router_from_env must initialize idempotency DB state from the shared API pool",
    );
    assert!(
        API_MOD_SOURCE.contains("idempotency::idempotency_db_middleware"),
        "build_router_from_env must layer idempotency middleware to protect POST mutations",
    );
}

#[test]
fn rest_contract_sources_emit_audit_events_for_mutations() {
    assert!(
        DOCUMENTS_SOURCE.contains("try_record_document_audit_event("),
        "documents mutation handlers must record audit events",
    );
    assert!(
        WORKSPACES_SOURCE.contains("try_record_audit_event("),
        "workspace mutation handlers must record audit events",
    );
    assert!(
        MEMBERS_SOURCE.contains("try_record_audit_event("),
        "membership mutation handlers must record audit events",
    );
    assert!(
        API_MOD_SOURCE.contains("AuditEventType::ShareLinkOperation"),
        "share-link mutation handlers must record share-link audit events",
    );
}
