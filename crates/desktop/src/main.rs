#[cfg(feature = "gui")]
mod app;
mod claude_config;
mod config;
mod mcp;
mod ssh;
#[cfg(feature = "gui")]
mod tray;

use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.iter().any(|a| a == "--mcp-server") {
        // MCP server mode: any MCP-compatible app invokes this via stdio
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .init();
        let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
        if let Err(e) = rt.block_on(mcp::server::run_mcp_server()) {
            eprintln!("MCP server error: {}", e);
            std::process::exit(1);
        }
    } else if args.iter().any(|a| a == "--register") {
        // Auto-register MCP server in all supported clients
        let config_path = config::AppConfig::config_path();
        let results = claude_config::register_all(&config_path.to_string_lossy());
        let mut any_ok = false;
        for (client, result) in &results {
            match result {
                Ok(_) => {
                    println!("Registered in {}: {}", client.name(), client.config_path().display());
                    any_ok = true;
                }
                Err(e) => {
                    eprintln!("Failed to register in {}: {}", client.name(), e);
                }
            }
        }
        if any_ok {
            println!();
            println!("Restart your AI app to load autopipe tools.");
        } else {
            std::process::exit(1);
        }
    } else if args.iter().any(|a| a == "--unregister") {
        // Unregister MCP server from all supported clients
        let results = claude_config::unregister_all();
        for (client, result) in &results {
            match result {
                Ok(_) => println!("Unregistered from {}.", client.name()),
                Err(e) => eprintln!("Failed to unregister from {}: {}", client.name(), e),
            }
        }
    } else if args.iter().any(|a| a == "--status") {
        // Check registration status for all clients
        println!("MCP Registration Status:");
        for (client, registered) in claude_config::status_all() {
            let status = if registered { "registered" } else { "not registered" };
            println!("  {}: {} ({})", client.name(), status, client.config_path().display());
        }
        println!();
        let config = config::AppConfig::load();
        println!("Registry URLs: {:?}", config.registry_urls);
    } else {
        #[cfg(feature = "gui")]
        {
            run_gui();
        }
        #[cfg(not(feature = "gui"))]
        {
            println!("AutoPipe Desktop");
            println!();
            println!("MCP server for bioinformatics pipeline management.");
            println!("Compatible with Claude Desktop, Gemini CLI, and any MCP-compatible app.");
            println!();
            println!("Usage:");
            println!("  desktop --mcp-server    Run as MCP server (stdio transport)");
            println!("  desktop --register      Auto-register in Claude Desktop & Gemini CLI");
            println!("  desktop --unregister    Unregister from all supported clients");
            println!("  desktop --status        Check registration status");
            println!();
            println!("GUI mode requires: cargo build --features gui");
            println!("(needs GTK development libraries on Linux)");
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
            .with_inner_size([550.0, 500.0])
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
