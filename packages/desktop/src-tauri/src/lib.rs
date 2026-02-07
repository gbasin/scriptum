// Scriptum desktop library entry point.
// Exported as a library so it can be embedded in-process by Tauri
// and linked from the daemon for the embedded desktop mode.

mod auth;
mod commands;
mod daemon;
mod menu;
mod tray;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            auth::init_deep_link_bridge(app);
            if let Err(error) = tray::init(&app.handle()) {
                eprintln!("failed to initialize system tray: {error}");
            }
            tauri::async_runtime::spawn(async {
                if let Err(error) = daemon::run_embedded_daemon().await {
                    eprintln!("embedded daemon exited unexpectedly: {error:#}");
                }
            });
            Ok(())
        })
        .menu(|app| menu::build_menu(app))
        .on_menu_event(menu::handle_menu_event)
        .invoke_handler(tauri::generate_handler![
            commands::greet,
            commands::auth_redirect_uri,
            commands::auth_open_browser,
            commands::auth_parse_callback,
            commands::auth_store_tokens,
            commands::auth_load_tokens,
            commands::auth_clear_tokens,
            commands::tray_set_sync_status,
            commands::tray_get_sync_status
        ])
        .run(tauri::generate_context!())
        .expect("failed to run scriptum desktop app");
}
