use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
        SshAuth::Agent
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
    pub mcp_registered: bool,
    /// GitHub personal access token (obtained via device flow).
    #[serde(default)]
    pub github_token: Option<String>,
    /// GitHub repository name for uploads (default: "autopipe-hub").
    #[serde(default = "default_github_repo")]
    pub github_repo: String,
    /// Local directory for viewer plugins (default: platform-specific data dir).
    #[serde(default = "default_plugins_dir")]
    pub plugins_dir: String,
}

fn default_registry_url() -> String {
    "http://localhost:8090".into()
}

fn default_registry_urls() -> Vec<String> {
    vec!["http://localhost:8090".into()]
}

fn default_github_repo() -> String {
    "autopipe-hub".into()
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
            mcp_registered: false,
            github_token: None,
            github_repo: default_github_repo(),
            plugins_dir: default_plugins_dir(),
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
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(_) => Self::default(),
            }
        } else {
            Self::default()
        }
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
