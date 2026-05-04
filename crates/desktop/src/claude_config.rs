//! Auto-registration of the autopipe MCP HTTP server in supported AI clients.
//!
//! Each client has its own config file location and entry format. We dispatch
//! per-client. Currently auto-registered clients:
//!
//! - Claude Desktop (JSON, **stdio** — Claude Desktop's JSON config does
//!   not accept HTTP/SSE entries with localhost URLs, so we register the
//!   autopipe binary itself and let Claude Desktop spawn it as a child in
//!   `--mcp-server` mode)
//! - Gemini CLI (JSON, **`httpUrl` + headers** — note: not `url`)
//! - Codex CLI (TOML, **stdio** — Codex CLI rejects raw `bearer_token` for
//!   streamable_http, only accepting `bearer_token_env_var`. To avoid the
//!   environment-variable setup burden, we use stdio with `command + args`
//!   the same way as Claude Desktop)
//!
//! Detection: each client has an `is_installed()` heuristic that checks for
//! a config-dir or config-file marker. `register_all` only registers in
//! clients that look installed, to avoid creating files for apps the user
//! doesn't have. `register_all_force` exists for the rare case the user
//! wants to register everywhere regardless.
//!
//! Other MCP-compatible clients (Claude Code, Cursor, VS Code, Continue.dev,
//! ...) are not auto-registered. Users of those apps can paste the JSON
//! snippet produced by `mcp_config_snippet` into their config manually.

use serde_json::{json, Value};
use std::path::PathBuf;

/// Absolute path of the currently-running autopipe binary. Used as the
/// `command` for clients (notably Claude Desktop) whose JSON config only
/// accepts stdio entries — those clients spawn this same binary in
/// `--mcp-server` mode as a child process.
fn autopipe_binary_path() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("autopipe"))
}

/// Supported MCP client applications for auto-registration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum McpClient {
    ClaudeDesktop,
    GeminiCli,
    CodexCli,
}

impl McpClient {
    pub const ALL: &'static [McpClient] = &[
        McpClient::ClaudeDesktop,
        McpClient::GeminiCli,
        McpClient::CodexCli,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            McpClient::ClaudeDesktop => "Claude Desktop",
            McpClient::GeminiCli => "Gemini CLI",
            McpClient::CodexCli => "Codex CLI",
        }
    }

    pub fn config_path(&self) -> PathBuf {
        match self {
            McpClient::ClaudeDesktop => claude_desktop_config_path(),
            McpClient::GeminiCli => gemini_cli_config_path(),
            McpClient::CodexCli => codex_cli_config_path(),
        }
    }

    /// Best-effort detection of whether the client is installed on this
    /// machine. Used to skip auto-registration for clients the user isn't
    /// using, so we don't litter their home directory with config files.
    pub fn is_installed(&self) -> bool {
        let path = self.config_path();
        if path.exists() {
            return true;
        }
        // Parent dir presence is a strong signal that the app has been
        // installed/used at least once.
        path.parent().map(|p| p.exists()).unwrap_or(false)
    }
}

// ── Config file paths ───────────────────────────────────────────────────

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
        if let Some(local_data) = dirs::data_local_dir() {
            let packages = local_data.join("Packages");
            if packages.is_dir() {
                if let Ok(entries) = std::fs::read_dir(&packages) {
                    for entry in entries.flatten() {
                        let name = entry.file_name();
                        if name.to_string_lossy().starts_with("Claude_") {
                            let store_path = entry
                                .path()
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

pub fn gemini_cli_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".gemini")
        .join("settings.json")
}

pub fn codex_cli_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codex")
        .join("config.toml")
}

// ── Entry generators (per-client format) ─────────────────────────────────

/// Claude Desktop — stdio entry pointing at the autopipe binary itself,
/// invoked in `--mcp-server` mode. No external dependency (Node.js, npx,
/// mcp-remote) required. Claude Desktop spawns autopipe as a child
/// process and talks to it over stdin/stdout.
///
/// `_url` and `_token` are unused for stdio; they remain in the signature
/// so all client entry generators share the same shape.
fn claude_desktop_entry(_url: &str, _token: &str) -> Value {
    let exe = autopipe_binary_path();
    json!({
        "command": exe.to_string_lossy(),
        "args": ["--mcp-server"]
    })
}

/// Gemini CLI — uses `httpUrl` (not `url`!) + headers.
fn gemini_cli_entry(url: &str, token: &str) -> Value {
    json!({
        "httpUrl": url,
        "headers": {
            "Authorization": format!("Bearer {}", token)
        }
    })
}

// ── JSON-shape registration (Claude Desktop, Code, Cursor, Gemini) ───────

fn register_json_at(file_path: &PathBuf, entry: Value) -> std::io::Result<()> {
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut root: Value = if file_path.exists() {
        let content = std::fs::read_to_string(file_path)?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

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

    let content = serde_json::to_string_pretty(&root)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(file_path, content)
}

fn unregister_json_at(file_path: &PathBuf) -> std::io::Result<()> {
    if !file_path.exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(file_path)?;
    let mut root: Value = serde_json::from_str(&content).unwrap_or_else(|_| json!({}));
    if let Some(servers) = root.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
        servers.remove("autopipe");
    }
    let content = serde_json::to_string_pretty(&root)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(file_path, content)
}

fn is_registered_json(file_path: &PathBuf) -> bool {
    if !file_path.exists() {
        return false;
    }
    let Ok(content) = std::fs::read_to_string(file_path) else {
        return false;
    };
    let Ok(root) = serde_json::from_str::<Value>(&content) else {
        return false;
    };
    root.get("mcpServers")
        .and_then(|v| v.get("autopipe"))
        .is_some()
}

// ── TOML-shape registration (Codex CLI) ──────────────────────────────────

/// Codex CLI uses TOML and supports stdio entries with `command` + `args`.
/// We register the autopipe binary itself in `--mcp-server` mode, the same
/// way Claude Desktop does it. This avoids Codex's `bearer_token` validation
/// (raw token rejection) and the alternative `bearer_token_env_var` (which
/// requires the user to maintain a shell environment variable).
fn register_codex_toml(path: &PathBuf) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut root: toml::Table = if path.exists() {
        let content = std::fs::read_to_string(path)?;
        content.parse().unwrap_or_default()
    } else {
        toml::Table::new()
    };

    let servers = root
        .entry("mcp_servers".to_string())
        .or_insert(toml::Value::Table(toml::Table::new()));

    let servers = servers.as_table_mut().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "mcp_servers is not a TOML table",
        )
    })?;

    let exe = autopipe_binary_path();
    let mut autopipe = toml::Table::new();
    autopipe.insert(
        "command".into(),
        toml::Value::String(exe.to_string_lossy().into_owned()),
    );
    autopipe.insert(
        "args".into(),
        toml::Value::Array(vec![toml::Value::String("--mcp-server".into())]),
    );

    servers.insert("autopipe".into(), toml::Value::Table(autopipe));

    let content = toml::to_string_pretty(&root)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(path, content)
}

fn unregister_codex_toml(path: &PathBuf) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(path)?;
    let mut root: toml::Table = content.parse().unwrap_or_default();
    if let Some(servers) = root.get_mut("mcp_servers").and_then(|v| v.as_table_mut()) {
        servers.remove("autopipe");
    }
    let content = toml::to_string_pretty(&root)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(path, content)
}

fn is_registered_codex_toml(path: &PathBuf) -> bool {
    if !path.exists() {
        return false;
    }
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(root) = content.parse::<toml::Table>() else {
        return false;
    };
    root.get("mcp_servers")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("autopipe"))
        .is_some()
}

// ── Public API (per-client) ──────────────────────────────────────────────

pub fn register_mcp_server(client: McpClient, url: &str, token: &str) -> std::io::Result<()> {
    let path = client.config_path();
    match client {
        McpClient::ClaudeDesktop => register_json_at(&path, claude_desktop_entry(url, token)),
        McpClient::GeminiCli => register_json_at(&path, gemini_cli_entry(url, token)),
        McpClient::CodexCli => register_codex_toml(&path),
    }
}

pub fn unregister_mcp_server(client: McpClient) -> std::io::Result<()> {
    let path = client.config_path();
    match client {
        McpClient::CodexCli => unregister_codex_toml(&path),
        _ => unregister_json_at(&path),
    }
}

pub fn is_registered(client: McpClient) -> bool {
    let path = client.config_path();
    match client {
        McpClient::CodexCli => is_registered_codex_toml(&path),
        _ => is_registered_json(&path),
    }
}

// ── Bulk operations ──────────────────────────────────────────────────────

/// Register autopipe in all installed clients (heuristic detection).
/// Skips clients whose config dir/file is not present, to avoid creating
/// files for apps the user doesn't use.
pub fn register_all(url: &str, token: &str) -> Vec<(McpClient, std::io::Result<()>)> {
    McpClient::ALL
        .iter()
        .filter(|c| c.is_installed())
        .map(|c| (*c, register_mcp_server(*c, url, token)))
        .collect()
}

/// Register in every supported client, even those not detected.
/// Used when the user explicitly wants to set up a client they haven't
/// opened yet (e.g. installing Cursor right after autopipe).
pub fn register_all_force(url: &str, token: &str) -> Vec<(McpClient, std::io::Result<()>)> {
    McpClient::ALL
        .iter()
        .map(|c| (*c, register_mcp_server(*c, url, token)))
        .collect()
}

/// Re-register only clients that already have an autopipe entry.
/// Used when port or token rotates — preserves user's selection.
pub fn re_register_existing(url: &str, token: &str) -> Vec<(McpClient, std::io::Result<()>)> {
    McpClient::ALL
        .iter()
        .filter(|c| is_registered(**c))
        .map(|c| (*c, register_mcp_server(*c, url, token)))
        .collect()
}

pub fn unregister_all() -> Vec<(McpClient, std::io::Result<()>)> {
    McpClient::ALL
        .iter()
        .filter(|c| is_registered(**c))
        .map(|c| (*c, unregister_mcp_server(*c)))
        .collect()
}

pub fn status_all() -> Vec<(McpClient, bool)> {
    McpClient::ALL
        .iter()
        .map(|c| (*c, is_registered(*c)))
        .collect()
}

/// JSON snippet for any other MCP-compatible client the user wants to
/// register manually. Uses the most widely supported `type: "http"` form.
pub fn mcp_config_snippet(url: &str, token: &str) -> String {
    let snippet = json!({
        "mcpServers": {
            "autopipe": {
                "type": "http",
                "url": url,
                "headers": {
                    "Authorization": format!("Bearer {}", token)
                }
            }
        }
    });
    serde_json::to_string_pretty(&snippet).unwrap_or_default()
}
