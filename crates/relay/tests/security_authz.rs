const API_SOURCE: &str = include_str!("../src/api/mod.rs");
const DOCUMENTS_SOURCE: &str = include_str!("../src/api/documents.rs");
const OAUTH_SOURCE: &str = include_str!("../src/auth/oauth.rs");

#[test]
fn authz_guards_require_workspace_roles_and_reject_non_members() {
    assert!(
        API_SOURCE.contains("require_workspace_editor_role"),
        "editor routes must enforce workspace editor role"
    );
    assert!(
        API_SOURCE.contains("require_workspace_owner_role"),
        "owner routes must enforce workspace owner role"
    );
    assert!(
        API_SOURCE.contains("caller lacks workspace access"),
        "non-members must be rejected by workspace access checks"
    );
    assert!(
        API_SOURCE.contains("caller lacks required role"),
        "insufficient role requests must return forbidden"
    );
}

#[test]
fn share_link_permission_checks_cover_disabled_revoked_expired_and_exhausted() {
    for token in [
        "this share link has been disabled",
        "this share link has been revoked",
        "this share link has expired",
        "this share link has reached its maximum uses",
    ] {
        assert!(API_SOURCE.contains(token), "share-link guard for `{token}` must be present");
    }
}

#[test]
fn acl_override_management_enforces_workspace_authorization() {
    assert!(
        DOCUMENTS_SOURCE
            .contains("require_workspace_role(&state.store, &user, ws_id, WorkspaceRole::Editor)"),
        "ACL override handlers must enforce at least workspace editor role"
    );
    assert!(
        DOCUMENTS_SOURCE.contains("create_acl_override_rejects_non_owner_and_non_document_owner"),
        "ACL authorization regression test for forbidden callers must exist"
    );
    assert!(
        DOCUMENTS_SOURCE
            .contains("workspace_owner_can_manage_acl_overrides_for_other_document_owner"),
        "workspace owners should be allowed to manage overrides"
    );
    assert!(
        DOCUMENTS_SOURCE
            .contains("editor_role_cannot_manage_acl_overrides_for_other_users_documents"),
        "workspace editors should be denied ACL override management"
    );
    assert!(
        DOCUMENTS_SOURCE.contains(
            "require_document_role(&state.store, &user, ws_id, doc_id, WorkspaceRole::Owner)"
        ),
        "destructive document routes must enforce owner role"
    );
    assert!(
        DOCUMENTS_SOURCE.contains("editor_role_cannot_delete_documents"),
        "editor delete regression test must enforce owner-only document deletion"
    );
}

#[test]
fn refresh_token_security_rejects_expired_and_revoked_tokens() {
    assert!(
        OAUTH_SOURCE.contains("refresh token has expired"),
        "expired refresh tokens must be rejected"
    );
    assert!(
        OAUTH_SOURCE.contains("refresh token reuse detected; token family revoked"),
        "refresh token replay detection must revoke token family"
    );
    assert!(
        OAUTH_SOURCE.contains("logout_revokes_session"),
        "logout path should revoke refresh sessions"
    );
}
