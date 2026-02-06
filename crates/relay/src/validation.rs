// Input validation middleware and helpers.
//
// - `ValidatedJson<T>` extractor: content-type check + serde + size enforcement.
// - WebSocket frame size limit constant.
// - Markdown HTML sanitization via allowlist.

use axum::{
    extract::{rejection::JsonRejection, FromRequest, Request},
    response::{IntoResponse, Response},
    Json,
};
use serde::de::DeserializeOwned;

use crate::error::{ErrorCode, RelayError};

/// Maximum WebSocket frame payload in bytes (256 KiB).
pub const MAX_WS_FRAME_BYTES: usize = 256 * 1024;

/// Maximum REST request body in bytes (1 MiB).
/// Matches `MAX_REQUEST_BODY_BYTES` in main.rs — canonical source for validators.
pub const MAX_REST_BODY_BYTES: usize = 1024 * 1024;

// ── ValidatedJson extractor ────────────────────────────────────────

/// A JSON body extractor that returns structured `RelayError` on failure.
///
/// Use this instead of `axum::Json<T>` in handlers to get consistent
/// VALIDATION_FAILED error responses instead of plain-text Axum rejections.
pub struct ValidatedJson<T>(pub T);

impl<S, T> FromRequest<S> for ValidatedJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
    Json<T>: FromRequest<S, Rejection = JsonRejection>,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(ValidatedJson(value)),
            Err(rejection) => {
                let (message, details) = classify_json_rejection(&rejection);
                Err(RelayError::new(ErrorCode::ValidationFailed, message)
                    .with_details(details)
                    .into_response())
            }
        }
    }
}

/// Classify a JSON rejection into a human-readable message and details object.
fn classify_json_rejection(rejection: &JsonRejection) -> (String, serde_json::Value) {
    match rejection {
        JsonRejection::JsonDataError(e) => (
            format!("invalid JSON payload: {e}"),
            serde_json::json!({ "kind": "data_error" }),
        ),
        JsonRejection::JsonSyntaxError(e) => (
            format!("malformed JSON: {e}"),
            serde_json::json!({ "kind": "syntax_error" }),
        ),
        JsonRejection::MissingJsonContentType(_) => (
            "expected Content-Type: application/json".to_string(),
            serde_json::json!({ "kind": "missing_content_type" }),
        ),
        JsonRejection::BytesRejection(e) => (
            format!("request body error: {e}"),
            serde_json::json!({ "kind": "body_error" }),
        ),
        other => (
            format!("request body error: {other}"),
            serde_json::json!({ "kind": "unknown" }),
        ),
    }
}

// ── WebSocket frame validation ─────────────────────────────────────

/// Check if a WebSocket binary frame exceeds the size limit.
/// Returns an error message suitable for sending back as a WS close reason.
pub fn check_ws_frame_size(payload: &[u8]) -> Result<(), String> {
    if payload.len() > MAX_WS_FRAME_BYTES {
        Err(format!(
            "frame size {} bytes exceeds limit of {} bytes",
            payload.len(),
            MAX_WS_FRAME_BYTES
        ))
    } else {
        Ok(())
    }
}

// ── Markdown sanitization ──────────────────────────────────────────

/// Allowed HTML tags in rendered markdown content.
const ALLOWED_TAGS: &[&str] = &[
    "h1", "h2", "h3", "h4", "h5", "h6", "p", "br", "hr", "blockquote",
    "pre", "code", "em", "strong", "del", "ul", "ol", "li", "a", "img",
    "table", "thead", "tbody", "tr", "th", "td", "div", "span", "sup",
    "sub", "details", "summary", "kbd",
];

/// Allowed attributes on HTML elements.
const ALLOWED_ATTRS: &[(&str, &[&str])] = &[
    ("a", &["href", "title", "rel"]),
    ("img", &["src", "alt", "title", "width", "height"]),
    ("td", &["align"]),
    ("th", &["align"]),
    ("code", &["class"]),
    ("div", &["class"]),
    ("span", &["class"]),
];

/// URL scheme allowlist for href/src attributes.
const ALLOWED_URL_SCHEMES: &[&str] = &["http", "https", "mailto"];

/// Strip disallowed HTML from markdown-rendered content.
///
/// This is a lightweight allowlist-based sanitizer. It removes:
/// - Tags not in `ALLOWED_TAGS`
/// - Attributes not in `ALLOWED_ATTRS` for the given tag
/// - URLs with disallowed schemes (e.g. `javascript:`)
/// - Event handler attributes (`on*`)
///
/// Does NOT parse full HTML — operates on a tag-by-tag basis via regex-free
/// scanning. For V1 this is sufficient; a full HTML parser can be added later.
pub fn sanitize_html(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(&ch) = chars.peek() {
        if ch == '<' {
            let tag = collect_tag(&mut chars);
            if let Some(sanitized) = sanitize_tag(&tag) {
                output.push_str(&sanitized);
            }
            // Disallowed tags are silently dropped.
        } else {
            output.push(ch);
            chars.next();
        }
    }

    output
}

/// Collect characters from `<` through the matching `>` (inclusive),
/// respecting quoted attribute values that may contain `>`.
fn collect_tag(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut tag = String::new();
    let mut in_quote: Option<char> = None;
    for ch in chars.by_ref() {
        tag.push(ch);
        match in_quote {
            Some(q) if ch == q => in_quote = None,
            Some(_) => {}
            None if ch == '"' || ch == '\'' => in_quote = Some(ch),
            None if ch == '>' => break,
            None => {}
        }
    }
    tag
}

/// Parse and sanitize a single HTML tag string. Returns `None` if disallowed.
fn sanitize_tag(tag: &str) -> Option<String> {
    let inner = tag.trim_start_matches('<').trim_end_matches('>').trim();

    // Handle closing tags: </tagname>
    if let Some(rest) = inner.strip_prefix('/') {
        let tag_name = rest.trim().split_whitespace().next().unwrap_or("").to_ascii_lowercase();
        if is_allowed_tag(&tag_name) {
            return Some(format!("</{tag_name}>"));
        }
        return None;
    }

    // Handle self-closing or opening tags.
    let mut parts = inner.splitn(2, |c: char| c.is_whitespace());
    let tag_name = parts.next().unwrap_or("").trim_end_matches('/').to_ascii_lowercase();

    if !is_allowed_tag(&tag_name) {
        return None;
    }

    let attrs_str = parts.next().unwrap_or("");
    let self_closing = inner.ends_with('/');

    let allowed = allowed_attrs_for(&tag_name);
    let sanitized_attrs = sanitize_attrs(attrs_str, allowed);

    let mut result = format!("<{tag_name}");
    if !sanitized_attrs.is_empty() {
        result.push(' ');
        result.push_str(&sanitized_attrs);
    }
    if self_closing {
        result.push_str(" />");
    } else {
        result.push('>');
    }
    Some(result)
}

fn is_allowed_tag(name: &str) -> bool {
    ALLOWED_TAGS.contains(&name)
}

fn allowed_attrs_for(tag: &str) -> &'static [&'static str] {
    for &(t, attrs) in ALLOWED_ATTRS {
        if t == tag {
            return attrs;
        }
    }
    &[]
}

/// Parse and filter attributes, dropping disallowed ones and unsafe URLs.
fn sanitize_attrs(attrs_str: &str, allowed: &[&str]) -> String {
    let mut result = Vec::new();
    // Simple attribute parser: name="value" or name='value' or name=value.
    let mut remaining = attrs_str.trim();

    while !remaining.is_empty() {
        // Skip self-closing slash at end.
        remaining = remaining.trim_start();
        if remaining.starts_with('/') {
            break;
        }

        // Extract attribute name.
        let name_end = remaining
            .find(|c: char| c == '=' || c.is_whitespace())
            .unwrap_or(remaining.len());
        let attr_name = remaining[..name_end].to_ascii_lowercase();
        remaining = remaining[name_end..].trim_start();

        // Reject event handlers (onclick, onload, etc.).
        if attr_name.starts_with("on") {
            // Skip past the value.
            remaining = skip_attr_value(remaining);
            continue;
        }

        // Extract value if present.
        if remaining.starts_with('=') {
            remaining = remaining[1..].trim_start();
            let (value, rest) = extract_attr_value(remaining);
            remaining = rest;

            if allowed.contains(&attr_name.as_str()) {
                // Validate URL schemes for href/src.
                if (attr_name == "href" || attr_name == "src") && !is_safe_url(&value) {
                    continue;
                }
                result.push(format!("{attr_name}=\"{}\"", escape_attr_value(&value)));
            }
        } else if allowed.contains(&attr_name.as_str()) {
            // Boolean attribute.
            result.push(attr_name);
        }
    }

    result.join(" ")
}

fn skip_attr_value(s: &str) -> &str {
    if s.starts_with('=') {
        let s = s[1..].trim_start();
        let (_, rest) = extract_attr_value(s);
        rest
    } else {
        s
    }
}

fn extract_attr_value(s: &str) -> (String, &str) {
    if s.starts_with('"') {
        let end = s[1..].find('"').map(|i| i + 1).unwrap_or(s.len());
        (s[1..end].to_string(), &s[end + 1..])
    } else if s.starts_with('\'') {
        let end = s[1..].find('\'').map(|i| i + 1).unwrap_or(s.len());
        (s[1..end].to_string(), &s[end + 1..])
    } else {
        let end = s.find(|c: char| c.is_whitespace() || c == '>' || c == '/').unwrap_or(s.len());
        (s[..end].to_string(), &s[end..])
    }
}

fn escape_attr_value(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn is_safe_url(url: &str) -> bool {
    let trimmed = url.trim();
    // Relative URLs and fragment-only URLs are safe.
    if trimmed.starts_with('/') || trimmed.starts_with('#') || trimmed.starts_with('?') {
        return true;
    }
    // Check scheme.
    if let Some(colon_pos) = trimmed.find(':') {
        let scheme = &trimmed[..colon_pos].to_ascii_lowercase();
        ALLOWED_URL_SCHEMES.contains(&scheme.as_str())
    } else {
        // No scheme — treat as relative.
        true
    }
}

// ── Convenience: validate content length ───────────────────────────

/// Check if a body size exceeds the REST limit. Returns a `RelayError` on violation.
pub fn check_content_length(len: usize) -> Result<(), RelayError> {
    if len > MAX_REST_BODY_BYTES {
        Err(RelayError::new(
            ErrorCode::ValidationFailed,
            format!("request body size {len} bytes exceeds limit of {MAX_REST_BODY_BYTES} bytes"),
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::{Method, Request, StatusCode},
        routing::post,
        Router,
    };
    use serde::Deserialize;
    use tower::ServiceExt;

    // ── ValidatedJson tests ───────────────────────────────────────

    #[derive(Debug, Deserialize)]
    struct TestPayload {
        name: String,
    }

    async fn echo_handler(ValidatedJson(payload): ValidatedJson<TestPayload>) -> impl IntoResponse {
        (StatusCode::OK, payload.name)
    }

    fn test_app() -> Router {
        Router::new().route("/test", post(echo_handler))
    }

    #[tokio::test]
    async fn validated_json_accepts_valid_payload() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/test")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"alice"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body.as_ref(), b"alice");
    }

    #[tokio::test]
    async fn validated_json_rejects_missing_content_type() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/test")
                    .body(Body::from(r#"{"name":"alice"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["error"]["code"], "VALIDATION_FAILED");
        assert_eq!(parsed["error"]["details"]["kind"], "missing_content_type");
    }

    #[tokio::test]
    async fn validated_json_rejects_malformed_json() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/test")
                    .header("content-type", "application/json")
                    .body(Body::from("not json"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["error"]["code"], "VALIDATION_FAILED");
        assert_eq!(parsed["error"]["details"]["kind"], "syntax_error");
    }

    #[tokio::test]
    async fn validated_json_rejects_missing_field() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/test")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"age": 42}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["error"]["code"], "VALIDATION_FAILED");
        assert_eq!(parsed["error"]["details"]["kind"], "data_error");
    }

    // ── WebSocket frame size tests ────────────────────────────────

    #[test]
    fn ws_frame_within_limit() {
        let payload = vec![0u8; MAX_WS_FRAME_BYTES];
        assert!(check_ws_frame_size(&payload).is_ok());
    }

    #[test]
    fn ws_frame_exceeds_limit() {
        let payload = vec![0u8; MAX_WS_FRAME_BYTES + 1];
        let err = check_ws_frame_size(&payload).unwrap_err();
        assert!(err.contains("exceeds limit"));
    }

    #[test]
    fn ws_frame_empty() {
        assert!(check_ws_frame_size(&[]).is_ok());
    }

    // ── Content length check ──────────────────────────────────────

    #[test]
    fn content_length_within_limit() {
        assert!(check_content_length(MAX_REST_BODY_BYTES).is_ok());
    }

    #[test]
    fn content_length_exceeds_limit() {
        assert!(check_content_length(MAX_REST_BODY_BYTES + 1).is_err());
    }

    // ── Sanitization tests ────────────────────────────────────────

    #[test]
    fn sanitize_allows_safe_tags() {
        let input = "<p>Hello <strong>world</strong></p>";
        assert_eq!(sanitize_html(input), input);
    }

    #[test]
    fn sanitize_strips_script_tags() {
        let input = "<p>Hello</p><script>alert('xss')</script><p>World</p>";
        assert_eq!(sanitize_html(input), "<p>Hello</p>alert('xss')<p>World</p>");
    }

    #[test]
    fn sanitize_strips_event_handlers() {
        let input = r#"<a href="https://example.com" onclick="alert('xss')">link</a>"#;
        let result = sanitize_html(input);
        assert!(result.contains("href="));
        assert!(!result.contains("onclick"));
    }

    #[test]
    fn sanitize_strips_javascript_urls() {
        let input = r#"<a href="javascript:alert('xss')">click</a>"#;
        let result = sanitize_html(input);
        assert!(!result.contains("javascript"));
        assert!(result.contains("<a>"));
    }

    #[test]
    fn sanitize_allows_https_urls() {
        let input = r#"<a href="https://example.com">link</a>"#;
        let result = sanitize_html(input);
        assert!(result.contains("https://example.com"));
    }

    #[test]
    fn sanitize_allows_mailto_urls() {
        let input = r#"<a href="mailto:user@example.com">email</a>"#;
        let result = sanitize_html(input);
        assert!(result.contains("mailto:user@example.com"));
    }

    #[test]
    fn sanitize_allows_relative_urls() {
        let input = r#"<a href="/docs/readme.md">readme</a>"#;
        let result = sanitize_html(input);
        assert!(result.contains("/docs/readme.md"));
    }

    #[test]
    fn sanitize_strips_iframe() {
        let input = r#"<iframe src="https://evil.com"></iframe>"#;
        assert_eq!(sanitize_html(input), "");
    }

    #[test]
    fn sanitize_strips_style_tag() {
        let input = "<style>body { display: none; }</style><p>content</p>";
        assert_eq!(sanitize_html(input), "body { display: none; }<p>content</p>");
    }

    #[test]
    fn sanitize_allows_img_with_alt() {
        let input = r#"<img src="https://example.com/img.png" alt="photo" />"#;
        let result = sanitize_html(input);
        assert!(result.contains("src="));
        assert!(result.contains("alt="));
    }

    #[test]
    fn sanitize_strips_data_url_in_img() {
        let input = r#"<img src="data:text/html,<script>alert('xss')</script>" />"#;
        let result = sanitize_html(input);
        assert!(!result.contains("data:"));
    }

    #[test]
    fn sanitize_preserves_plain_text() {
        let input = "Hello world, no HTML here.";
        assert_eq!(sanitize_html(input), input);
    }

    #[test]
    fn sanitize_handles_code_blocks() {
        let input = "<pre><code class=\"language-rust\">fn main() {}</code></pre>";
        let result = sanitize_html(input);
        assert!(result.contains("<pre>"));
        assert!(result.contains("<code"));
        assert!(result.contains("class=\"language-rust\""));
    }

    #[test]
    fn sanitize_empty_input() {
        assert_eq!(sanitize_html(""), "");
    }

    #[test]
    fn sanitize_escapes_attribute_values() {
        let input = r#"<a href="https://example.com?a=1&amp;b=2" title="a &quot;link&quot;">text</a>"#;
        let result = sanitize_html(input);
        // Should contain the link but re-escaped.
        assert!(result.contains("<a"));
        assert!(result.contains("href="));
    }
}
