// Scriptum desktop library entry point.
// Exported as a library so it can be embedded in-process by Tauri
// and linked from the daemon for the embedded desktop mode.

mod commands;
mod daemon;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|_app| {
            tauri::async_runtime::spawn(async {
                if let Err(error) = daemon::run_embedded_daemon().await {
                    eprintln!("embedded daemon exited unexpectedly: {error:#}");
                }
            });
            Ok(())
        })
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![commands::greet])
        .run(tauri::generate_context!())
        .expect("failed to run scriptum desktop app");
}
