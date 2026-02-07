// Tauri IPC commands exposed to the frontend.

use crate::{
    auth::{self, AuthTokens, OAuthCallbackPayload, DESKTOP_OAUTH_REDIRECT_URI},
    tray::{self, TraySyncSnapshot, TraySyncStatus},
};

#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("Hello, {name}! Welcome to Scriptum.")
}

#[tauri::command]
pub fn auth_redirect_uri() -> String {
    DESKTOP_OAUTH_REDIRECT_URI.to_string()
}

#[tauri::command]
pub fn auth_open_browser(app: tauri::AppHandle, authorization_url: String) -> Result<(), String> {
    auth::open_authorization_url(&app, &authorization_url).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn auth_parse_callback(url: String) -> Result<OAuthCallbackPayload, String> {
    auth::parse_oauth_callback(&url).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn auth_store_tokens(tokens: AuthTokens) -> Result<(), String> {
    auth::store_tokens(&tokens).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn auth_load_tokens() -> Result<Option<AuthTokens>, String> {
    auth::load_tokens().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn auth_clear_tokens() -> Result<(), String> {
    auth::clear_tokens().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn tray_set_sync_status(
    app: tauri::AppHandle,
    status: TraySyncStatus,
    pending_changes: Option<u32>,
) -> Result<(), String> {
    tray::update_sync_status(&app, status, pending_changes.unwrap_or(0))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn tray_get_sync_status(app: tauri::AppHandle) -> TraySyncSnapshot {
    tray::current_sync_status(&app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greet_returns_welcome_message() {
        let result = greet("Alice");
        assert_eq!(result, "Hello, Alice! Welcome to Scriptum.");
    }

    #[test]
    fn auth_redirect_uri_uses_scriptum_deep_link() {
        assert_eq!(auth_redirect_uri(), "scriptum://auth/callback");
    }
}
