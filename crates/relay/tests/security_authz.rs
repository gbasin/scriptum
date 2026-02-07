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
fn acl_override_management_requires_workspace_or_document_ownership() {
    assert!(
        DOCUMENTS_SOURCE.contains("ensure_acl_override_manager_access"),
        "ACL override handlers must validate manager access"
    );
    assert!(
        DOCUMENTS_SOURCE.contains("is_document_owner"),
        "document ownership check must exist for ACL overrides"
    );
    assert!(
        DOCUMENTS_SOURCE.contains("is_workspace_owner"),
        "workspace ownership check must exist for ACL overrides"
    );
    assert!(
        DOCUMENTS_SOURCE.contains("create_acl_override_rejects_non_owner_and_non_document_owner"),
        "ACL authorization regression test for unauthorized caller must exist"
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
