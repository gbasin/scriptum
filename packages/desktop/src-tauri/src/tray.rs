use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{
    menu::{MenuBuilder, MenuEvent, MenuItem, MenuItemBuilder, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Runtime,
};

const TRAY_ICON_ID: &str = "tray.scriptum";
const TRAY_OPEN_APP_ID: &str = "tray.open-app";
const TRAY_SYNC_STATUS_ID: &str = "tray.sync-status";
const TRAY_QUIT_ID: &str = "tray.quit-app";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TraySyncStatus {
    Online,
    Offline,
    Reconnecting,
}

impl TraySyncStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Online => "Online",
            Self::Offline => "Offline",
            Self::Reconnecting => "Reconnecting",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraySyncSnapshot {
    pub status: TraySyncStatus,
    pub pending_changes: u32,
}

impl Default for TraySyncSnapshot {
    fn default() -> Self {
        Self { status: TraySyncStatus::Offline, pending_changes: 0 }
    }
}

struct TrayController<R: Runtime> {
    tray_icon: TrayIcon<R>,
    sync_status_item: MenuItem<R>,
    snapshot: Mutex<TraySyncSnapshot>,
}

pub fn init<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    if app.try_state::<TrayController<R>>().is_some() {
        return Ok(());
    }

    let default_snapshot = TraySyncSnapshot::default();
    let open_app = MenuItemBuilder::with_id(TRAY_OPEN_APP_ID, "Open App").build(app)?;
    let sync_status =
        MenuItemBuilder::with_id(TRAY_SYNC_STATUS_ID, format_sync_menu_label(default_snapshot))
            .enabled(false)
            .build(app)?;
    let quit = MenuItemBuilder::with_id(TRAY_QUIT_ID, "Quit").build(app)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let tray_menu =
        MenuBuilder::new(app).items(&[&open_app, &sync_status, &separator, &quit]).build()?;

    let mut tray_builder = TrayIconBuilder::with_id(TRAY_ICON_ID)
        .menu(&tray_menu)
        .show_menu_on_left_click(false)
        .tooltip(format_sync_tooltip(default_snapshot))
        .on_menu_event(handle_tray_menu_event)
        .on_tray_icon_event(handle_tray_icon_event);

    if let Some(icon) = app.default_window_icon().cloned() {
        tray_builder = tray_builder.icon(icon);
    }

    let tray_icon = tray_builder.build(app)?;
    app.manage(TrayController {
        tray_icon,
        sync_status_item: sync_status,
        snapshot: Mutex::new(default_snapshot),
    });

    update_sync_status(app, default_snapshot.status, default_snapshot.pending_changes)
}

pub fn update_sync_status<R: Runtime>(
    app: &AppHandle<R>,
    status: TraySyncStatus,
    pending_changes: u32,
) -> tauri::Result<()> {
    let Some(controller) = app.try_state::<TrayController<R>>() else {
        return Ok(());
    };

    let snapshot = TraySyncSnapshot { status, pending_changes };
    controller.sync_status_item.set_text(format_sync_menu_label(snapshot))?;
    controller.tray_icon.set_tooltip(Some(format_sync_tooltip(snapshot)))?;

    if let Some(badge) = format_pending_badge(snapshot.pending_changes) {
        controller.tray_icon.set_title(Some(badge))?;
    } else {
        controller.tray_icon.set_title(Option::<String>::None)?;
    }

    if let Ok(mut guard) = controller.snapshot.lock() {
        *guard = snapshot;
    }

    Ok(())
}

pub fn current_sync_status<R: Runtime>(app: &AppHandle<R>) -> TraySyncSnapshot {
    app.try_state::<TrayController<R>>()
        .and_then(|controller| controller.snapshot.lock().ok().map(|snapshot| *snapshot))
        .unwrap_or_default()
}

fn handle_tray_menu_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    match event.id().as_ref() {
        TRAY_OPEN_APP_ID => open_main_window(app),
        TRAY_QUIT_ID => app.exit(0),
        _ => {}
    }
}

fn handle_tray_icon_event<R: Runtime>(tray: &TrayIcon<R>, event: TrayIconEvent) {
    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
        open_main_window(tray.app_handle());
    }
}

fn open_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn format_sync_menu_label(snapshot: TraySyncSnapshot) -> String {
    if snapshot.pending_changes == 0 {
        return format!("Sync: {}", snapshot.status.label());
    }

    format!(
        "Sync: {} ({})",
        snapshot.status.label(),
        format_pending_changes(snapshot.pending_changes)
    )
}

fn format_sync_tooltip(snapshot: TraySyncSnapshot) -> String {
    if snapshot.pending_changes == 0 {
        return format!("Scriptum\nSync: {}", snapshot.status.label());
    }

    format!(
        "Scriptum\nSync: {}\n{}",
        snapshot.status.label(),
        format_pending_changes(snapshot.pending_changes)
    )
}

fn format_pending_badge(pending_changes: u32) -> Option<String> {
    if pending_changes == 0 {
        None
    } else {
        Some(pending_changes.to_string())
    }
}

fn format_pending_changes(pending_changes: u32) -> String {
    if pending_changes == 1 {
        "1 pending change".to_string()
    } else {
        format!("{pending_changes} pending changes")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        format_pending_badge, format_sync_menu_label, format_sync_tooltip, TraySyncSnapshot,
        TraySyncStatus,
    };

    #[test]
    fn menu_label_without_pending_changes_is_compact() {
        let label = format_sync_menu_label(TraySyncSnapshot {
            status: TraySyncStatus::Online,
            pending_changes: 0,
        });
        assert_eq!(label, "Sync: Online");
    }

    #[test]
    fn menu_label_with_pending_changes_includes_badge_text() {
        let label = format_sync_menu_label(TraySyncSnapshot {
            status: TraySyncStatus::Reconnecting,
            pending_changes: 4,
        });
        assert_eq!(label, "Sync: Reconnecting (4 pending changes)");
    }

    #[test]
    fn tooltip_contains_status_and_pending_change_count() {
        let tooltip = format_sync_tooltip(TraySyncSnapshot {
            status: TraySyncStatus::Offline,
            pending_changes: 1,
        });
        assert_eq!(tooltip, "Scriptum\nSync: Offline\n1 pending change");
    }

    #[test]
    fn pending_badge_hides_for_zero() {
        assert_eq!(format_pending_badge(0), None);
        assert_eq!(format_pending_badge(12), Some("12".to_string()));
    }
}
