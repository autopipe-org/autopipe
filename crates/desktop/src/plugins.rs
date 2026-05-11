//! Plugin management — install / list / uninstall viewer plugins.
//!
//! This module is shared by both the Tauri command layer (so the GUI can
//! manage plugins) and the MCP tool layer (for read-only listing). The
//! `install` flow itself is GUI-only — the corresponding MCP tool was
//! removed so plugin installation always goes through the AutoPipe app.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::mcp::server::parse_github_url;

/// Default plugins auto-installed on first launch.
pub const DEFAULT_PLUGIN_NAMES: &[&str] = &[
    "vcf-viewer",
    "bam-viewer",
    "bcf-viewer",
    "bed-viewer",
    "cram-viewer",
    "csv-viewer",
    "fasta-viewer",
    "fastq-viewer",
    "gff-viewer",
    "hdf5-viewer",
    "image-viewer",
    "pdf-viewer",
    "text-viewer",
];

/// Info about a plugin installed locally (from manifest.json).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPlugin {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub extensions: Vec<String>,
}

/// Info about a plugin available in the remote registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryPlugin {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub github_url: String,
}

/// Result of a successful install.
#[derive(Debug, Clone, Serialize)]
pub struct InstallResult {
    pub name: String,
    pub version: String,
    pub extensions: Vec<String>,
    pub location: String,
}

/// Scan `plugins_dir` for installed plugins by reading each subdirectory's
/// `manifest.json`. Directories without a valid manifest are skipped silently.
pub fn list_installed(plugins_dir: &Path) -> Vec<InstalledPlugin> {
    let mut out = Vec::new();
    if !plugins_dir.is_dir() {
        return out;
    }
    let entries = match std::fs::read_dir(plugins_dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest_path = path.join("manifest.json");
        let Ok(content) = std::fs::read_to_string(&manifest_path) else { continue };
        let Ok(p) = serde_json::from_str::<InstalledPlugin>(&content) else { continue };
        out.push(p);
    }
    out
}

/// Fetch the full plugin list from the registry.
pub async fn list_registry(registry_url: &str) -> Result<Vec<RegistryPlugin>, String> {
    let base = registry_url.trim_end_matches('/');
    let url = format!("{}/api/plugins", base);
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("Registry returned HTTP {}", resp.status()));
    }
    resp.json::<Vec<RegistryPlugin>>().await.map_err(|e| e.to_string())
}

/// Install (or re-install) a plugin from the registry by name.
///
/// Steps:
/// 1. Resolve plugin metadata by exact name match against the registry search API.
/// 2. Parse the GitHub URL, then fetch `manifest.json`, the entry script, and
///    the optional style file from `raw.githubusercontent.com`.
/// 3. Write all three files into `plugins_dir/<plugin_name>/`.
pub async fn install_one(
    plugin_name: &str,
    registry_url: &str,
    plugins_dir: &Path,
) -> Result<InstallResult, String> {
    let base = registry_url.trim_end_matches('/');
    let client = reqwest::Client::new();

    // 1. Resolve plugin in the registry.
    let encoded_name: String = plugin_name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c.to_string()
            } else {
                format!("%{:02X}", c as u32)
            }
        })
        .collect();
    let search_url = format!("{}/api/plugins/search?q={}", base, encoded_name);
    let plugins: Vec<serde_json::Value> = client
        .get(&search_url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let plugin = plugins
        .into_iter()
        .find(|p| p["name"].as_str() == Some(plugin_name))
        .ok_or_else(|| format!("Plugin '{}' not found in the registry.", plugin_name))?;

    let github_url = plugin["github_url"]
        .as_str()
        .ok_or_else(|| "Plugin has no GitHub URL in the registry.".to_string())?
        .to_string();

    // 2. Parse GitHub URL.
    let (gh_owner, gh_repo, gh_branch, gh_path) = parse_github_url(&github_url)
        .ok_or_else(|| format!("Invalid GitHub URL: {}", github_url))?;
    let branch = gh_branch.as_deref().unwrap_or("main");

    let raw_url = |file: &str| {
        if gh_path.is_empty() {
            format!(
                "https://raw.githubusercontent.com/{}/{}/{}/{}",
                gh_owner, gh_repo, branch, file
            )
        } else {
            format!(
                "https://raw.githubusercontent.com/{}/{}/{}/{}/{}",
                gh_owner, gh_repo, branch, gh_path, file
            )
        }
    };

    // 3. Download manifest.json.
    let manifest_resp = client
        .get(raw_url("manifest.json"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !manifest_resp.status().is_success() {
        return Err(format!(
            "Failed to download manifest.json: HTTP {}",
            manifest_resp.status()
        ));
    }
    let manifest_text = manifest_resp.text().await.map_err(|e| e.to_string())?;
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_text).map_err(|e| format!("Invalid manifest.json: {}", e))?;

    let resolved_name = manifest["name"].as_str().unwrap_or(plugin_name).to_string();
    let entry = manifest["entry"].as_str().unwrap_or("index.js").to_string();
    let style = manifest["style"].as_str().map(|s| s.to_string());

    // 4. Create plugin directory.
    let plugin_dir = plugins_dir.join(&resolved_name);
    std::fs::create_dir_all(&plugin_dir)
        .map_err(|e| format!("Failed to create plugin directory: {}", e))?;
    std::fs::write(plugin_dir.join("manifest.json"), &manifest_text)
        .map_err(|e| format!("Failed to write manifest.json: {}", e))?;

    // 5. Download entry script.
    let entry_resp = client
        .get(raw_url(&entry))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !entry_resp.status().is_success() {
        return Err(format!(
            "Failed to download {}: HTTP {}",
            entry,
            entry_resp.status()
        ));
    }
    let entry_bytes = entry_resp.bytes().await.map_err(|e| e.to_string())?;
    std::fs::write(plugin_dir.join(&entry), &entry_bytes)
        .map_err(|e| format!("Failed to write {}: {}", entry, e))?;

    // 6. Download optional style file (best effort — not fatal if missing).
    if let Some(style_file) = &style {
        if let Ok(resp) = client.get(raw_url(style_file)).send().await {
            if resp.status().is_success() {
                if let Ok(bytes) = resp.bytes().await {
                    let _ = std::fs::write(plugin_dir.join(style_file), &bytes);
                }
            }
        }
    }

    let version = manifest["version"].as_str().unwrap_or("?").to_string();
    let extensions: Vec<String> = manifest["extensions"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    Ok(InstallResult {
        name: resolved_name,
        version,
        extensions,
        location: plugin_dir.display().to_string(),
    })
}

/// Remove a plugin's directory. Returns Ok(()) if the directory did not exist.
pub fn uninstall_one(plugin_name: &str, plugins_dir: &Path) -> Result<(), String> {
    let plugin_dir = plugins_dir.join(plugin_name);
    if !plugin_dir.exists() {
        return Ok(());
    }
    std::fs::remove_dir_all(&plugin_dir)
        .map_err(|e| format!("Failed to remove plugin '{}': {}", plugin_name, e))
}

/// On first launch (or any launch with default plugins missing), install the
/// default plugin set from the registry. Errors are logged to stderr and
/// otherwise swallowed — the app must still start even if the registry is
/// unreachable.
pub async fn auto_install_defaults(registry_url: &str, plugins_dir: &Path) {
    let _ = std::fs::create_dir_all(plugins_dir);
    let missing: Vec<&str> = DEFAULT_PLUGIN_NAMES
        .iter()
        .filter(|name| !plugins_dir.join(name).exists())
        .copied()
        .collect();
    if missing.is_empty() {
        return;
    }
    for name in missing {
        match install_one(name, registry_url, plugins_dir).await {
            Ok(r) => eprintln!("auto-installed plugin '{}' v{}", r.name, r.version),
            Err(e) => eprintln!("failed to auto-install plugin '{}': {}", name, e),
        }
    }
}
