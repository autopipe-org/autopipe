use serde_json::{json, Value};
use std::path::PathBuf;

/// Supported MCP client applications for auto-registration.
///
/// Other MCP-compatible clients (Cursor, VS Code, Gemini CLI, Codex CLI, ...)
/// can connect using the "Copy MCP config snippet" button in the settings UI;
/// they don't need a dedicated entry here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum McpClient {
    ClaudeDesktop,
    ClaudeCode,
}

impl McpClient {
    /// All supported clients.
    pub const ALL: &'static [McpClient] = &[McpClient::ClaudeDesktop, McpClient::ClaudeCode];

    /// Display name.
    pub fn name(&self) -> &'static str {
        match self {
            McpClient::ClaudeDesktop => "Claude Desktop",
            McpClient::ClaudeCode => "Claude Code",
        }
    }

    /// Config file path for this client.
    pub fn config_path(&self) -> PathBuf {
        match self {
            McpClient::ClaudeDesktop => claude_desktop_config_path(),
            McpClient::ClaudeCode => claude_code_config_path(),
        }
    }
}

/// Returns the Claude Desktop config file path for the current platform.
///
/// - macOS:   ~/Library/Application Support/Claude/claude_desktop_config.json
/// - Windows: %APPDATA%\Claude\claude_desktop_config.json
///            or %LOCALAPPDATA%\Packages\Claude_*\LocalCache\Roaming\Claude\ (MS Store)
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
        let normal = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Claude")
            .join("claude_desktop_config.json");
        if normal.exists() {
            return normal;
        }

        // Check MS Store path: %LOCALAPPDATA%\Packages\Claude_*\LocalCache\Roaming\Claude\
        if let Some(local_data) = dirs::data_local_dir() {
            let packages = local_data.join("Packages");
            if packages.is_dir() {
                if let Ok(entries) = std::fs::read_dir(&packages) {
                    for entry in entries.flatten() {
                        let name = entry.file_name();
                        if name.to_string_lossy().starts_with("Claude_") {
                            let store_path = entry.path()
                                .join("LocalCache")
                                .join("Roaming")
                                .join("Claude")
                                .join("claude_desktop_config.json");
                            if store_path.exists() {
                                return store_path;
                            }
                        }
                    }
                }
            }
        }

        // Neither found — return normal path for new config creation
        normal
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Claude")
            .join("claude_desktop_config.json")
    }
}

/// Returns the Claude Code config file path for the current platform.
///
/// All platforms: ~/.claude.json
pub fn claude_code_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude.json")
}

/// Build the MCP server entry JSON for an HTTP-transport server.
///
/// Format used by Claude Desktop, Claude Code, Cursor, VS Code, and any
/// MCP-compatible client that supports Streamable HTTP transport.
fn mcp_entry(url: &str, token: &str) -> Value {
    json!({
        "type": "http",
        "url": url,
        "headers": {
            "Authorization": format!("Bearer {}", token)
        }
    })
}

/// Public helper for the "Copy MCP config snippet" UI button.
/// Returns a pretty-printed JSON snippet that users can paste into any
/// MCP-compatible client's config.
pub fn mcp_config_snippet(url: &str, token: &str) -> String {
    let snippet = json!({
        "mcpServers": {
            "autopipe": mcp_entry(url, token)
        }
    });
    serde_json::to_string_pretty(&snippet).unwrap_or_default()
}

/// Register the autopipe MCP server in a JSON config that uses the `mcpServers` key.
fn register_json_mcp(file_path: &PathBuf, url: &str, token: &str) -> std::io::Result<()> {
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut root: Value = if file_path.exists() {
        let content = std::fs::read_to_string(file_path)?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    let entry = mcp_entry(url, token);

    if !root.is_object() {
        root = json!({});
    }

    let servers = root
        .as_object_mut()
        .expect("root is object")
        .entry("mcpServers")
        .or_insert_with(|| json!({}));

    if !servers.is_object() {
        *servers = json!({});
    }

    servers
        .as_object_mut()
        .expect("mcpServers is object")
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
pub fn register_mcp_server(client: McpClient, url: &str, token: &str) -> std::io::Result<()> {
    let file = client.config_path();
    register_json_mcp(&file, url, token)
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
pub fn register_all(url: &str, token: &str) -> Vec<(McpClient, std::io::Result<()>)> {
    McpClient::ALL
        .iter()
        .map(|c| (*c, register_mcp_server(*c, url, token)))
        .collect()
}

/// Re-register autopipe in only the clients that already have an entry.
/// Used when port or token changes — we only update existing registrations
/// rather than registering everywhere.
pub fn re_register_existing(url: &str, token: &str) -> Vec<(McpClient, std::io::Result<()>)> {
    McpClient::ALL
        .iter()
        .filter(|c| is_registered(**c))
        .map(|c| (*c, register_mcp_server(*c, url, token)))
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
