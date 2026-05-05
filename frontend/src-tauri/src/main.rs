// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use autopipe_desktop::commands;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::AppState::default())
        .setup(|app| {
            // Start the MCP daemon as soon as the app is ready so the
            // Status section can immediately show the URL/token.
            commands::init_state(&app.handle());

            // Build the system tray with a Show / Quit menu.
            let show_item = MenuItem::with_id(app, "show", "Show AutoPipe", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

            // Use the default window icon for the tray. tauri.conf.json must
            // declare at least one icon under bundle.icon for this to be
            // populated; we panic with a clear message otherwise so the
            // misconfiguration is obvious during development.
            let icon = app
                .default_window_icon()
                .cloned()
                .expect("no default window icon — add one to tauri.conf.json bundle.icon");

            let _tray = TrayIconBuilder::with_id("autopipe-tray")
                .icon(icon)
                .tooltip("AutoPipe")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                // Left-click the tray icon to bring the window back.
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                })
                .build(app)?;

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
            commands::move_to_tray,
            commands::get_registry_url,
            commands::set_registry_url,
        ])
        // Window close (X button) now quits the app. Use the explicit
        // "Move to tray" button in the UI (or the tray menu) when you
        // want to keep AutoPipe running in the background.
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}
