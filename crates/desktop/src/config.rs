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
}

fn default_registry_url() -> String {
    "http://localhost:8090".into()
}

fn default_registry_urls() -> Vec<String> {
    vec!["http://localhost:8090".into()]
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
