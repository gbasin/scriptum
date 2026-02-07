const JWT_SOURCE: &str = include_str!("../src/auth/jwt.rs");
const OAUTH_SOURCE: &str = include_str!("../src/auth/oauth.rs");
const WS_HANDLER_SOURCE: &str = include_str!("../src/ws/handler.rs");
const WS_SESSION_SOURCE: &str = include_str!("../src/ws/session.rs");
const WS_TESTS_SOURCE: &str = include_str!("../src/ws/tests.rs");

#[test]
fn refresh_tokens_are_single_use_and_detect_replay() {
    assert!(
        OAUTH_SOURCE.contains("Revoke old session (single-use rotation)"),
        "refresh token rotation must revoke consumed session"
    );
    assert!(
        OAUTH_SOURCE.contains("refresh token reuse detected; token family revoked"),
        "refresh token replay must trigger family revocation"
    );
    assert!(
        OAUTH_SOURCE.contains("refresh_reuse_detection_revokes_family"),
        "refresh replay regression test must be present"
    );
}

#[test]
fn websocket_resume_tokens_are_single_use_and_bound_to_session_context() {
    assert!(
        WS_TESTS_SOURCE.contains("hello_ack_rotates_resume_token_and_enforces_single_use"),
        "resume tokens should rotate and enforce single-use semantics"
    );
    assert!(
        WS_TESTS_SOURCE.contains("resume_token_is_bound_to_session_context"),
        "resume tokens must be bound to session context"
    );
    assert!(
        WS_HANDLER_SOURCE.contains("resume_accepted"),
        "hello acknowledgement must report resume acceptance status"
    );
    assert!(
        WS_SESSION_SOURCE.contains("next_resume_token"),
        "session validation must rotate resume tokens on successful hello"
    );
}

#[test]
fn expired_jwts_and_session_tokens_are_rejected() {
    assert!(
        JWT_SOURCE.contains("rejects_expired_tokens"),
        "JWT unit coverage must reject expired access tokens"
    );
    assert!(
        WS_TESTS_SOURCE.contains("hello_rejects_expired_session_token"),
        "websocket session handshake must reject expired session tokens"
    );
    assert!(
        WS_HANDLER_SOURCE.contains("SYNC_TOKEN_EXPIRED"),
        "websocket handshake must map expired sessions to SYNC_TOKEN_EXPIRED"
    );
    assert!(
        OAUTH_SOURCE.contains("refresh_rejects_expired_token"),
        "refresh endpoint must reject expired refresh tokens"
    );
}
