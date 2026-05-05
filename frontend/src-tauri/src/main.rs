// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use autopipe_desktop::commands;
use autopipe_desktop::{claude_config, config, mcp};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

fn cli_mcp_url(cfg: &config::AppConfig) -> String {
    let port = cfg.mcp_actual_port.unwrap_or(cfg.mcp_port);
    format!("http://127.0.0.1:{}/mcp", port)
}

fn main() {
    // Handle CLI modes BEFORE constructing the Tauri builder. Claude Desktop
    // and friends spawn this binary with `--mcp-server` (and other CLI
    // commands), and they expect a stdio MCP server — not a GUI window.
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--mcp-server") {
        // Stdio MCP server mode (spawned by Claude Desktop / Codex)
        let rt = tokio::runtime::Runtime::new().expect("failed to create runtime");
        if let Err(e) = rt.block_on(mcp::server::run_mcp_stdio_server()) {
            eprintln!("MCP server error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    if args.iter().any(|a| a == "--register") {
        let cfg = config::AppConfig::load();
        let url = cli_mcp_url(&cfg);
        let token = config::load_or_create_mcp_token().unwrap_or_default();
        let results = claude_config::register_all(&url, &token);
        for (client, result) in &results {
            match result {
                Ok(_) => println!("Registered in {}", client.name()),
                Err(e) => eprintln!("Failed to register in {}: {}", client.name(), e),
            }
        }
        return;
    }

    if args.iter().any(|a| a == "--unregister") {
        for (client, result) in claude_config::unregister_all() {
            match result {
                Ok(_) => println!("Unregistered from {}", client.name()),
                Err(e) => eprintln!("Failed to unregister from {}: {}", client.name(), e),
            }
        }
        return;
    }

    if args.iter().any(|a| a == "--status") {
        let cfg = config::AppConfig::load();
        println!("MCP URL:        {}", cli_mcp_url(&cfg));
        println!("Configured port: {}", cfg.mcp_port);
        for (client, registered) in claude_config::status_all() {
            let status = if registered { "registered" } else { "not registered" };
            println!("  {}: {}", client.name(), status);
        }
        return;
    }

    // No CLI arg → launch the GUI/tray app.
    run_gui();
}

fn run_gui() {
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
