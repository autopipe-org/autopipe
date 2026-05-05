// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use autopipe_desktop::commands;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::AppState::default())
        .setup(|app| {
            // Start the MCP daemon as soon as the app is ready so the
            // Status tab can immediately show the URL/token.
            commands::init_state(&app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_mcp_status,
            commands::set_mcp_port,
            commands::rotate_mcp_token,
            commands::register_mcp,
            commands::unregister_mcp,
            commands::registration_status,
            commands::get_ssh_config,
            commands::save_ssh_config,
            commands::get_github_username,
            commands::clear_github_token,
            commands::start_github_login,
        ])
        .on_window_event(|window, event| {
            // Hide the window instead of quitting on close — keep the MCP
            // server running so registered AI apps stay connected. (When the
            // tray icon lands in Phase 3 the user can re-open from there.)
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}
