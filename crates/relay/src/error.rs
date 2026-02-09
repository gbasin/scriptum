use std::future::Future;

use axum::{
    http::{header::HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use uuid::Uuid;

pub const REQUEST_ID_HEADER: &str = "x-request-id";
pub const TRACE_ID_HEADER: &str = "x-trace-id";

tokio::task_local! {
    static REQUEST_ID: String;
}

tokio::task_local! {
    static TRACE_ID: String;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    ValidationFailed,
    AuthInvalidToken,
    AuthInvalidRedirect,
    AuthStateMismatch,
    AuthCodeInvalid,
    AuthTokenRevoked,
    AuthForbidden,
    NotFound,
    DocPathConflict,
    EditPreconditionFailed,
    YjsUpdateTooLarge,
    PreconditionRequired,
    RateLimited,
    InternalError,
    AiCommitUnavailable,
    DiskWriteFailed,
    UpgradeRequired,
}

impl ErrorCode {
    /// All variants, for contract test iteration.
    pub const ALL: &[Self] = &[
        Self::ValidationFailed,
        Self::AuthInvalidToken,
        Self::AuthInvalidRedirect,
        Self::AuthStateMismatch,
        Self::AuthCodeInvalid,
        Self::AuthTokenRevoked,
        Self::AuthForbidden,
        Self::NotFound,
        Self::DocPathConflict,
        Self::EditPreconditionFailed,
        Self::YjsUpdateTooLarge,
        Self::PreconditionRequired,
        Self::RateLimited,
        Self::InternalError,
        Self::AiCommitUnavailable,
        Self::DiskWriteFailed,
        Self::UpgradeRequired,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ValidationFailed => "VALIDATION_FAILED",
            Self::AuthInvalidToken => "AUTH_INVALID_TOKEN",
            Self::AuthInvalidRedirect => "AUTH_INVALID_REDIRECT",
            Self::AuthStateMismatch => "AUTH_STATE_MISMATCH",
            Self::AuthCodeInvalid => "AUTH_CODE_INVALID",
            Self::AuthTokenRevoked => "AUTH_TOKEN_REVOKED",
            Self::AuthForbidden => "AUTH_FORBIDDEN",
            Self::NotFound => "NOT_FOUND",
            Self::DocPathConflict => "DOC_PATH_CONFLICT",
            Self::EditPreconditionFailed => "EDIT_PRECONDITION_FAILED",
            Self::YjsUpdateTooLarge => "YJS_UPDATE_TOO_LARGE",
            Self::PreconditionRequired => "PRECONDITION_REQUIRED",
            Self::RateLimited => "RATE_LIMITED",
            Self::InternalError => "INTERNAL_ERROR",
            Self::AiCommitUnavailable => "AI_COMMIT_UNAVAILABLE",
            Self::DiskWriteFailed => "DISK_WRITE_FAILED",
            Self::UpgradeRequired => "UPGRADE_REQUIRED",
        }
    }

    pub const fn status(self) -> StatusCode {
        match self {
            Self::ValidationFailed => StatusCode::BAD_REQUEST,
            Self::AuthInvalidToken => StatusCode::UNAUTHORIZED,
            Self::AuthInvalidRedirect => StatusCode::BAD_REQUEST,
            Self::AuthStateMismatch => StatusCode::UNAUTHORIZED,
            Self::AuthCodeInvalid => StatusCode::UNAUTHORIZED,
            Self::AuthTokenRevoked => StatusCode::UNAUTHORIZED,
            Self::AuthForbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::DocPathConflict => StatusCode::CONFLICT,
            Self::EditPreconditionFailed => StatusCode::PRECONDITION_FAILED,
            Self::YjsUpdateTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::PreconditionRequired => StatusCode::PRECONDITION_REQUIRED,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
            Self::AiCommitUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::DiskWriteFailed => StatusCode::INSUFFICIENT_STORAGE,
            Self::UpgradeRequired => StatusCode::UPGRADE_REQUIRED,
        }
    }

    pub const fn retryable(self) -> bool {
        matches!(
            self,
            Self::RateLimited
                | Self::InternalError
                | Self::AiCommitUnavailable
                | Self::DiskWriteFailed
        )
    }

    pub const fn default_message(self) -> &'static str {
        match self {
            Self::ValidationFailed => "request validation failed",
            Self::AuthInvalidToken => "invalid authentication token",
            Self::AuthInvalidRedirect => "invalid oauth redirect URI",
            Self::AuthStateMismatch => "oauth state parameter mismatch",
            Self::AuthCodeInvalid => "oauth authorization code invalid or expired",
            Self::AuthTokenRevoked => "refresh token has been revoked",
            Self::AuthForbidden => "caller lacks required permission",
            Self::NotFound => "requested resource not found",
            Self::DocPathConflict => "resource already exists",
            Self::EditPreconditionFailed => "request precondition failed",
            Self::YjsUpdateTooLarge => "payload exceeds maximum allowed size",
            Self::PreconditionRequired => "missing required precondition header",
            Self::RateLimited => "request was rate limited",
            Self::InternalError => "internal server error",
            Self::AiCommitUnavailable => "upstream AI service is unavailable",
            Self::DiskWriteFailed => "server could not persist data",
            Self::UpgradeRequired => "client protocol version is not supported",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RelayError {
    code: ErrorCode,
    message: String,
    details: Value,
    request_id: Option<String>,
}

impl RelayError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), details: json!({}), request_id: None }
    }

    pub fn from_code(code: ErrorCode) -> Self {
        Self::new(code, code.default_message())
    }

    pub fn from_status(status: StatusCode, message: impl Into<String>) -> Self {
        Self::new(default_code_for_status(status), message)
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }

    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }
}

impl IntoResponse for RelayError {
    fn into_response(self) -> Response {
        let code = self.code;
        let request_id = self.request_id.or_else(current_request_id);

        let mut response = (
            code.status(),
            Json(json!({
                "error": {
                    "code": code.as_str(),
                    "message": self.message,
                    "retryable": code.retryable(),
                    "request_id": request_id.clone(),
                    "details": self.details,
                }
            })),
        )
            .into_response();
        response.extensions_mut().insert(code);

        if let Some(request_id) = request_id {
            attach_request_id_header(&mut response, &request_id);
        }

        response
    }
}

pub fn default_code_for_status(status: StatusCode) -> ErrorCode {
    match status {
        StatusCode::BAD_REQUEST => ErrorCode::ValidationFailed,
        StatusCode::UNAUTHORIZED => ErrorCode::AuthInvalidToken,
        StatusCode::FORBIDDEN => ErrorCode::AuthForbidden,
        StatusCode::NOT_FOUND => ErrorCode::NotFound,
        StatusCode::CONFLICT => ErrorCode::DocPathConflict,
        StatusCode::PRECONDITION_FAILED => ErrorCode::EditPreconditionFailed,
        StatusCode::PAYLOAD_TOO_LARGE => ErrorCode::YjsUpdateTooLarge,
        StatusCode::PRECONDITION_REQUIRED => ErrorCode::PreconditionRequired,
        StatusCode::TOO_MANY_REQUESTS => ErrorCode::RateLimited,
        StatusCode::SERVICE_UNAVAILABLE => ErrorCode::AiCommitUnavailable,
        StatusCode::INSUFFICIENT_STORAGE => ErrorCode::DiskWriteFailed,
        StatusCode::UPGRADE_REQUIRED => ErrorCode::UpgradeRequired,
        _ => ErrorCode::InternalError,
    }
}

pub async fn with_request_id_scope<F>(request_id: String, future: F) -> F::Output
where
    F: Future,
{
    REQUEST_ID.scope(request_id, future).await
}

pub async fn with_trace_id_scope<F>(trace_id: String, future: F) -> F::Output
where
    F: Future,
{
    TRACE_ID.scope(trace_id, future).await
}

pub fn current_request_id() -> Option<String> {
    REQUEST_ID.try_with(Clone::clone).ok()
}

pub fn current_trace_id() -> Option<String> {
    TRACE_ID.try_with(Clone::clone).ok()
}

pub fn request_id_from_headers_or_generate(headers: &HeaderMap) -> String {
    id_from_headers_or_generate(headers, REQUEST_ID_HEADER)
}

pub fn trace_id_from_headers_or_generate(headers: &HeaderMap) -> String {
    id_from_headers_or_generate(headers, TRACE_ID_HEADER)
}

fn id_from_headers_or_generate(headers: &HeaderMap, header_name: &str) -> String {
    headers
        .get(header_name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().to_string())
}

pub fn attach_request_id_header(response: &mut Response, request_id: &str) {
    if let Ok(header) = HeaderValue::from_str(request_id) {
        response.headers_mut().insert(REQUEST_ID_HEADER, header);
    }
}

pub fn attach_trace_id_header(response: &mut Response, trace_id: &str) {
    if let Ok(header) = HeaderValue::from_str(trace_id) {
        response.headers_mut().insert(TRACE_ID_HEADER, header);
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        body::to_bytes,
        http::{header::HeaderName, HeaderMap, HeaderValue, StatusCode},
        response::IntoResponse,
    };
    use serde_json::Value;

    use super::{
        current_trace_id, default_code_for_status, request_id_from_headers_or_generate,
        trace_id_from_headers_or_generate, with_request_id_scope, with_trace_id_scope, ErrorCode,
        RelayError, REQUEST_ID_HEADER, TRACE_ID_HEADER,
    };

    #[tokio::test]
    async fn relay_error_uses_scoped_request_id() {
        let response = with_request_id_scope("req-scoped-123".to_owned(), async {
            RelayError::from_code(ErrorCode::InternalError).into_response()
        })
        .await;

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("error response body should be readable");
        let parsed: Value =
            serde_json::from_slice(&body).expect("error response body should be valid json");

        assert_eq!(parsed["error"]["code"], "INTERNAL_ERROR");
        assert_eq!(parsed["error"]["retryable"], true);
        assert_eq!(parsed["error"]["request_id"], "req-scoped-123");
        assert_eq!(parsed["error"]["details"], serde_json::json!({}));
    }

    #[test]
    fn status_code_mapping_matches_registry_defaults() {
        assert_eq!(default_code_for_status(StatusCode::BAD_REQUEST), ErrorCode::ValidationFailed);
        assert_eq!(
            default_code_for_status(StatusCode::PRECONDITION_REQUIRED),
            ErrorCode::PreconditionRequired
        );
        assert_eq!(default_code_for_status(StatusCode::TOO_MANY_REQUESTS), ErrorCode::RateLimited);
        assert_eq!(
            default_code_for_status(StatusCode::INTERNAL_SERVER_ERROR),
            ErrorCode::InternalError
        );
    }

    #[tokio::test]
    async fn from_status_maps_http_status_to_registry_code() {
        let response = RelayError::from_status(StatusCode::FORBIDDEN, "denied").into_response();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("error response body should be readable");
        let parsed: Value =
            serde_json::from_slice(&body).expect("error response body should be valid json");
        assert_eq!(parsed["error"]["code"], "AUTH_FORBIDDEN");
        assert_eq!(parsed["error"]["message"], "denied");
    }

    #[tokio::test]
    async fn custom_details_are_preserved() {
        let response = RelayError::new(ErrorCode::ValidationFailed, "bad payload")
            .with_details(serde_json::json!({ "field": "name" }))
            .into_response();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("error response body should be readable");
        let parsed: Value =
            serde_json::from_slice(&body).expect("error response body should be valid json");
        assert_eq!(parsed["error"]["details"]["field"], "name");
    }

    #[tokio::test]
    async fn explicit_request_id_overrides_scope() {
        let response = with_request_id_scope("req-scoped-123".to_owned(), async {
            RelayError::from_code(ErrorCode::AuthForbidden)
                .with_request_id("req-explicit-456")
                .into_response()
        })
        .await;

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("error response body should be readable");
        let parsed: Value =
            serde_json::from_slice(&body).expect("error response body should be valid json");
        assert_eq!(parsed["error"]["request_id"], "req-explicit-456");
    }

    #[test]
    fn response_extensions_include_canonical_error_code() {
        let response = RelayError::from_code(ErrorCode::RateLimited).into_response();
        assert_eq!(response.extensions().get::<ErrorCode>().copied(), Some(ErrorCode::RateLimited));
    }

    #[tokio::test]
    async fn trace_id_scope_is_exposed_to_current_context() {
        let trace_id =
            with_trace_id_scope("trace-scoped-123".to_owned(), async { current_trace_id() }).await;

        assert_eq!(trace_id.as_deref(), Some("trace-scoped-123"));
    }

    #[test]
    fn header_based_ids_accept_request_and_trace_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static(REQUEST_ID_HEADER),
            HeaderValue::from_static("req-header-1"),
        );
        headers.insert(
            HeaderName::from_static(TRACE_ID_HEADER),
            HeaderValue::from_static("trace-header-1"),
        );

        assert_eq!(request_id_from_headers_or_generate(&headers), "req-header-1");
        assert_eq!(trace_id_from_headers_or_generate(&headers), "trace-header-1");
    }

    // ── Contract tests ─────────────────────────────────────────────

    fn load_error_codes_contract() -> serde_json::Value {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../contracts/error-codes.json");
        let content = std::fs::read_to_string(path).expect("contract file should be readable");
        serde_json::from_str(&content).expect("contract file should be valid JSON")
    }

    #[test]
    fn error_codes_match_contract() {
        let contract = load_error_codes_contract();
        let entries = contract["codes"].as_array().expect("codes should be an array");

        let mut contract_codes: Vec<&str> =
            entries.iter().map(|e| e["code"].as_str().expect("code should be a string")).collect();
        contract_codes.sort();

        let mut rust_codes: Vec<&str> = ErrorCode::ALL.iter().map(|c| c.as_str()).collect();
        rust_codes.sort();

        assert_eq!(rust_codes, contract_codes, "ErrorCode::ALL diverged from contract");
    }

    #[test]
    fn error_code_retryability_matches_contract() {
        let contract = load_error_codes_contract();
        let entries = contract["codes"].as_array().expect("codes should be an array");

        for entry in entries {
            let code_str = entry["code"].as_str().expect("code should be a string");
            let expected_retryable =
                entry["retryable"].as_bool().expect("retryable should be a bool");

            let error_code = ErrorCode::ALL
                .iter()
                .find(|c| c.as_str() == code_str)
                .unwrap_or_else(|| panic!("ErrorCode not found for contract code: {code_str}"));

            assert_eq!(
                error_code.retryable(),
                expected_retryable,
                "retryable mismatch for {code_str}"
            );
        }
    }

    #[test]
    fn error_code_http_status_matches_contract() {
        let contract = load_error_codes_contract();
        let entries = contract["codes"].as_array().expect("codes should be an array");

        for entry in entries {
            let code_str = entry["code"].as_str().expect("code should be a string");
            let expected_status =
                entry["http_status"].as_u64().expect("http_status should be a number") as u16;

            let error_code = ErrorCode::ALL
                .iter()
                .find(|c| c.as_str() == code_str)
                .unwrap_or_else(|| panic!("ErrorCode not found for contract code: {code_str}"));

            assert_eq!(
                error_code.status().as_u16(),
                expected_status,
                "HTTP status mismatch for {code_str}"
            );
        }
    }
}
