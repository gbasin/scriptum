// Protocol version negotiation and N-1 support.
//
// Clients send a protocol version string (e.g. "scriptum-sync.v1") when
// creating a sync session. The server rejects unsupported versions with
// an UPGRADE_REQUIRED error. N-1 support is maintained for at least one
// release cycle.

use crate::error::{ErrorCode, RelayError};
use serde_json::json;

/// The current (latest) protocol version.
pub const CURRENT_VERSION: &str = "scriptum-sync.v1";

/// All protocol versions the server accepts, newest first.
/// When a new version is released, the previous version moves to index 1
/// (the N-1 slot) and the new version takes index 0.
const SUPPORTED_VERSIONS: &[&str] = &[
    CURRENT_VERSION,
    // N-1 compatibility window.
    "scriptum-sync.v0",
];

/// Returns true if the given protocol version string is supported.
pub fn is_supported(version: &str) -> bool {
    SUPPORTED_VERSIONS.contains(&version)
}

/// Returns the list of supported protocol versions (newest first).
pub fn supported_versions() -> &'static [&'static str] {
    SUPPORTED_VERSIONS
}

/// Validates a client-supplied protocol version. Returns `Ok(())` if
/// supported, or a `RelayError` with code `UPGRADE_REQUIRED` and
/// `details.supported_versions` if not.
pub fn require_supported(version: &str) -> Result<(), RelayError> {
    if is_supported(version) {
        Ok(())
    } else {
        Err(RelayError::new(
            ErrorCode::UpgradeRequired,
            format!("unsupported protocol version: {version}"),
        )
        .with_details(json!({
            "requested_version": version,
            "supported_versions": SUPPORTED_VERSIONS,
            "current_version": CURRENT_VERSION,
        })))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn current_version_is_supported() {
        assert!(is_supported(CURRENT_VERSION));
    }

    #[test]
    fn unknown_version_is_not_supported() {
        assert!(!is_supported("scriptum-sync.v99"));
        assert!(!is_supported(""));
        assert!(!is_supported("some-other-protocol"));
    }

    #[test]
    fn previous_version_is_supported_for_n_minus_one_compatibility() {
        assert!(is_supported("scriptum-sync.v0"));
        assert!(require_supported("scriptum-sync.v0").is_ok());
    }

    #[test]
    fn supported_versions_contains_current() {
        let versions = supported_versions();
        assert!(!versions.is_empty());
        assert_eq!(versions[0], CURRENT_VERSION);
    }

    #[test]
    fn require_supported_accepts_current_version() {
        assert!(require_supported(CURRENT_VERSION).is_ok());
    }

    #[test]
    fn require_supported_rejects_unsupported_version() {
        let err = require_supported("scriptum-sync.v99").unwrap_err();
        let response = axum::response::IntoResponse::into_response(err);
        assert_eq!(response.status(), axum::http::StatusCode::UPGRADE_REQUIRED);
    }

    #[tokio::test]
    async fn upgrade_required_error_includes_supported_versions_in_details() {
        let err = require_supported("scriptum-sync.v99").unwrap_err();
        let response = axum::response::IntoResponse::into_response(err);
        assert_eq!(response.status(), axum::http::StatusCode::UPGRADE_REQUIRED);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        let parsed: serde_json::Value =
            serde_json::from_slice(&body).expect("body should be valid json");

        assert_eq!(parsed["error"]["code"], "UPGRADE_REQUIRED");
        assert_eq!(parsed["error"]["retryable"], false);
        assert_eq!(parsed["error"]["details"]["requested_version"], "scriptum-sync.v99");
        assert_eq!(parsed["error"]["details"]["current_version"], CURRENT_VERSION);

        let supported = parsed["error"]["details"]["supported_versions"]
            .as_array()
            .expect("supported_versions should be an array");
        assert!(supported.iter().any(|v| v == CURRENT_VERSION));
    }

    #[test]
    fn require_supported_rejects_empty_string() {
        assert!(require_supported("").is_err());
    }

    #[test]
    fn require_supported_rejects_partial_match() {
        // Must be exact match, not prefix/suffix
        assert!(require_supported("scriptum-sync.v1-beta").is_err());
        assert!(require_supported("scriptum-sync.v").is_err());
    }

    #[test]
    fn compatibility_matrix_accepts_all_supported_versions_and_keeps_unique_order() {
        let versions = supported_versions();
        assert!(!versions.is_empty(), "supported version list must not be empty");
        assert_eq!(
            versions[0], CURRENT_VERSION,
            "the current version must remain the first (N) entry",
        );

        let mut seen = HashSet::new();
        for version in versions {
            assert!(seen.insert(*version), "duplicate supported version entry: {version}");
            assert!(
                require_supported(version).is_ok(),
                "supported version should be accepted: {version}",
            );
        }
    }
}
