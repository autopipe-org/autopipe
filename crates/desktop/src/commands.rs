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
use crate::plugins;

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

/// Accept a port arriving as either a number or a string. Text inputs in the
/// UI hand back strings, and rejecting those turned a plain port edit into
/// "invalid type: string, expected u16".
fn deserialize_port<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::Number(n) => n
            .as_u64()
            .and_then(|v| u16::try_from(v).ok())
            .ok_or_else(|| D::Error::custom("port out of range")),
        serde_json::Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                return Ok(22);
            }
            s.parse::<u16>()
                .map_err(|_| D::Error::custom(format!("invalid port: {}", s)))
        }
        other => Err(D::Error::custom(format!("invalid port: {}", other))),
    }
}

#[derive(Serialize, Deserialize)]
pub struct SshConfigDto {
    pub host: String,
    #[serde(deserialize_with = "deserialize_port")]
    pub port: u16,
    pub user: String,
    /// "password" | "key" | "agent". Defaults to "password" for backward
    /// compatibility with older frontends that only sent a password.
    #[serde(default = "default_auth_method")]
    pub auth_method: String,
    #[serde(default)]
    pub password: String,
    /// Path to a private key file (e.g. a cloud VM's .pem) when auth_method is "key".
    #[serde(default)]
    pub key_path: String,
    pub repo_path: String,
    /// "ssh" (self-managed server) or "cloud" (a cloud VM). UI-only; both use SSH.
    #[serde(default = "default_connection_type")]
    pub connection_type: String,
    /// "aws" | "gcp" | "azure" when connection_type is "cloud".
    #[serde(default)]
    pub cloud_provider: String,
}

fn default_auth_method() -> String {
    "password".to_string()
}

fn default_connection_type() -> String {
    "ssh".to_string()
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

// NOTE: `unregister_mcp` and `registration_status` were exposed as Tauri
// commands for the old egui Status tab (Unregister button + per-client
// status indicators). The single-page Tauri+Svelte UI no longer surfaces
// them — `unregister` is still reachable via the `--unregister` CLI flag
// (handled in main.rs through `claude_config::unregister_all()` directly),
// and `--status` covers the registration indicator. Keep the bodies
// commented out so they're easy to revive if a Status panel comes back.
//
// #[tauri::command]
// pub fn unregister_mcp() -> Result<(), String> {
//     let results = claude_config::unregister_all();
//     for (client, result) in results {
//         if let Err(e) = result {
//             // Log but don't abort — best-effort across clients
//             eprintln!("Failed to unregister {}: {}", client.name(), e);
//         }
//     }
//     Ok(())
// }
//
// #[tauri::command]
// pub fn registration_status() -> Vec<(String, bool)> {
//     claude_config::status_all()
//         .into_iter()
//         .map(|(c, b)| (c.name().to_string(), b))
//         .collect()
// }

// ── Window / tray commands ───────────────────────────────────────────────

#[tauri::command]
pub fn move_to_tray(window: tauri::Window) -> Result<(), String> {
    window.hide().map_err(|e| e.to_string())
}

// ── Pipeline registry commands ───────────────────────────────────────────

#[tauri::command]
pub fn get_registry_url() -> String {
    AppConfig::load().registry_url
}

#[tauri::command]
pub fn set_registry_url(url: String) -> Result<(), String> {
    let mut cfg = AppConfig::load();
    cfg.registry_url = url.clone();
    // Mirror into the registry list so search/list endpoints see it too.
    if !cfg.registry_urls.contains(&url) {
        cfg.registry_urls.insert(0, url);
    }
    cfg.save().map_err(|e| e.to_string())
}

// ── SSH config commands ──────────────────────────────────────────────────

#[tauri::command]
pub fn get_ssh_config() -> SshConfigDto {
    let cfg = AppConfig::load();
    let (auth_method, password, key_path) = match &cfg.ssh_auth {
        SshAuth::Password { password } => ("password".to_string(), password.clone(), String::new()),
        SshAuth::Key { key_path } => ("key".to_string(), String::new(), key_path.clone()),
        SshAuth::Agent => ("agent".to_string(), String::new(), String::new()),
    };
    let connection_type = if cfg.connection_type.is_empty() {
        "ssh".to_string()
    } else {
        cfg.connection_type
    };
    SshConfigDto {
        host: cfg.ssh_host,
        port: cfg.ssh_port,
        user: cfg.ssh_user,
        auth_method,
        password,
        key_path,
        repo_path: cfg.repo_path,
        connection_type,
        cloud_provider: cfg.cloud_provider,
    }
}

#[tauri::command]
pub fn save_ssh_config(config: SshConfigDto) -> Result<(), String> {
    let mut cfg = AppConfig::load();
    cfg.ssh_host = config.host;
    cfg.ssh_port = config.port;
    cfg.ssh_user = config.user;
    cfg.repo_path = config.repo_path;
    cfg.ssh_auth = match config.auth_method.as_str() {
        "key" => SshAuth::Key {
            key_path: config.key_path,
        },
        "agent" => SshAuth::Agent,
        _ => SshAuth::Password {
            password: config.password,
        },
    };
    cfg.connection_type = config.connection_type;
    cfg.cloud_provider = config.cloud_provider;
    cfg.save().map_err(|e| e.to_string())
}

// ── AWS commands (cloud auto-provisioning, Phase 1) ──────────────────────

/// Verify AWS credentials (STS GetCallerIdentity) and, on success, save them.
/// Returns the AWS account ID so the UI can show "Connected as <account>".
#[tauri::command]
pub async fn aws_connect(
    access_key: String,
    secret_key: String,
    region: String,
) -> Result<String, String> {
    let region = if region.trim().is_empty() {
        "us-east-1".to_string()
    } else {
        region.trim().to_string()
    };
    let (account, username) =
        crate::aws::verify_credentials(&access_key, &secret_key, &region).await?;
    let mut cfg = AppConfig::load();
    cfg.aws_access_key = access_key.trim().to_string();
    cfg.aws_secret_key = secret_key.trim().to_string();
    cfg.aws_region = region;
    cfg.aws_user_name = username;
    cfg.save().map_err(|e| e.to_string())?;
    Ok(account)
}

/// List the connected account's S3 buckets (for the bucket picker).
#[tauri::command]
pub async fn aws_list_buckets() -> Result<Vec<String>, String> {
    let cfg = AppConfig::load();
    if cfg.aws_access_key.is_empty() {
        return Err("AWS is not connected. Enter your access key and click Connect first.".into());
    }
    crate::aws::list_buckets(&cfg.aws_access_key, &cfg.aws_secret_key, &cfg.aws_region).await
}

/// Persist the selected S3 bucket.
#[tauri::command]
pub fn aws_set_bucket(bucket: String) -> Result<(), String> {
    let mut cfg = AppConfig::load();
    cfg.aws_bucket = bucket.trim().to_string();
    cfg.save().map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct AwsConfigDto {
    pub region: String,
    pub bucket: String,
    pub user_name: String,
    /// Whether credentials are already saved (secret is never returned to the UI).
    pub has_credentials: bool,
}

/// Return saved AWS region/bucket/username + whether credentials exist, so the
/// setup UI can restore state without ever exposing the secret access key.
#[tauri::command]
pub fn aws_get_config() -> AwsConfigDto {
    let cfg = AppConfig::load();
    AwsConfigDto {
        region: cfg.aws_region,
        bucket: cfg.aws_bucket,
        user_name: cfg.aws_user_name,
        has_credentials: !cfg.aws_access_key.is_empty(),
    }
}

#[derive(Serialize)]
pub struct AwsVmDto {
    pub instance_id: String,
    pub public_ip: String,
}

/// Provision an EC2 VM in the user's account (Phase 2): create it with the
/// install user-data, wait until reachable, then auto-fill the SSH connection.
#[tauri::command]
pub async fn aws_provision() -> Result<AwsVmDto, String> {
    let cfg = AppConfig::load();
    if cfg.aws_access_key.is_empty() {
        return Err("Connect your AWS account first.".into());
    }
    let instance_type = if cfg.aws_instance_type.is_empty() {
        "t3.small".to_string()
    } else {
        cfg.aws_instance_type.clone()
    };
    let key_dir = AppConfig::config_path()
        .parent()
        .map(|p| p.join("keys").to_string_lossy().to_string())
        .unwrap_or_else(|| "keys".to_string());

    let res = crate::aws::provision_vm(
        &cfg.aws_access_key,
        &cfg.aws_secret_key,
        &cfg.aws_region,
        &instance_type,
        &key_dir,
    )
    .await?;

    // Persist managed state and auto-fill the SSH connection to the new VM.
    let mut cfg = AppConfig::load();
    cfg.aws_instance_id = res.instance_id.clone();
    cfg.aws_sg_id = res.sg_id.clone();
    cfg.aws_key_name = res.key_name.clone();
    cfg.aws_instance_type = instance_type;
    cfg.ssh_host = res.public_ip.clone();
    cfg.ssh_port = 22;
    cfg.ssh_user = "ubuntu".to_string();
    cfg.ssh_auth = SshAuth::Key {
        key_path: res.key_path.clone(),
    };
    if cfg.repo_path.trim().is_empty() {
        cfg.repo_path = "/home/ubuntu/autopipe".to_string();
    }
    cfg.save().map_err(|e| e.to_string())?;

    Ok(AwsVmDto {
        instance_id: res.instance_id,
        public_ip: res.public_ip,
    })
}

/// Terminate the managed VM (deletes it + its disk) and clear its saved state.
#[tauri::command]
pub async fn aws_teardown() -> Result<(), String> {
    let cfg = AppConfig::load();
    if cfg.aws_instance_id.is_empty() {
        return Err("No AutoPipe-managed VM to terminate.".into());
    }
    crate::aws::terminate_vm(
        &cfg.aws_access_key,
        &cfg.aws_secret_key,
        &cfg.aws_region,
        &cfg.aws_instance_id,
        &cfg.aws_sg_id,
        &cfg.aws_key_name,
    )
    .await?;
    let mut cfg = AppConfig::load();
    cfg.aws_instance_id = String::new();
    cfg.aws_sg_id = String::new();
    cfg.aws_key_name = String::new();
    cfg.save().map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct AwsVmStatusDto {
    pub provisioned: bool,
    pub instance_id: String,
    pub host: String,
}

/// Whether an AutoPipe-managed VM currently exists (for restoring UI state).
#[tauri::command]
pub fn aws_vm_status() -> AwsVmStatusDto {
    let cfg = AppConfig::load();
    AwsVmStatusDto {
        provisioned: !cfg.aws_instance_id.is_empty(),
        instance_id: cfg.aws_instance_id,
        host: cfg.ssh_host,
    }
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
pub fn get_github_repo() -> String {
    AppConfig::load().github_repo
}

#[tauri::command]
pub fn set_github_repo(repo: String) -> Result<(), String> {
    let trimmed = repo.trim().to_string();
    if trimmed.is_empty() {
        return Err("Repository name cannot be empty.".into());
    }
    let mut cfg = AppConfig::load();
    cfg.github_repo = trimmed;
    cfg.save().map_err(|e| e.to_string())
}

/// Read the `per_pipeline_repo` flag. When false (default), every pipeline
/// is uploaded into a subdirectory of the configured `github_repo`. When
/// true, the user picks a fresh repo for each pipeline at upload time.
/// This pair of commands restores the toggle that the old egui Setup tab
/// exposed; the Svelte GitHub panel needs the same control.
#[tauri::command]
pub fn get_per_pipeline_repo() -> bool {
    AppConfig::load().per_pipeline_repo
}

#[tauri::command]
pub fn set_per_pipeline_repo(value: bool) -> Result<(), String> {
    let mut cfg = AppConfig::load();
    cfg.per_pipeline_repo = value;
    cfg.save().map_err(|e| e.to_string())
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

    // 1. Request a device code from AutoPipeHub's auth proxy.
    // (The endpoint paths and JSON shape match the original egui flow in
    // app.rs::run_device_flow. Reading the response as text first gives a
    // clearer error message than reqwest's "error decoding response body".)
    let device_url = format!("{}/api/auth/device", base);
    let resp = client
        .post(&device_url)
        .send()
        .await
        .map_err(|e| format!("Request to {} failed: {}", device_url, e))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;
    let device_body: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        format!(
            "Invalid JSON from {} (status {}): {} — body: {}",
            device_url,
            status,
            e,
            &text[..text.len().min(200)]
        )
    })?;

    let device_code = device_body["device_code"]
        .as_str()
        .ok_or_else(|| {
            device_body["error"]
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "Missing device_code in response".to_string())
        })?
        .to_string();
    let user_code = device_body["user_code"]
        .as_str()
        .ok_or("Missing user_code")?
        .to_string();
    let verification_uri = device_body["verification_uri"]
        .as_str()
        .unwrap_or("https://github.com/login/device")
        .to_string();
    let interval = device_body["interval"].as_u64().unwrap_or(5);

    // 2. Poll for token in the background; emit an event when done.
    // Use Tauri's async runtime so the task is bound to the app lifetime
    // (a bare `tokio::spawn` can be dropped when the calling command
    // returns, depending on how the runtime is structured).
    // Surface errors via `github-login-error` instead of silently
    // continuing — silent loops were leaving the UI stuck on "Waiting
    // for you to authorize..." even after a successful browser auth.
    let app_handle = app.clone();
    let base_owned = base.to_string();
    tauri::async_runtime::spawn(async move {
        let client = reqwest::Client::new();
        let poll_url = format!("{}/api/auth/device/poll", base_owned);
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(interval)).await;

            let resp = match client
                .post(&poll_url)
                .json(&serde_json::json!({ "device_code": device_code }))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    let _ = app_handle.emit::<String>(
                        "github-login-error",
                        format!("Network error while polling: {}", e),
                    );
                    break;
                }
            };

            let status = resp.status();
            let text = match resp.text().await {
                Ok(t) => t,
                Err(e) => {
                    let _ = app_handle.emit::<String>(
                        "github-login-error",
                        format!("Failed to read poll response: {}", e),
                    );
                    break;
                }
            };

            let body: serde_json::Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(e) => {
                    let _ = app_handle.emit::<String>(
                        "github-login-error",
                        format!(
                            "Invalid JSON from poll endpoint (status {}): {} — body: {}",
                            status,
                            e,
                            &text[..text.len().min(200)]
                        ),
                    );
                    break;
                }
            };

            if let Some(token) = body["access_token"].as_str() {
                let mut cfg = AppConfig::load();
                cfg.github_token = Some(token.to_string());
                let _ = cfg.save();
                let username = fetch_github_username(token).await.ok();
                let _ = app_handle.emit("github-login-complete", username);
                break;
            }

            let err = body["error"].as_str().unwrap_or("");
            match err {
                "authorization_pending" | "slow_down" => continue,
                "expired_token" => {
                    let _ = app_handle.emit::<String>(
                        "github-login-error",
                        "Device code expired. Please try again.".into(),
                    );
                    break;
                }
                "" => {
                    // No access_token and no error key — unexpected shape.
                    let _ = app_handle.emit::<String>(
                        "github-login-error",
                        format!("Unexpected poll response: {}", &text[..text.len().min(200)]),
                    );
                    break;
                }
                other => {
                    let _ = app_handle
                        .emit::<String>("github-login-error", format!("Poll error: {}", other));
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

// ── Plugin management commands ───────────────────────────────────────────

#[tauri::command]
pub fn list_installed_plugins() -> Vec<plugins::InstalledPlugin> {
    let cfg = AppConfig::load();
    let dir = std::path::PathBuf::from(cfg.full_plugins_dir());
    plugins::list_installed(&dir)
}

#[tauri::command]
pub async fn list_registry_plugins() -> Result<Vec<plugins::RegistryPlugin>, String> {
    let cfg = AppConfig::load();
    plugins::list_registry(&cfg.registry_url).await
}

#[tauri::command]
pub async fn install_plugin(plugin_name: String) -> Result<plugins::InstallResult, String> {
    let cfg = AppConfig::load();
    let dir = std::path::PathBuf::from(cfg.full_plugins_dir());
    plugins::install_one(&plugin_name, &cfg.registry_url, &dir).await
}

#[tauri::command]
pub fn uninstall_plugin(plugin_name: String) -> Result<(), String> {
    let cfg = AppConfig::load();
    let dir = std::path::PathBuf::from(cfg.full_plugins_dir());
    plugins::uninstall_one(&plugin_name, &dir)
}

/// Update is a re-install — same flow, replaces files in place.
#[tauri::command]
pub async fn update_plugin(plugin_name: String) -> Result<plugins::InstallResult, String> {
    install_plugin(plugin_name).await
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
        // unregister_mcp,        // commented: no UI caller (see fn comment)
        // registration_status,   // commented: no UI caller (see fn comment)
        get_ssh_config,
        save_ssh_config,
        get_github_username,
        get_per_pipeline_repo,
        set_per_pipeline_repo,
        clear_github_token,
        start_github_login,
        list_installed_plugins,
        list_registry_plugins,
        install_plugin,
        uninstall_plugin,
        update_plugin,
    ])
}

/// Initialize the AppState (MCP daemon) at app start.
#[allow(dead_code)]
pub fn init_state(app: &AppHandle) {
    let cfg = AppConfig::load();
    let daemon = McpDaemonHandle::start(cfg.mcp_port);
    let state: State<AppState> = app.state();
    // Bind the lock result to a named variable first; using it directly
    // inside `if let` creates a temporary that the compiler thinks
    // outlives `state`.
    let lock_result = state.mcp.lock();
    if let Ok(mut guard) = lock_result {
        *guard = Some(daemon);
    }
}
