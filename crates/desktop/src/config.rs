use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Default port for the local MCP HTTP server.
///
/// Chosen in the IANA dynamic/private range to avoid collision with common
/// development ports (3000/8000/8080/etc.) and well-known MCP ports (8765).
pub const DEFAULT_MCP_PORT: u16 = 47823;

/// Range of ports to try when the preferred port is occupied.
/// After this range fails, fall back to OS-assigned port (bind to 0).
pub const MCP_PORT_FALLBACK_RANGE: u16 = 20;

/// Length of the MCP authentication token in bytes (before base64 encoding).
pub const MCP_TOKEN_BYTES: usize = 32;

/// SSH authentication method.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SshAuth {
    #[serde(rename = "key")]
    Key { key_path: String },
    #[serde(rename = "password")]
    Password { password: String },
    #[serde(rename = "agent")]
    Agent,
}

impl Default for SshAuth {
    fn default() -> Self {
        SshAuth::Password {
            password: String::new(),
        }
    }
}

/// Application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Primary registry URL (kept for backward compatibility with MCP server).
    #[serde(default = "default_registry_url")]
    pub registry_url: String,
    /// List of registry URLs. The first one is the active/primary URL.
    #[serde(default = "default_registry_urls")]
    pub registry_urls: Vec<String>,
    pub ssh_host: String,
    pub ssh_port: u16,
    pub ssh_user: String,
    pub ssh_auth: SshAuth,
    pub repo_path: String,
    pub pipelines_dir: String,
    pub output_dir: String,
    pub input_dir: String,
    pub mcp_registered: bool,
    /// GitHub personal access token (obtained via device flow).
    #[serde(default)]
    pub github_token: Option<String>,
    /// GitHub repository name for uploads (default: "autopipe-hub"). Used when per_pipeline_repo is false.
    #[serde(default = "default_github_repo")]
    pub github_repo: String,
    /// When true, each pipeline gets its own GitHub repository. When false, all pipelines go into github_repo.
    #[serde(default)]
    pub per_pipeline_repo: bool,
    /// Local directory for viewer plugins (default: platform-specific data dir).
    #[serde(default = "default_plugins_dir")]
    pub plugins_dir: String,
    /// Preferred port for the local MCP HTTP server.
    /// If occupied at startup, the server falls back to nearby ports.
    #[serde(default = "default_mcp_port")]
    pub mcp_port: u16,
    /// Actual port the MCP server bound to last run. May differ from
    /// `mcp_port` when the preferred port was occupied.
    #[serde(default)]
    pub mcp_actual_port: Option<u16>,
    /// How the analysis machine is reached: "ssh" (self-managed server) or
    /// "cloud" (a VM in a cloud provider). Both use SSH under the hood; this
    /// only drives the setup UI. Empty is treated as "ssh".
    #[serde(default)]
    pub connection_type: String,
    /// Cloud provider when connection_type is "cloud": "aws" | "gcp" | "azure".
    #[serde(default)]
    pub cloud_provider: String,
    /// AWS credentials for cloud auto-provisioning (Phase 1: identity + bucket list).
    #[serde(default)]
    pub aws_access_key: String,
    #[serde(default)]
    pub aws_secret_key: String,
    #[serde(default)]
    pub aws_region: String,
    /// Selected S3 bucket for pipeline input/output.
    #[serde(default)]
    pub aws_bucket: String,
    /// AutoPipe-managed EC2 VM state (Phase 2).
    #[serde(default)]
    pub aws_instance_id: String,
    #[serde(default)]
    pub aws_sg_id: String,
    #[serde(default)]
    pub aws_key_name: String,
    #[serde(default)]
    pub aws_instance_type: String,
}

fn default_registry_url() -> String {
    "https://hub.autopipe.org".into()
}

fn default_registry_urls() -> Vec<String> {
    vec![
        "https://hub.autopipe.org".into(),
    ]
}


fn default_github_repo() -> String {
    "autopipe-hub".into()
}

fn default_mcp_port() -> u16 {
    DEFAULT_MCP_PORT
}

fn default_plugins_dir() -> String {
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").unwrap_or_default();
        if appdata.is_empty() {
            "plugins".into()
        } else {
            format!("{}\\autopipe\\plugins", appdata)
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        if home.is_empty() {
            "plugins".into()
        } else {
            format!("{}/.local/share/autopipe/plugins", home)
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            registry_url: default_registry_url(),
            registry_urls: default_registry_urls(),
            ssh_host: String::new(),
            ssh_port: 22,
            ssh_user: String::new(),
            ssh_auth: SshAuth::default(),
            repo_path: String::new(),
            pipelines_dir: "pipelines".into(),
            output_dir: "pipelines_output".into(),
            input_dir: "pipelines_input".into(),
            mcp_registered: false,
            github_token: None,
            github_repo: default_github_repo(),
            per_pipeline_repo: false,
            plugins_dir: default_plugins_dir(),
            mcp_port: DEFAULT_MCP_PORT,
            mcp_actual_port: None,
            connection_type: String::new(),
            cloud_provider: String::new(),
            aws_access_key: String::new(),
            aws_secret_key: String::new(),
            aws_region: String::new(),
            aws_bucket: String::new(),
            aws_instance_id: String::new(),
            aws_sg_id: String::new(),
            aws_key_name: String::new(),
            aws_instance_type: String::new(),
        }
    }
}

impl AppConfig {
    /// Config file path: ~/.config/autopipe-app/config.json
    pub fn config_path() -> PathBuf {
        let dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("autopipe-app");
        dir.join("config.json")
    }

    /// Load config from file, or return default.
    pub fn load() -> Self {
        let path = Self::config_path();
        let mut config = if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(_) => Self::default(),
            }
        } else {
            Self::default()
        };

        // Migrate old relative "plugins" default to platform-specific path
        if config.plugins_dir == "plugins" {
            config.plugins_dir = default_plugins_dir();
        }

        config
    }

    /// Resolve a path: if absolute, return as-is; if relative, join with repo_path.
    fn resolve_path(&self, path: &str) -> String {
        if path.starts_with('/') {
            path.to_string()
        } else if self.repo_path.is_empty() {
            path.to_string()
        } else {
            format!("{}/{}", self.repo_path.trim_end_matches('/'), path)
        }
    }

    /// Full path to pipelines directory on remote server.
    pub fn full_pipelines_dir(&self) -> String {
        self.resolve_path(&self.pipelines_dir)
    }

    /// Full path to output directory on remote server.
    pub fn full_output_dir(&self) -> String {
        self.resolve_path(&self.output_dir)
    }

    /// Full path to input directory on remote server.
    pub fn full_input_dir(&self) -> String {
        self.resolve_path(&self.input_dir)
    }

    /// Full path to local plugins directory.
    pub fn full_plugins_dir(&self) -> String {
        self.plugins_dir.clone()
    }

    /// Save config to file.
    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, e)
        })?;
        std::fs::write(&path, content)
    }
}

// ── MCP token management ─────────────────────────────────────────────────
//
// The MCP HTTP server requires a Bearer token. The token is generated on
// first run, stored next to config.json (with restricted perms on Unix),
// and reused thereafter. Rotating the token is an explicit user action
// that triggers re-registration of all clients.

/// Path to the MCP token file: ~/.config/autopipe-app/mcp_token
pub fn mcp_token_path() -> PathBuf {
    let dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("autopipe-app");
    dir.join("mcp_token")
}

/// Generate a fresh random token (base64-encoded, URL-safe).
fn generate_token() -> String {
    use base64::Engine;
    use rand::RngCore;

    let mut bytes = [0u8; MCP_TOKEN_BYTES];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Write the token to disk with restricted permissions (0600 on Unix).
fn write_token_file(path: &PathBuf, token: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, token)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(path, perms)?;
    }

    Ok(())
}

/// Load the MCP token from disk, generating and saving a new one if missing.
pub fn load_or_create_mcp_token() -> std::io::Result<String> {
    let path = mcp_token_path();
    if path.exists() {
        let token = std::fs::read_to_string(&path)?.trim().to_string();
        if !token.is_empty() {
            return Ok(token);
        }
    }
    let token = generate_token();
    write_token_file(&path, &token)?;
    Ok(token)
}

/// Force-rotate the MCP token. Returns the new token.
/// Callers must re-register clients after rotation.
pub fn regenerate_mcp_token() -> std::io::Result<String> {
    let token = generate_token();
    write_token_file(&mcp_token_path(), &token)?;
    Ok(token)
}
