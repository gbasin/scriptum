// Tauri 2.0 desktop app entry point.
// Wraps the web frontend and provides native capabilities.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    scriptum_desktop_lib::run();
}
