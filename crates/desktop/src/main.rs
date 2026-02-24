#[cfg(feature = "gui")]
mod app;
mod claude_config;
mod config;
mod mcp;
#[cfg(feature = "gui")]
mod ssh;
#[cfg(feature = "gui")]
mod tray;

use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.iter().any(|a| a == "--mcp-server") {
        // MCP server mode: Claude Desktop invokes this via stdio
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .init();
        let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
        if let Err(e) = rt.block_on(mcp::server::run_mcp_server()) {
            eprintln!("MCP server error: {}", e);
            std::process::exit(1);
        }
    } else if args.iter().any(|a| a == "--register") {
        // Register MCP server in Claude Desktop config
        let config_path = config::AppConfig::config_path();
        match claude_config::register_mcp_server(&config_path.to_string_lossy()) {
            Ok(_) => {
                let dest = claude_config::claude_desktop_config_path();
                println!("MCP server registered in Claude Desktop config:");
                println!("  {}", dest.display());
                println!();
                println!("Restart Claude Desktop to load autopipe tools.");
            }
            Err(e) => {
                eprintln!("Failed to register: {}", e);
                std::process::exit(1);
            }
        }
    } else if args.iter().any(|a| a == "--unregister") {
        // Unregister MCP server from Claude Desktop config
        match claude_config::unregister_mcp_server() {
            Ok(_) => println!("MCP server unregistered from Claude Desktop."),
            Err(e) => {
                eprintln!("Failed to unregister: {}", e);
                std::process::exit(1);
            }
        }
    } else if args.iter().any(|a| a == "--status") {
        // Check registration status
        let dest = claude_config::claude_desktop_config_path();
        println!("Config path: {}", dest.display());
        if claude_config::is_registered() {
            println!("MCP server: registered");
        } else {
            println!("MCP server: not registered");
        }
        if claude_config::is_claude_desktop_installed() {
            println!("Claude Desktop: detected");
        } else {
            println!("Claude Desktop: not detected");
        }
    } else {
        #[cfg(feature = "gui")]
        {
            run_gui();
        }
        #[cfg(not(feature = "gui"))]
        {
            println!("AutoPipe Desktop");
            println!();
            println!("Usage:");
            println!("  desktop --mcp-server    Run as MCP server (for Claude Desktop)");
            println!("  desktop --register      Register MCP in Claude Desktop");
            println!("  desktop --unregister    Unregister MCP from Claude Desktop");
            println!("  desktop --status        Check registration status");
            println!();
            println!("GUI mode requires: cargo build --features gui");
            println!("(needs GTK development libraries on Linux)");
        }
    }
}

#[cfg(feature = "gui")]
fn run_gui() {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([550.0, 500.0])
            .with_title("AutoPipe"),
        ..Default::default()
    };

    eframe::run_native(
        "AutoPipe",
        options,
        Box::new(|cc| Ok(Box::new(app::AutoPipeApp::new(cc)))),
    )
    .expect("Failed to start eGUI");
}
