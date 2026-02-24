use serde_json::{json, Value};
use std::path::PathBuf;

/// Returns the Claude Desktop config file path for the current platform.
///
/// - macOS:   ~/Library/Application Support/Claude/claude_desktop_config.json
/// - Windows: %APPDATA%\Claude\claude_desktop_config.json
/// - Linux:   ~/.config/Claude/claude_desktop_config.json
pub fn claude_desktop_config_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Library/Application Support/Claude/claude_desktop_config.json")
    }
    #[cfg(target_os = "windows")]
    {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Claude")
            .join("claude_desktop_config.json")
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Claude")
            .join("claude_desktop_config.json")
    }
}

/// Check if Claude Desktop appears to be installed.
pub fn is_claude_desktop_installed() -> bool {
    let config_path = claude_desktop_config_path();
    // Check if the config directory exists (Claude Desktop creates it on install)
    config_path
        .parent()
        .map(|p| p.exists())
        .unwrap_or(false)
}

/// Register the autopipe MCP server in Claude Desktop's config.
pub fn register_mcp_server(config_path: &str) -> std::io::Result<()> {
    let path = claude_desktop_config_path();

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut root: Value = if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    let exe_path = std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("autopipe-desktop"))
        .to_string_lossy()
        .to_string();

    let mcp_entry = json!({
        "command": exe_path,
        "args": ["--mcp-server"],
        "env": {
            "AUTOPIPE_CONFIG": config_path
        }
    });

    let servers = root
        .as_object_mut()
        .unwrap()
        .entry("mcpServers")
        .or_insert_with(|| json!({}));

    servers
        .as_object_mut()
        .unwrap()
        .insert("autopipe".to_string(), mcp_entry);

    let content = serde_json::to_string_pretty(&root).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::Other, e)
    })?;
    std::fs::write(&path, content)?;

    Ok(())
}

/// Remove the autopipe MCP server from Claude Desktop's config.
pub fn unregister_mcp_server() -> std::io::Result<()> {
    let path = claude_desktop_config_path();
    if !path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&path)?;
    let mut root: Value = serde_json::from_str(&content).unwrap_or_else(|_| json!({}));

    if let Some(servers) = root.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
        servers.remove("autopipe");
    }

    let content = serde_json::to_string_pretty(&root).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::Other, e)
    })?;
    std::fs::write(&path, content)?;

    Ok(())
}

/// Check if autopipe MCP server is registered in Claude Desktop.
pub fn is_registered() -> bool {
    let path = claude_desktop_config_path();
    if !path.exists() {
        return false;
    }

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return false,
    };

    let root: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return false,
    };

    root.get("mcpServers")
        .and_then(|v| v.get("autopipe"))
        .is_some()
}
