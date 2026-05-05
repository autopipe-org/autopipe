// Hide the console window on Windows when built with GUI support.
// MCP server mode still works because Claude Desktop connects via pipes, not console.
#![cfg_attr(all(target_os = "windows", feature = "gui"), windows_subsystem = "windows")]

// These mod declarations make the binary self-contained. The same .rs
// files are also re-exported via `lib.rs` so that the Tauri binary in
// frontend/src-tauri/ can re-use them as a library without duplicating
// any business logic.
#[cfg(feature = "gui")]
mod app;
mod claude_config;
mod config;
mod mcp;
mod ssh;
#[cfg(feature = "gui")]
mod tray;

use std::env;
use std::path::PathBuf;

/// Returns the log file path: same directory as config.json.
fn log_path() -> PathBuf {
    let dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("autopipe-app");
    dir.join("autopipe.log")
}

/// Truncate log file if it exceeds 5 MB, keeping the last 1 MB.
fn trim_log_file(path: &PathBuf) {
    const MAX_SIZE: u64 = 5 * 1024 * 1024;
    const KEEP_SIZE: usize = 1024 * 1024;

    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return,
    };
    if meta.len() <= MAX_SIZE {
        return;
    }
    if let Ok(content) = std::fs::read(path) {
        let start = content.len().saturating_sub(KEEP_SIZE);
        let _ = std::fs::write(path, &content[start..]);
    }
}

/// Initialize tracing to append to the log file with timestamps.
fn init_file_logging() {
    let path = log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    trim_log_file(&path);

    // Append a session separator
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| {
            use std::io::Write;
            writeln!(f, "\n{}", "=".repeat(60))?;
            writeln!(f, "=== AutoPipe session started at {} ===", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"))?;
            writeln!(f, "{}", "=".repeat(60))
        });

    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .expect("Failed to open log file");

    tracing_subscriber::fmt()
        .with_writer(std::sync::Mutex::new(file))
        .with_ansi(false)
        .init();
}

/// On Windows, allocate a console for CLI modes that print output.
#[cfg(target_os = "windows")]
fn ensure_console() {
    unsafe {
        extern "system" {
            fn AttachConsole(dwProcessId: u32) -> i32;
            fn AllocConsole() -> i32;
        }
        if AttachConsole(0xFFFFFFFF) == 0 {
            AllocConsole();
        }
    }
}

/// Build the MCP URL using the last known actual port (preferred) or the
/// configured preferred port. CLI commands rely on this when the GUI daemon
/// is not running in the same process.
fn cli_mcp_url(cfg: &config::AppConfig) -> String {
    let port = cfg.mcp_actual_port.unwrap_or(cfg.mcp_port);
    format!("http://127.0.0.1:{}/mcp", port)
}

fn main() {
    let args: Vec<String> = env::args().collect();

    // CLI modes need a console for output on Windows
    #[cfg(target_os = "windows")]
    if args.iter().any(|a| a == "--register" || a == "--unregister" || a == "--status") {
        ensure_console();
    }

    if args.iter().any(|a| a == "--mcp-server") {
        // stdio MCP server mode — spawned by Claude Desktop (or any client
        // whose JSON config only accepts `command + args` stdio entries).
        // Logs to file because stderr is captured by the MCP transport.
        init_file_logging();
        let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
        if let Err(e) = rt.block_on(mcp::server::run_mcp_stdio_server()) {
            eprintln!("MCP stdio server error: {}", e);
            std::process::exit(1);
        }
    } else if args.iter().any(|a| a == "--register") {
        // Auto-register MCP server in all supported clients using the last
        // known URL + token. If the desktop app has never started, we use the
        // configured (default) port; the next GUI launch will update clients
        // automatically if it has to fall back to a different port.
        let cfg = config::AppConfig::load();
        let url = cli_mcp_url(&cfg);
        let token = match config::load_or_create_mcp_token() {
            Ok(t) => t,
            Err(e) => {
                eprintln!("Failed to load MCP token: {}", e);
                std::process::exit(1);
            }
        };
        let results = claude_config::register_all(&url, &token);
        let mut any_ok = false;
        for (client, result) in &results {
            match result {
                Ok(_) => {
                    println!(
                        "Registered in {}: {}",
                        client.name(),
                        client.config_path().display()
                    );
                    any_ok = true;
                }
                Err(e) => {
                    eprintln!("Failed to register in {}: {}", client.name(), e);
                }
            }
        }
        if any_ok {
            println!();
            println!("URL:   {}", url);
            println!("Make sure the AutoPipe app is running, then restart your AI app.");
        } else {
            std::process::exit(1);
        }
    } else if args.iter().any(|a| a == "--unregister") {
        let results = claude_config::unregister_all();
        for (client, result) in &results {
            match result {
                Ok(_) => println!("Unregistered from {}.", client.name()),
                Err(e) => eprintln!("Failed to unregister from {}: {}", client.name(), e),
            }
        }
    } else if args.iter().any(|a| a == "--status") {
        let cfg = config::AppConfig::load();
        println!("MCP Registration Status:");
        for (client, registered) in claude_config::status_all() {
            let status = if registered { "registered" } else { "not registered" };
            println!(
                "  {}: {} ({})",
                client.name(),
                status,
                client.config_path().display()
            );
        }
        println!();
        println!("MCP URL:        {}", cli_mcp_url(&cfg));
        println!("Configured port: {}", cfg.mcp_port);
        if let Some(actual) = cfg.mcp_actual_port {
            println!("Last bound port: {}", actual);
        }
        println!("Registry URLs:  {:?}", cfg.registry_urls);
    } else {
        // The original `autopipe` binary keeps its legacy egui UI behind the
        // `gui` feature. The new Svelte/Tauri UI lives in a separate Cargo
        // project at frontend/src-tauri/ — run it with `npm run tauri dev`
        // from the frontend/ directory.
        #[cfg(feature = "gui")]
        {
            init_file_logging();
            run_gui();
        }
        #[cfg(not(feature = "gui"))]
        {
            println!("AutoPipe Desktop");
            println!();
            println!("MCP server for bioinformatics pipeline management.");
            println!("Compatible with any MCP client that supports Streamable HTTP.");
            println!();
            println!("Usage:");
            println!("  autopipe                Launch the legacy egui GUI (needs --features gui)");
            println!("  autopipe --mcp-server   Run as stdio MCP server");
            println!("  autopipe --register     Register in supported AI clients");
            println!("  autopipe --unregister   Unregister from supported clients");
            println!("  autopipe --status       Show registration status and current URL");
            println!();
            println!("New Svelte UI: cd frontend && npm run tauri dev");
        }
    }
}

// ── Windows: raw Win32 FFI for tray window restore ──────────────────────────
// On Windows, Visible(false) stops the eframe event loop entirely, so update()
// is never called and tray events are never processed.  We work around this by
// polling tray events on a background thread and calling ShowWindow/
// SetForegroundWindow directly via the Win32 API to make the window visible
// again, which restarts the event loop.
#[cfg(all(feature = "gui", target_os = "windows"))]
mod win_tray {
    extern "system" {
        fn FindWindowW(lpClassName: *const u16, lpWindowName: *const u16) -> isize;
        fn ShowWindow(hWnd: isize, nCmdShow: i32) -> i32;
        fn SetForegroundWindow(hWnd: isize) -> i32;
    }

    const SW_SHOW: i32 = 5;

    /// Find a top-level window by its title.  Returns 0 if not found.
    pub fn find_window_by_title(title: &str) -> isize {
        let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe { FindWindowW(std::ptr::null(), wide.as_ptr()) }
    }

    /// Make a hidden window visible and bring it to the foreground.
    pub fn show_and_focus(hwnd: isize) {
        unsafe {
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
        }
    }
}

#[cfg(feature = "gui")]
fn run_gui() {
    // Linux: tray-icon uses GTK for menus, must init before use
    #[cfg(target_os = "linux")]
    gtk::init().expect("Failed to initialize GTK");

    let app_icon = {
        let png_bytes = include_bytes!("../assets/tray_icon.png");
        let img = image::load_from_memory_with_format(png_bytes, image::ImageFormat::Png)
            .expect("Failed to decode app icon PNG");
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        eframe::egui::IconData {
            rgba: rgba.into_raw(),
            width: w,
            height: h,
        }
    };

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([550.0, 650.0])
            .with_title("AutoPipe")
            .with_icon(std::sync::Arc::new(app_icon)),
        ..Default::default()
    };

    // Windows: shared flags for communication between tray thread and update()
    #[cfg(target_os = "windows")]
    let restore_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    #[cfg(target_os = "windows")]
    let quit_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    eframe::run_native(
        "AutoPipe",
        options,
        Box::new(move |cc| {
            // Background thread: keep requesting repaints so the event loop
            // stays alive even when the window is hidden (Visible(false)).
            // This is needed for Linux/macOS; on Windows, the Win32 API
            // thread handles restore directly.
            let ctx_bg = cc.egui_ctx.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_millis(200));
                ctx_bg.request_repaint();
            });

            // Create tray icon on main thread (required for macOS)
            let tray = match tray::AppTray::new() {
                Ok(t) => Some(t),
                Err(e) => {
                    eprintln!("Warning: Failed to create tray icon: {}", e);
                    None
                }
            };

            // Windows: spawn a background thread that polls tray events and
            // uses Win32 ShowWindow to restore the hidden window.
            #[cfg(target_os = "windows")]
            {
                let ctx_win = cc.egui_ctx.clone();
                let restore = restore_flag.clone();
                let quit = quit_flag.clone();
                let show_id = tray.as_ref().map(|t| t.show_id().clone());
                let quit_id = tray.as_ref().map(|t| t.quit_id().clone());

                std::thread::spawn(move || {
                    use std::sync::atomic::Ordering;

                    // Wait for the window to be created, then find its HWND
                    let hwnd = {
                        let mut h: isize = 0;
                        for _ in 0..50 {
                            std::thread::sleep(std::time::Duration::from_millis(200));
                            h = win_tray::find_window_by_title("AutoPipe");
                            if h != 0 {
                                break;
                            }
                        }
                        if h == 0 {
                            eprintln!("[tray-thread] Could not find AutoPipe window");
                            return;
                        }
                        eprintln!("[tray-thread] Found HWND: {}", h);
                        h
                    };

                    loop {
                        let mut want_restore = false;
                        let mut want_quit = false;

                        // Poll tray icon click events
                        if let Ok(event) = tray_icon::TrayIconEvent::receiver().try_recv() {
                            match event {
                                tray_icon::TrayIconEvent::Click {
                                    button: tray_icon::MouseButton::Left,
                                    ..
                                }
                                | tray_icon::TrayIconEvent::DoubleClick {
                                    button: tray_icon::MouseButton::Left,
                                    ..
                                } => {
                                    want_restore = true;
                                }
                                _ => {}
                            }
                        }

                        // Poll tray menu events (Settings / Quit)
                        if let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
                            if show_id.as_ref().map_or(false, |id| event.id() == id) {
                                want_restore = true;
                            }
                            if quit_id.as_ref().map_or(false, |id| event.id() == id) {
                                want_quit = true;
                            }
                        }

                        if want_restore {
                            eprintln!("[tray-thread] Restore requested");
                            restore.store(true, Ordering::SeqCst);
                            win_tray::show_and_focus(hwnd);
                            ctx_win.request_repaint();
                        }

                        if want_quit {
                            eprintln!("[tray-thread] Quit requested");
                            quit.store(true, Ordering::SeqCst);
                            // Make window visible so eframe can process the Close command
                            win_tray::show_and_focus(hwnd);
                            ctx_win.request_repaint();
                        }

                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                });
            }

            let app = app::AutoPipeApp::new(cc);
            Ok(Box::new(TrayAwareApp {
                inner: app,
                tray,
                #[cfg(target_os = "windows")]
                restore_flag,
                #[cfg(target_os = "windows")]
                quit_flag,
            }))
        }),
    )
    .expect("Failed to start eGUI");
}

#[cfg(feature = "gui")]
struct TrayAwareApp {
    inner: app::AutoPipeApp,
    tray: Option<tray::AppTray>,
    #[cfg(target_os = "windows")]
    restore_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    #[cfg(target_os = "windows")]
    quit_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(feature = "gui")]
impl eframe::App for TrayAwareApp {
    fn update(&mut self, ctx: &eframe::egui::Context, frame: &mut eframe::Frame) {
        // ── Windows: process flags set by the background tray thread ────────
        #[cfg(target_os = "windows")]
        {
            use std::sync::atomic::Ordering;
            if self.restore_flag.load(Ordering::SeqCst) {
                self.restore_flag.store(false, Ordering::SeqCst);
                self.inner.restore_from_tray(ctx);
            }
            if self.quit_flag.load(Ordering::SeqCst) {
                ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
                return;
            }
        }

        // ── Linux: pump GTK events so libappindicator tray icon works ───────
        #[cfg(target_os = "linux")]
        while gtk::events_pending() {
            gtk::main_iteration();
        }

        // ── Non-Windows: poll tray events directly in update() ──────────────
        // On Linux/macOS the event loop stays alive with Visible(false) +
        // request_repaint, so we can poll tray events here.
        #[cfg(not(target_os = "windows"))]
        {
            if let Ok(event) = tray_icon::TrayIconEvent::receiver().try_recv() {
                match event {
                    tray_icon::TrayIconEvent::Click {
                        button: tray_icon::MouseButton::Left,
                        ..
                    }
                    | tray_icon::TrayIconEvent::DoubleClick {
                        button: tray_icon::MouseButton::Left,
                        ..
                    } => {
                        self.inner.restore_from_tray(ctx);
                    }
                    _ => {}
                }
            }

            if let Some(ref tray) = self.tray {
                if let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
                    if event.id() == tray.show_id() {
                        self.inner.restore_from_tray(ctx);
                    } else if event.id() == tray.quit_id() {
                        ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
                        return;
                    }
                }
            }
        }

        self.inner.update(ctx, frame);

        // Non-Windows: keep polling even when minimized to tray
        #[cfg(not(target_os = "windows"))]
        if self.inner.is_minimized_to_tray() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}
