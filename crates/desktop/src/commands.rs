//! Tauri command bridge — exposes existing Rust functions to the Svelte
//! frontend. All actual business logic stays in the original modules
//! (config, claude_config, mcp::daemon, etc.); this file just provides
//! `#[tauri::command]` wrappers so the frontend can call them via
//! `invoke('command_name', { args })`.
//!
//! Conventions:
//! - All errors are stringified before crossing the JS boundary.
//! - State that needs to persist across calls (the MCP daemon handle, the
//!   GitHub device flow, etc.) lives in `AppState` managed by Tauri.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::claude_config;
use crate::config::{self, AppConfig, SshAuth};
use crate::mcp::daemon::{McpDaemonHandle, McpServerInfo};

// ── Shared application state ─────────────────────────────────────────────

/// Holds long-lived handles that span multiple command calls.
pub struct AppState {
    pub mcp: Mutex<Option<McpDaemonHandle>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            mcp: Mutex::new(None),
        }
    }
}

fn build_mcp_url(port: u16) -> String {
    format!("http://127.0.0.1:{}/mcp", port)
}

// ── DTOs (serializable for JS) ───────────────────────────────────────────

#[derive(Serialize)]
pub struct McpStatusDto {
    pub url: String,
    pub token: String,
    pub bound_port: u16,
    pub configured_port: u16,
}

impl From<&McpServerInfo> for McpStatusDto {
    fn from(info: &McpServerInfo) -> Self {
        Self {
            url: info.url.clone(),
            token: info.token.clone(),
            bound_port: info.bound_port,
            configured_port: info.configured_port,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct SshConfigDto {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub repo_path: String,
}

#[derive(Serialize)]
pub struct DeviceFlowDto {
    pub user_code: String,
    pub verification_uri: String,
}

// ── MCP daemon commands ──────────────────────────────────────────────────

#[tauri::command]
pub fn get_mcp_status(state: State<'_, AppState>) -> Option<McpStatusDto> {
    let guard = state.mcp.lock().ok()?;
    guard.as_ref()?.info().as_ref().map(McpStatusDto::from)
}

#[tauri::command]
pub fn set_mcp_port(
    state: State<'_, AppState>,
    port: u16,
) -> Result<(), String> {
    if port < 1024 {
        return Err("Port must be >= 1024".into());
    }
    let mut guard = state.mcp.lock().map_err(|e| e.to_string())?;
    if let Some(d) = guard.take() {
        d.shutdown();
        drop(d);
    }
    let mut cfg = AppConfig::load();
    cfg.mcp_port = port;
    cfg.mcp_actual_port = None;
    cfg.save().map_err(|e| e.to_string())?;

    let new_handle = McpDaemonHandle::start(port);
    // Wait briefly for the new daemon to bind so the next get_mcp_status sees it.
    for _ in 0..40 {
        if new_handle.is_settled() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    // Re-register existing clients with the new URL/token.
    if let Some(info) = new_handle.info() {
        let _ = claude_config::re_register_existing(&info.url, &info.token);
    }
    *guard = Some(new_handle);
    Ok(())
}

#[tauri::command]
pub fn rotate_mcp_token(state: State<'_, AppState>) -> Result<String, String> {
    let new_token = config::regenerate_mcp_token().map_err(|e| e.to_string())?;
    // Restart daemon so the in-memory token matches the file
    let port = {
        let guard = state.mcp.lock().map_err(|e| e.to_string())?;
        guard
            .as_ref()
            .and_then(|d| d.info())
            .map(|i| i.configured_port)
            .unwrap_or(AppConfig::load().mcp_port)
    };
    set_mcp_port(state, port)?;
    Ok(new_token)
}

// ── Claude / Codex / Gemini registration commands ────────────────────────

#[tauri::command]
pub fn register_mcp(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let info = {
        let guard = state.mcp.lock().map_err(|e| e.to_string())?;
        guard
            .as_ref()
            .and_then(|d| d.info())
            .ok_or_else(|| "MCP server not started yet".to_string())?
    };
    let results = claude_config::register_all(&info.url, &info.token);
    Ok(results
        .iter()
        .filter(|(_, r)| r.is_ok())
        .map(|(c, _)| c.name().to_string())
        .collect())
}

#[tauri::command]
pub fn unregister_mcp() -> Result<(), String> {
    let results = claude_config::unregister_all();
    for (client, result) in results {
        if let Err(e) = result {
            // Log but don't abort — best-effort across clients
            eprintln!("Failed to unregister {}: {}", client.name(), e);
        }
    }
    Ok(())
}

#[tauri::command]
pub fn registration_status() -> Vec<(String, bool)> {
    claude_config::status_all()
        .into_iter()
        .map(|(c, b)| (c.name().to_string(), b))
        .collect()
}

// ── SSH config commands ──────────────────────────────────────────────────

#[tauri::command]
pub fn get_ssh_config() -> SshConfigDto {
    let cfg = AppConfig::load();
    let password = match &cfg.ssh_auth {
        SshAuth::Password { password } => password.clone(),
        _ => String::new(),
    };
    SshConfigDto {
        host: cfg.ssh_host,
        port: cfg.ssh_port,
        user: cfg.ssh_user,
        password,
        repo_path: cfg.repo_path,
    }
}

#[tauri::command]
pub fn save_ssh_config(config: SshConfigDto) -> Result<(), String> {
    let mut cfg = AppConfig::load();
    cfg.ssh_host = config.host;
    cfg.ssh_port = config.port;
    cfg.ssh_user = config.user;
    cfg.repo_path = config.repo_path;
    cfg.ssh_auth = SshAuth::Password {
        password: config.password,
    };
    cfg.save().map_err(|e| e.to_string())
}

// ── GitHub commands ──────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_github_username() -> Option<String> {
    let cfg = AppConfig::load();
    let token = cfg.github_token?;
    if token.is_empty() {
        return None;
    }
    fetch_github_username(&token).await.ok()
}

#[tauri::command]
pub fn clear_github_token() -> Result<(), String> {
    let mut cfg = AppConfig::load();
    cfg.github_token = None;
    cfg.save().map_err(|e| e.to_string())
}

/// Start the GitHub device flow. Returns the user_code + verification_uri
/// immediately; once the polling loop succeeds, the backend emits the
/// `github-login-complete` event with the resolved username.
#[tauri::command]
pub async fn start_github_login(app: AppHandle) -> Result<DeviceFlowDto, String> {
    let cfg = AppConfig::load();
    let registry_url = cfg.registry_url.clone();
    let client = reqwest::Client::new();
    let base = registry_url.trim_end_matches('/');

    // 1. Request a device code from the registry's GitHub proxy
    let device_resp = client
        .post(format!("{}/api/github/device", base))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let device_body: serde_json::Value = device_resp.json().await.map_err(|e| e.to_string())?;
    let device_code = device_body["device_code"].as_str()
        .ok_or("missing device_code")?
        .to_string();
    let user_code = device_body["user_code"].as_str()
        .ok_or("missing user_code")?
        .to_string();
    let verification_uri = device_body["verification_uri"].as_str()
        .ok_or("missing verification_uri")?
        .to_string();
    let interval = device_body["interval"].as_u64().unwrap_or(5);

    // 2. Poll for token in the background; emit event when done
    let app_handle = app.clone();
    let base_owned = base.to_string();
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
            let resp = match client
                .post(format!("{}/api/github/poll", base_owned))
                .json(&serde_json::json!({ "device_code": device_code }))
                .send()
                .await
            {
                Ok(r) => r,
                Err(_) => continue,
            };
            let body: serde_json::Value = match resp.json().await {
                Ok(v) => v,
                Err(_) => continue,
            };
            if let Some(token) = body["access_token"].as_str() {
                // Save token to config
                let mut cfg = AppConfig::load();
                cfg.github_token = Some(token.to_string());
                let _ = cfg.save();
                // Resolve username and notify frontend
                let username = fetch_github_username(token).await.ok();
                let _ = app_handle.emit("github-login-complete", username);
                break;
            }
            if let Some(err) = body["error"].as_str() {
                if err != "authorization_pending" && err != "slow_down" {
                    let _ = app_handle.emit::<Option<String>>("github-login-error", Some(err.to_string()));
                    break;
                }
            }
        }
    });

    Ok(DeviceFlowDto {
        user_code,
        verification_uri,
    })
}

// ── Helpers ──────────────────────────────────────────────────────────────

async fn fetch_github_username(token: &str) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", "autopipe-desktop")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("GitHub API error: {}", resp.status()));
    }
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    body["login"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "No login field".to_string())
}

// ── Tauri builder helper ─────────────────────────────────────────────────

/// Convenience: register all commands with the Tauri builder.
#[allow(dead_code)]
pub fn register(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.invoke_handler(tauri::generate_handler![
        get_mcp_status,
        set_mcp_port,
        rotate_mcp_token,
        register_mcp,
        unregister_mcp,
        registration_status,
        get_ssh_config,
        save_ssh_config,
        get_github_username,
        clear_github_token,
        start_github_login,
    ])
}

/// Initialize the AppState (MCP daemon) at app start.
#[allow(dead_code)]
pub fn init_state(app: &AppHandle) {
    let cfg = AppConfig::load();
    let daemon = McpDaemonHandle::start(cfg.mcp_port);
    let state: State<AppState> = app.state();
    if let Ok(mut guard) = state.mcp.lock() {
        *guard = Some(daemon);
    }
}
