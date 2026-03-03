use serde_json::{json, Value};
use std::path::PathBuf;

/// Supported MCP client applications.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum McpClient {
    ClaudeDesktop,
    GeminiCli,
}

impl McpClient {
    /// All supported clients.
    pub const ALL: &'static [McpClient] = &[McpClient::ClaudeDesktop, McpClient::GeminiCli];

    /// Display name.
    pub fn name(&self) -> &'static str {
        match self {
            McpClient::ClaudeDesktop => "Claude Desktop",
            McpClient::GeminiCli => "Gemini CLI",
        }
    }

    /// Config file path for this client.
    pub fn config_path(&self) -> PathBuf {
        match self {
            McpClient::ClaudeDesktop => claude_desktop_config_path(),
            McpClient::GeminiCli => gemini_cli_config_path(),
        }
    }
}

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

/// Returns the Gemini CLI config file path.
/// All platforms: ~/.gemini/settings.json
pub fn gemini_cli_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".gemini")
        .join("settings.json")
}

/// Build the MCP server entry JSON (same format for Claude Desktop and Gemini CLI).
fn mcp_entry(config_path: &str) -> Value {
    let exe_path = std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("autopipe-desktop"))
        .to_string_lossy()
        .to_string();

    json!({
        "command": exe_path,
        "args": ["--mcp-server"],
        "env": {
            "AUTOPIPE_CONFIG": config_path
        }
    })
}

/// Register the autopipe MCP server in a JSON config that uses the `mcpServers` key.
/// Works for both Claude Desktop and Gemini CLI.
fn register_json_mcp(file_path: &PathBuf, config_path: &str) -> std::io::Result<()> {
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut root: Value = if file_path.exists() {
        let content = std::fs::read_to_string(file_path)?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    let entry = mcp_entry(config_path);

    let servers = root
        .as_object_mut()
        .unwrap()
        .entry("mcpServers")
        .or_insert_with(|| json!({}));

    servers
        .as_object_mut()
        .unwrap()
        .insert("autopipe".to_string(), entry);

    let content = serde_json::to_string_pretty(&root).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::Other, e)
    })?;
    std::fs::write(file_path, content)?;

    Ok(())
}

/// Unregister the autopipe MCP server from a JSON config that uses the `mcpServers` key.
fn unregister_json_mcp(file_path: &PathBuf) -> std::io::Result<()> {
    if !file_path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(file_path)?;
    let mut root: Value = serde_json::from_str(&content).unwrap_or_else(|_| json!({}));

    if let Some(servers) = root.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
        servers.remove("autopipe");
    }

    let content = serde_json::to_string_pretty(&root).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::Other, e)
    })?;
    std::fs::write(file_path, content)?;

    Ok(())
}

/// Check if autopipe is registered in a JSON config that uses the `mcpServers` key.
fn is_registered_json(file_path: &PathBuf) -> bool {
    if !file_path.exists() {
        return false;
    }

    let content = match std::fs::read_to_string(file_path) {
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

// ── Public API (per-client) ──────────────────────────────────────────────

/// Register autopipe in the given MCP client.
pub fn register_mcp_server(client: McpClient, config_path: &str) -> std::io::Result<()> {
    let file = client.config_path();
    register_json_mcp(&file, config_path)
}

/// Unregister autopipe from the given MCP client.
pub fn unregister_mcp_server(client: McpClient) -> std::io::Result<()> {
    let file = client.config_path();
    unregister_json_mcp(&file)
}

/// Check if autopipe is registered in the given MCP client.
pub fn is_registered(client: McpClient) -> bool {
    let file = client.config_path();
    is_registered_json(&file)
}

// ── Bulk operations ──────────────────────────────────────────────────────

/// Register autopipe in all supported MCP clients.
pub fn register_all(config_path: &str) -> Vec<(McpClient, std::io::Result<()>)> {
    McpClient::ALL
        .iter()
        .map(|c| (*c, register_mcp_server(*c, config_path)))
        .collect()
}

/// Unregister autopipe from all supported MCP clients.
pub fn unregister_all() -> Vec<(McpClient, std::io::Result<()>)> {
    McpClient::ALL
        .iter()
        .map(|c| (*c, unregister_mcp_server(*c)))
        .collect()
}

/// Check registration status for all supported MCP clients.
pub fn status_all() -> Vec<(McpClient, bool)> {
    McpClient::ALL
        .iter()
        .map(|c| (*c, is_registered(*c)))
        .collect()
}
