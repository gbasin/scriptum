use axum::Router;

use crate::auth::oauth::OAuthState;

pub fn router(state: OAuthState) -> Router {
    crate::auth::oauth::router(state)
}
