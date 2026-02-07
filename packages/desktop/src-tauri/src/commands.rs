// Tauri IPC commands exposed to the frontend.

use crate::{
    auth::{self, AuthTokens, OAuthCallbackPayload, DESKTOP_OAUTH_REDIRECT_URI},
    menu::{
        EXPORT_DIALOG_SELECTED_EVENT, IMPORT_DIALOG_SELECTED_EVENT, MENU_ACTION_EVENT,
        MENU_ID_CLOSE_WINDOW, MENU_ID_NEW_DOCUMENT, MENU_ID_QUIT_APP, MENU_ID_SAVE_DOCUMENT,
        MENU_ID_TOGGLE_FULLSCREEN, MENU_SHORTCUT_CLOSE_WINDOW, MENU_SHORTCUT_NEW_DOCUMENT,
        MENU_SHORTCUT_QUIT_APP, MENU_SHORTCUT_SAVE_DOCUMENT, MENU_SHORTCUT_TOGGLE_FULLSCREEN,
    },
    tray::{self, TraySyncSnapshot, TraySyncStatus},
    updater::{self, UpdaterCheckResult, UpdaterInstallResult, UpdaterPolicySnapshot},
};
use serde::Serialize;

const DAEMON_IPC_ENTRYPOINT: &str = "scriptum_daemon::runtime::start_embedded";
const FILE_WATCHER_INTEGRATION_NOTE: &str = "embedded daemon includes watcher pipeline";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopMenuShortcutContract {
    pub action: &'static str,
    pub menu_id: &'static str,
    pub accelerator: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopWebDriverContract {
    pub menu_action_event: &'static str,
    pub import_dialog_selected_event: &'static str,
    pub export_dialog_selected_event: &'static str,
    pub daemon_ipc_entrypoint: &'static str,
    pub file_watcher_integration: &'static str,
    pub menu_shortcuts: Vec<DesktopMenuShortcutContract>,
}

#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("Hello, {name}! Welcome to Scriptum.")
}

#[tauri::command]
pub fn auth_redirect_uri() -> String {
    DESKTOP_OAUTH_REDIRECT_URI.to_string()
}

#[tauri::command]
pub fn desktop_webdriver_contract() -> DesktopWebDriverContract {
    DesktopWebDriverContract {
        menu_action_event: MENU_ACTION_EVENT,
        import_dialog_selected_event: IMPORT_DIALOG_SELECTED_EVENT,
        export_dialog_selected_event: EXPORT_DIALOG_SELECTED_EVENT,
        daemon_ipc_entrypoint: DAEMON_IPC_ENTRYPOINT,
        file_watcher_integration: FILE_WATCHER_INTEGRATION_NOTE,
        menu_shortcuts: vec![
            DesktopMenuShortcutContract {
                action: "new-document",
                menu_id: MENU_ID_NEW_DOCUMENT,
                accelerator: MENU_SHORTCUT_NEW_DOCUMENT,
            },
            DesktopMenuShortcutContract {
                action: "save-document",
                menu_id: MENU_ID_SAVE_DOCUMENT,
                accelerator: MENU_SHORTCUT_SAVE_DOCUMENT,
            },
            DesktopMenuShortcutContract {
                action: "close-window",
                menu_id: MENU_ID_CLOSE_WINDOW,
                accelerator: MENU_SHORTCUT_CLOSE_WINDOW,
            },
            DesktopMenuShortcutContract {
                action: "quit-app",
                menu_id: MENU_ID_QUIT_APP,
                accelerator: MENU_SHORTCUT_QUIT_APP,
            },
            DesktopMenuShortcutContract {
                action: "toggle-fullscreen",
                menu_id: MENU_ID_TOGGLE_FULLSCREEN,
                accelerator: MENU_SHORTCUT_TOGGLE_FULLSCREEN,
            },
        ],
    }
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

#[tauri::command]
pub fn updater_policy(app: tauri::AppHandle) -> UpdaterPolicySnapshot {
    updater::policy_snapshot(&app)
}

#[tauri::command]
pub async fn updater_check(app: tauri::AppHandle) -> UpdaterCheckResult {
    updater::check_for_updates(app, true).await
}

#[tauri::command]
pub async fn updater_install(app: tauri::AppHandle) -> UpdaterInstallResult {
    updater::install_update(app).await
}

#[tauri::command]
pub fn updater_last_check(app: tauri::AppHandle) -> Option<UpdaterCheckResult> {
    updater::last_check(&app)
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

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

    #[test]
    fn desktop_webdriver_contract_exposes_stable_tauri_integration_surface() {
        let contract = desktop_webdriver_contract();

        assert_eq!(contract.menu_action_event, "scriptum://menu-action");
        assert_eq!(contract.import_dialog_selected_event, "scriptum://dialog/import-selected");
        assert_eq!(contract.export_dialog_selected_event, "scriptum://dialog/export-selected");
        assert_eq!(contract.daemon_ipc_entrypoint, DAEMON_IPC_ENTRYPOINT);
        assert_eq!(contract.file_watcher_integration, FILE_WATCHER_INTEGRATION_NOTE);
        assert_eq!(
            contract.menu_shortcuts,
            vec![
                DesktopMenuShortcutContract {
                    action: "new-document",
                    menu_id: MENU_ID_NEW_DOCUMENT,
                    accelerator: MENU_SHORTCUT_NEW_DOCUMENT,
                },
                DesktopMenuShortcutContract {
                    action: "save-document",
                    menu_id: MENU_ID_SAVE_DOCUMENT,
                    accelerator: MENU_SHORTCUT_SAVE_DOCUMENT,
                },
                DesktopMenuShortcutContract {
                    action: "close-window",
                    menu_id: MENU_ID_CLOSE_WINDOW,
                    accelerator: MENU_SHORTCUT_CLOSE_WINDOW,
                },
                DesktopMenuShortcutContract {
                    action: "quit-app",
                    menu_id: MENU_ID_QUIT_APP,
                    accelerator: MENU_SHORTCUT_QUIT_APP,
                },
                DesktopMenuShortcutContract {
                    action: "toggle-fullscreen",
                    menu_id: MENU_ID_TOGGLE_FULLSCREEN,
                    accelerator: MENU_SHORTCUT_TOGGLE_FULLSCREEN,
                },
            ]
        );
    }

    #[test]
    fn desktop_webdriver_contract_serializes_with_webdriver_friendly_shape() {
        let value = serde_json::to_value(desktop_webdriver_contract())
            .expect("desktop webdriver contract should serialize");

        let mut keys = value
            .as_object()
            .expect("contract should serialize as json object")
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        keys.sort();

        assert_eq!(
            keys,
            vec![
                "daemonIpcEntrypoint".to_string(),
                "exportDialogSelectedEvent".to_string(),
                "fileWatcherIntegration".to_string(),
                "importDialogSelectedEvent".to_string(),
                "menuActionEvent".to_string(),
                "menuShortcuts".to_string(),
            ]
        );

        let menu_shortcuts = value
            .get("menuShortcuts")
            .and_then(Value::as_array)
            .expect("menuShortcuts should be an array");
        assert_eq!(menu_shortcuts.len(), 5);
        let first = menu_shortcuts
            .first()
            .and_then(Value::as_object)
            .expect("first shortcut should be an object");
        assert!(first.get("menuId").is_some(), "shortcut keys should use camelCase");
        assert!(
            first.get("menu_id").is_none(),
            "snake_case keys would break webdriver contract consumers"
        );
    }
}
