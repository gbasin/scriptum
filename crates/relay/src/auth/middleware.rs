use crate::{
    auth::jwt::{JwtAccessTokenService, WorkspaceAccess},
    error::{ErrorCode, RelayError},
};
use axum::{
    extract::{Request, State},
    http::header::AUTHORIZATION,
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedUser {
    pub user_id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
}

pub async fn require_bearer_auth(
    State(jwt_service): State<Arc<JwtAccessTokenService>>,
    mut request: Request,
    next: Next,
) -> Response {
    let token = match request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(extract_bearer_token)
    {
        Some(token) => token,
        None => return unauthorized_response("missing bearer token"),
    };

    let WorkspaceAccess { user_id, workspace_id } =
        match jwt_service.validate_workspace_token(token) {
            Ok(claims) => claims,
            Err(_) => return unauthorized_response("invalid bearer token"),
        };

    request.extensions_mut().insert(AuthenticatedUser { user_id, workspace_id });

    next.run(request).await
}

fn extract_bearer_token(value: &str) -> Option<&str> {
    let (scheme, token) = value.split_once(' ')?;

    if !scheme.eq_ignore_ascii_case("Bearer") {
        return None;
    }

    let token = token.trim();
    if token.is_empty() {
        return None;
    }

    Some(token)
}

fn unauthorized_response(message: &'static str) -> Response {
    RelayError::new(ErrorCode::AuthInvalidToken, message).into_response()
}

#[cfg(test)]
mod tests {
    use super::{require_bearer_auth, AuthenticatedUser};
    use crate::auth::jwt::JwtAccessTokenService;
    use axum::{
        body::Body,
        extract::Extension,
        http::{header::AUTHORIZATION, Request, StatusCode},
        middleware,
        routing::get,
        Router,
    };
    use std::sync::Arc;
    use tower::ServiceExt;
    use uuid::Uuid;

    const TEST_SECRET: &str = "scriptum_test_secret_that_is_definitely_long_enough";

    fn protected_app(jwt_service: Arc<JwtAccessTokenService>) -> Router {
        Router::new()
            .route(
                "/protected",
                get(|Extension(user): Extension<AuthenticatedUser>| async move {
                    format!("{}:{}", user.user_id, user.workspace_id)
                }),
            )
            .layer(middleware::from_fn_with_state(jwt_service, require_bearer_auth))
    }

    #[tokio::test]
    async fn rejects_requests_without_bearer_token() {
        let app = protected_app(Arc::new(
            JwtAccessTokenService::new(TEST_SECRET).expect("service should initialize"),
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should return a response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn rejects_requests_with_invalid_bearer_token() {
        let app = protected_app(Arc::new(
            JwtAccessTokenService::new(TEST_SECRET).expect("service should initialize"),
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header(AUTHORIZATION, "Bearer invalid-token")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should return a response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn injects_authenticated_user_for_valid_bearer_token() {
        let service =
            Arc::new(JwtAccessTokenService::new(TEST_SECRET).expect("service should initialize"));
        let user_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let token =
            service.issue_workspace_token(user_id, workspace_id).expect("token should be issued");

        let response = protected_app(service)
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should return a response");

        assert_eq!(response.status(), StatusCode::OK);
    }
}
