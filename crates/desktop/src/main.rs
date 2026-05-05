// CLI entry point for the legacy `autopipe` binary. The GUI now lives in
// frontend/src-tauri/ as the `autopipe-tauri` binary, which depends on the
// same modules via the `autopipe_desktop` library crate. This binary keeps
// the historical CLI flags (`--mcp-server`, `--register`, `--unregister`,
// `--status`) working so existing scripts and MCP client registrations
// don't break.

mod claude_config;
mod config;
mod mcp;
mod ssh;

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

    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| {
            use std::io::Write;
            writeln!(f, "\n{}", "=".repeat(60))?;
            writeln!(
                f,
                "=== AutoPipe session started at {} ===",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
            )?;
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
/// configured preferred port.
fn cli_mcp_url(cfg: &config::AppConfig) -> String {
    let port = cfg.mcp_actual_port.unwrap_or(cfg.mcp_port);
    format!("http://127.0.0.1:{}/mcp", port)
}

fn main() {
    let args: Vec<String> = env::args().collect();

    #[cfg(target_os = "windows")]
    if args.iter().any(|a| a == "--register" || a == "--unregister" || a == "--status") {
        ensure_console();
    }

    if args.iter().any(|a| a == "--mcp-server") {
        // MCP server mode: log to file (stderr is used by MCP transport)
        init_file_logging();
        let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
        if let Err(e) = rt.block_on(mcp::server::run_mcp_stdio_server()) {
            eprintln!("MCP server error: {}", e);
            std::process::exit(1);
        }
    } else if args.iter().any(|a| a == "--register") {
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
                    println!("Registered in {}: {}", client.name(), client.config_path().display());
                    any_ok = true;
                }
                Err(e) => eprintln!("Failed to register in {}: {}", client.name(), e),
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
        for (client, result) in claude_config::unregister_all() {
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
        println!("AutoPipe CLI");
        println!();
        println!("MCP server for bioinformatics pipeline management.");
        println!("Compatible with any MCP client that supports Streamable HTTP.");
        println!();
        println!("Usage:");
        println!("  autopipe --mcp-server   Run as stdio MCP server (spawned by AI clients)");
        println!("  autopipe --register     Register in supported AI clients");
        println!("  autopipe --unregister   Unregister from supported clients");
        println!("  autopipe --status       Show registration status and current URL");
        println!();
        println!("For the GUI app (configuration, tray icon, etc.), use the new");
        println!("AutoPipe desktop app at autopipe.org/getting-started.");
    }
}
