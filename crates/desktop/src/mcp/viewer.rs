#![allow(dead_code)]

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Json},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::config::AppConfig;
use crate::ssh;
use common::models::clean_content;

/// Shared state: stores files keyed by filename.
#[derive(Clone)]
struct ViewerState {
    files: Arc<Mutex<HashMap<String, FileEntry>>>,
    plugins: Arc<Mutex<Arc<Vec<PluginManifest>>>>,
}

struct FileEntry {
    data: Vec<u8>,
    mime: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct DataSource {
    /// "docker" or "text"
    #[serde(rename = "type")]
    source_type: String,
    /// Docker image name (only for type=docker)
    #[serde(default)]
    image: String,
    /// Command template to count rows. Placeholders: {file}
    #[serde(default)]
    row_count: String,
    /// Command template to extract metadata/header. Placeholders: {file}
    #[serde(default)]
    metadata: String,
    /// Command template to extract data rows. Placeholders: {file}, {start}, {end}
    #[serde(default)]
    rows: String,
    /// Column headers (static list). If empty, parsed from metadata.
    #[serde(default)]
    col_headers: Vec<String>,
    /// How to parse metadata output: "vcf_style" (#CHROM + ## lines), "bam_style" (@SQ headers), or "none"
    #[serde(default = "default_meta_parse")]
    meta_parse: String,
    /// Accept non-zero exit codes (e.g. BAM SIGPIPE from head)
    #[serde(default)]
    allow_nonzero_exit: bool,
    /// Fallback data sources to try if this one fails (e.g. Docker fallback when CLI tool missing)
    #[serde(default)]
    fallback: Vec<DataSource>,
}

fn default_meta_parse() -> String {
    "none".to_string()
}

#[derive(Clone, Serialize)]
struct PluginManifest {
    name: String,
    version: String,
    description: String,
    extensions: Vec<String>,
    entry: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data_source: Option<DataSource>,
}

/// A running viewer server handle.
pub struct ViewerHandle {
    port: u16,
    shutdown: tokio::sync::oneshot::Sender<()>,
}

impl ViewerHandle {
    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn shutdown(self) {
        let _ = self.shutdown.send(());
    }
}

/// Global viewer instance (reused across calls).
static VIEWER: tokio::sync::OnceCell<Arc<Mutex<Option<ViewerHandle>>>> =
    tokio::sync::OnceCell::const_new();

async fn get_viewer_lock() -> &'static Arc<Mutex<Option<ViewerHandle>>> {
    VIEWER
        .get_or_init(|| async { Arc::new(Mutex::new(None)) })
        .await
}

/// Shared file store (persists across server restarts within the same process).
static FILE_STORE: tokio::sync::OnceCell<Arc<Mutex<HashMap<String, FileEntry>>>> =
    tokio::sync::OnceCell::const_new();

async fn get_file_store() -> &'static Arc<Mutex<HashMap<String, FileEntry>>> {
    FILE_STORE
        .get_or_init(|| async { Arc::new(Mutex::new(HashMap::new())) })
        .await
}

/// Shared plugins data.
static PLUGINS: tokio::sync::OnceCell<Arc<Mutex<Arc<Vec<PluginManifest>>>>> =
    tokio::sync::OnceCell::const_new();

async fn get_plugins_lock() -> &'static Arc<Mutex<Arc<Vec<PluginManifest>>>> {
    PLUGINS
        .get_or_init(|| async { Arc::new(Mutex::new(Arc::new(Vec::new()))) })
        .await
}

/// Shared plugins directory path.
static PLUGINS_DIR: tokio::sync::OnceCell<Arc<Mutex<String>>> =
    tokio::sync::OnceCell::const_new();

async fn get_plugins_dir_lock() -> &'static Arc<Mutex<String>> {
    PLUGINS_DIR
        .get_or_init(|| async { Arc::new(Mutex::new(String::new())) })
        .await
}

/// Shared reference info (genome ID or FASTA filename).
static REFERENCE: tokio::sync::OnceCell<Arc<Mutex<Option<String>>>> =
    tokio::sync::OnceCell::const_new();

async fn get_reference_lock() -> &'static Arc<Mutex<Option<String>>> {
    REFERENCE
        .get_or_init(|| async { Arc::new(Mutex::new(None)) })
        .await
}

/// Remote file entry — files that stay on the remote server (not transferred).
struct RemoteFileEntry {
    remote_path: String,
    size: u64,
    mime: String,
}

/// SSH config for on-demand remote data fetching.
static SSH_CONFIG: tokio::sync::OnceCell<Arc<Mutex<Option<AppConfig>>>> =
    tokio::sync::OnceCell::const_new();

async fn get_ssh_config_lock() -> &'static Arc<Mutex<Option<AppConfig>>> {
    SSH_CONFIG
        .get_or_init(|| async { Arc::new(Mutex::new(None)) })
        .await
}

/// Remote files that are NOT transferred — filename → remote path + metadata.
static REMOTE_FILES: tokio::sync::OnceCell<Arc<Mutex<HashMap<String, RemoteFileEntry>>>> =
    tokio::sync::OnceCell::const_new();

async fn get_remote_files_lock() -> &'static Arc<Mutex<HashMap<String, RemoteFileEntry>>> {
    REMOTE_FILES
        .get_or_init(|| async { Arc::new(Mutex::new(HashMap::new())) })
        .await
}

/// Cache for total row counts of remote files (to avoid re-counting on every page).
static ROW_COUNT_CACHE: tokio::sync::OnceCell<Arc<Mutex<HashMap<String, usize>>>> =
    tokio::sync::OnceCell::const_new();

async fn get_row_count_cache() -> &'static Arc<Mutex<HashMap<String, usize>>> {
    ROW_COUNT_CACHE
        .get_or_init(|| async { Arc::new(Mutex::new(HashMap::new())) })
        .await
}

/// The canonicalized output directory the viewer is allowed to browse. All
/// /api/browse and /api/browse-open requests are sandboxed under this root.
/// `None` disables in-viewer directory navigation (snapshot-only mode).
static BROWSE_ROOT: tokio::sync::OnceCell<Arc<Mutex<Option<String>>>> =
    tokio::sync::OnceCell::const_new();

async fn get_browse_root_lock() -> &'static Arc<Mutex<Option<String>>> {
    BROWSE_ROOT
        .get_or_init(|| async { Arc::new(Mutex::new(None)) })
        .await
}

/// Resolve `requested` to its canonical path on the remote server and verify
/// it stays within `root`. Returns the canonical path on success, or `None`
/// when the path escapes the sandbox or cannot be resolved. Using the remote
/// `realpath` means symlinks are followed, so a symlink pointing outside the
/// output directory is correctly rejected.
async fn realpath_within_root(
    ssh_cfg: &AppConfig,
    root: &str,
    requested: &str,
) -> Option<String> {
    // Resolve the requested path. `realpath -m` resolves even when the final
    // component does not exist yet, but we only ever browse existing paths so
    // a plain `realpath` is fine and additionally guarantees existence.
    let cmd = format!("realpath '{}' 2>/dev/null", requested.replace('\'', "'\\''"));
    let resolved = match ssh_run(ssh_cfg, &cmd).await {
        Ok((out, 0)) => clean_content(&out).trim().to_string(),
        _ => return None,
    };
    if resolved.is_empty() {
        return None;
    }
    let root_trimmed = root.trim_end_matches('/');
    if resolved == root_trimmed || resolved.starts_with(&format!("{}/", root_trimmed)) {
        Some(resolved)
    } else {
        None
    }
}

/// Scan local plugins directory, reading manifest.json from each subdirectory.
fn scan_plugins(plugins_dir: &str) -> Vec<PluginManifest> {
    let mut plugins = Vec::new();
    let dir = std::path::Path::new(plugins_dir);
    if !dir.is_dir() {
        return plugins;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let manifest_path = path.join("manifest.json");
            if let Ok(content) = std::fs::read_to_string(&manifest_path) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
                    let name = v["name"].as_str().unwrap_or_default().to_string();
                    if name.is_empty() {
                        continue;
                    }
                    let data_source: Option<DataSource> = v.get("data_source")
                        .and_then(|ds| serde_json::from_value(ds.clone()).ok());
                    plugins.push(PluginManifest {
                        name,
                        version: v["version"].as_str().unwrap_or("0.0.0").to_string(),
                        description: v["description"].as_str().unwrap_or("").to_string(),
                        extensions: v["extensions"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default(),
                        entry: v["entry"].as_str().unwrap_or("index.js").to_string(),
                        style: v["style"].as_str().map(|s| s.to_string()),
                        data_source,
                    });
                }
            }
        }
    }
    plugins
}

/// Start the viewer server (or reuse existing one).
/// Files and plugins are shared via Arc so the running server sees updates.
async fn ensure_server(plugins_dir: &str) -> Result<u16, String> {
    let lock = get_viewer_lock().await;
    let mut handle = lock.lock().await;

    // Reuse existing server — files/plugins are shared via Arc<Mutex>
    if let Some(ref h) = *handle {
        return Ok(h.port());
    }

    let files = get_file_store().await.clone();
    let plugins = get_plugins_lock().await.clone();
    let state = ViewerState { files, plugins };

    // Store the plugins_dir for serving plugin assets
    {
        let mut dir = get_plugins_dir_lock().await.lock().await;
        *dir = plugins_dir.to_string();
    }

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/logo.png", get(logo_handler))
        .route("/api/files", get(files_list_handler))
        .route("/api/reference", get(reference_handler))
        .route("/api/browse", get(browse_handler))
        .route("/api/browse-open", get(browse_open_handler))
        .route("/file/{filename}", get(file_handler))
        .route("/data/{filename}", get(data_handler))
        .route("/plugin/{name}/{*path}", get(plugin_asset_handler))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("Failed to bind: {}", e))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("Failed to get addr: {}", e))?
        .port();

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();

    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await
            .ok();
    });

    *handle = Some(ViewerHandle {
        port,
        shutdown: tx,
    });

    Ok(port)
}

/// Add files and open browser. Returns the URL.
/// `reference` can be a local FASTA filename (present in files), or a genome ID like "hg38", "mm10".
/// `remote_files`: (filename, remote_path, size, mime) — files kept on remote server, fetched on demand.
/// `ssh_config`: SSH credentials for on-demand remote data fetching.
pub async fn show_files(
    files: Vec<(String, Vec<u8>, String)>,
    remote_files: Vec<(String, String, u64, String)>,
    plugins_dir: String,
    reference: Option<String>,
    ssh_config: Option<AppConfig>,
    browse_root: Option<String>,
) -> Result<String, String> {
    // Update file store
    let store = get_file_store().await;
    {
        let mut map = store.lock().await;
        map.clear();
        for (name, data, mime) in files {
            map.insert(name, FileEntry { data, mime });
        }
    }

    // Update remote file store
    {
        let mut rmap = get_remote_files_lock().await.lock().await;
        rmap.clear();
        for (name, remote_path, size, mime) in remote_files {
            rmap.insert(name, RemoteFileEntry { remote_path, size, mime });
        }
    }

    // Store SSH config
    {
        let mut cfg = get_ssh_config_lock().await.lock().await;
        *cfg = ssh_config;
    }

    // Store the browse root (sandbox boundary for in-viewer navigation).
    {
        let mut root = get_browse_root_lock().await.lock().await;
        *root = browse_root;
    }

    // Clear row count cache (new file set)
    {
        let mut cache = get_row_count_cache().await.lock().await;
        cache.clear();
    }

    // Scan and update plugins
    let scanned = scan_plugins(&plugins_dir);
    {
        let mut p = get_plugins_lock().await.lock().await;
        *p = Arc::new(scanned);
    }

    // Store reference info
    {
        let mut r = get_reference_lock().await.lock().await;
        *r = reference;
    }

    let port = ensure_server(&plugins_dir).await?;

    let url = format!("http://127.0.0.1:{}", port);

    open::that(&url).map_err(|e| format!("Failed to open browser: {}", e))?;

    Ok(url)
}

// ── Handlers ────────────────────────────────────────────────────────

const LOGO_PNG: &[u8] = include_bytes!("../../../../web/static/logo.png");

async fn logo_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "image/png".to_string())],
        LOGO_PNG.to_vec(),
    )
}

/// API: return file list as JSON (includes both local and remote files).
async fn files_list_handler(State(state): State<ViewerState>) -> Json<Vec<FileListItem>> {
    let files = state.files.lock().await;
    let mut items: Vec<FileListItem> = files
        .iter()
        .map(|(name, entry)| FileListItem {
            name: name.clone(),
            mime: entry.mime.clone(),
            size: entry.data.len() as u64,
            remote: false,
        })
        .collect();
    drop(files);

    // Add remote files (exclude index files from viewer list — they remain accessible via /file/ endpoint)
    let index_exts = ["bai", "tbi", "csi", "crai", "fai", "idx"];
    let remote = get_remote_files_lock().await.lock().await;
    for (name, entry) in remote.iter() {
        let ext = name.rsplit('.').next().map(|e| e.to_lowercase()).unwrap_or_default();
        if index_exts.contains(&ext.as_str()) {
            continue;
        }
        items.push(FileListItem {
            name: name.clone(),
            mime: entry.mime.clone(),
            size: entry.size,
            remote: true,
        });
    }

    items.sort_by(|a, b| a.name.cmp(&b.name));
    Json(items)
}

#[derive(Serialize)]
struct FileListItem {
    name: String,
    mime: String,
    size: u64,
    remote: bool,
}

/// API: return reference info as JSON.
async fn reference_handler() -> Json<serde_json::Value> {
    let r = get_reference_lock().await.lock().await;
    match &*r {
        Some(ref_val) => Json(serde_json::json!({ "reference": ref_val })),
        None => Json(serde_json::json!({ "reference": null })),
    }
}

/// Query parameters for the /api/browse and /api/browse-open endpoints.
#[derive(Deserialize)]
struct BrowseQuery {
    /// Absolute path to browse (directory) or open (file). When omitted on
    /// /api/browse, the browse root itself is listed.
    #[serde(default)]
    dir: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Serialize)]
struct BrowseEntry {
    name: String,
    /// Full remote path of this entry.
    path: String,
    is_dir: bool,
    size: u64,
}

#[derive(Serialize)]
struct BrowseListing {
    /// Whether browsing is available (browse root configured).
    enabled: bool,
    /// The configured sandbox root.
    root: String,
    /// The directory currently being listed.
    cwd: String,
    /// Parent directory path, or null when `cwd` is the root (cannot go up).
    parent: Option<String>,
    entries: Vec<BrowseEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// API: list a directory under the browse root. Sandboxed — any path that
/// resolves outside the root is rejected.
async fn browse_handler(Query(query): Query<BrowseQuery>) -> Json<BrowseListing> {
    let root = {
        let r = get_browse_root_lock().await.lock().await;
        r.clone()
    };
    let root = match root {
        Some(r) => r,
        None => {
            return Json(BrowseListing {
                enabled: false,
                root: String::new(),
                cwd: String::new(),
                parent: None,
                entries: Vec::new(),
                error: None,
            });
        }
    };

    let ssh_cfg = {
        let lock = get_ssh_config_lock().await.lock().await;
        lock.clone()
    };
    let ssh_cfg = match ssh_cfg {
        Some(c) => c,
        None => {
            return Json(BrowseListing {
                enabled: true,
                root: root.clone(),
                cwd: root.clone(),
                parent: None,
                entries: Vec::new(),
                error: Some("SSH not configured".into()),
            });
        }
    };

    // Default to the root when no dir is given.
    let requested = query.dir.unwrap_or_else(|| root.clone());

    // Sandbox: resolve and verify within root.
    let cwd = match realpath_within_root(&ssh_cfg, &root, &requested).await {
        Some(p) => p,
        None => {
            return Json(BrowseListing {
                enabled: true,
                root: root.clone(),
                cwd: root.clone(),
                parent: None,
                entries: Vec::new(),
                error: Some("Path is outside the allowed output directory".into()),
            });
        }
    };

    // List directory entries (name, type, size). Use a NUL-safe-ish format:
    // one entry per line as "<type>\t<size>\t<name>".
    let list_cmd = format!(
        "cd '{}' && for f in * .[!.]*; do [ -e \"$f\" ] || continue; if [ -d \"$f\" ]; then echo \"d\\t0\\t$f\"; else echo \"f\\t$(stat -c%s \"$f\" 2>/dev/null || stat -f%z \"$f\" 2>/dev/null)\\t$f\"; fi; done",
        cwd.replace('\'', "'\\''")
    );
    let entries: Vec<BrowseEntry> = match ssh_run(&ssh_cfg, &list_cmd).await {
        Ok((out, _)) => clean_content(&out)
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.splitn(3, '\t').collect();
                if parts.len() != 3 {
                    return None;
                }
                let is_dir = parts[0] == "d";
                let size: u64 = parts[1].parse().unwrap_or(0);
                let name = parts[2].to_string();
                if name.is_empty() {
                    return None;
                }
                Some(BrowseEntry {
                    name: name.clone(),
                    path: format!("{}/{}", cwd.trim_end_matches('/'), name),
                    is_dir,
                    size,
                })
            })
            .collect(),
        Err(_) => Vec::new(),
    };

    // Compute parent only if cwd is strictly inside the root.
    let root_trimmed = root.trim_end_matches('/').to_string();
    let parent = if cwd == root_trimmed {
        None
    } else {
        cwd.rsplit_once('/').map(|(p, _)| {
            if p.is_empty() { "/".to_string() } else { p.to_string() }
        })
    };

    Json(BrowseListing {
        enabled: true,
        root: root_trimmed,
        cwd,
        parent,
        entries,
        error: None,
    })
}

/// API: register a file (under the browse root) into the remote file store
/// so it can be rendered on demand through the existing /file/ + plugin
/// pipeline. Returns the filename key to use. Sandboxed.
async fn browse_open_handler(Query(query): Query<BrowseQuery>) -> Json<serde_json::Value> {
    let root = {
        let r = get_browse_root_lock().await.lock().await;
        r.clone()
    };
    let root = match root {
        Some(r) => r,
        None => return Json(serde_json::json!({ "error": "Browsing is not enabled" })),
    };
    let requested = match query.path {
        Some(p) => p,
        None => return Json(serde_json::json!({ "error": "Missing path" })),
    };

    let ssh_cfg = {
        let lock = get_ssh_config_lock().await.lock().await;
        lock.clone()
    };
    let ssh_cfg = match ssh_cfg {
        Some(c) => c,
        None => return Json(serde_json::json!({ "error": "SSH not configured" })),
    };

    let resolved = match realpath_within_root(&ssh_cfg, &root, &requested).await {
        Some(p) => p,
        None => {
            return Json(serde_json::json!({
                "error": "Path is outside the allowed output directory"
            }));
        }
    };

    // Reject directories — only files can be opened.
    let is_file = matches!(
        ssh_run(&ssh_cfg, &format!("test -f '{}' && echo OK", resolved.replace('\'', "'\\''"))).await,
        Ok((out, 0)) if clean_content(&out).trim() == "OK"
    );
    if !is_file {
        return Json(serde_json::json!({ "error": "Not a file" }));
    }

    let filename = std::path::Path::new(&resolved)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    let ext = filename.rsplit('.').next().map(|e| e.to_lowercase()).unwrap_or_default();
    let mime = mime_for_ext(&ext);

    // Stat the size.
    let size: u64 = match ssh_run(
        &ssh_cfg,
        &format!(
            "stat -c%s '{}' 2>/dev/null || stat -f%z '{}' 2>/dev/null",
            resolved.replace('\'', "'\\''"),
            resolved.replace('\'', "'\\''")
        ),
    )
    .await
    {
        Ok((s, 0)) => clean_content(&s).trim().parse().unwrap_or(0),
        _ => 0,
    };

    // Register into the remote file store so /file/{filename} + /data/{filename}
    // serve it lazily through the existing pipeline.
    {
        let mut rmap = get_remote_files_lock().await.lock().await;
        rmap.insert(
            filename.clone(),
            RemoteFileEntry { remote_path: resolved.clone(), size, mime: mime.to_string() },
        );
    }
    // Drop any stale cached row count for this filename (file may have changed).
    {
        let mut cache = get_row_count_cache().await.lock().await;
        cache.remove(&filename);
        cache.remove(&format!("__ds_method__{}", filename));
    }

    Json(serde_json::json!({
        "filename": filename,
        "mime": mime,
        "size": size,
        "remote": true,
    }))
}

/// Map a file extension to a MIME type. Shared by browse-open and the
/// snapshot loader's classification.
fn mime_for_ext(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "tiff" | "tif" => "image/tiff",
        "pdf" => "application/pdf",
        "txt" | "log" | "md" | "sh" | "py" | "r" | "nf" | "smk" | "cfg" | "ini" | "toml" => "text/plain",
        "csv" | "tsv" => "text/csv",
        "json" => "application/json",
        "yaml" | "yml" => "text/yaml",
        "xml" => "text/xml",
        "html" | "htm" => "text/html",
        "fastq" | "fq" | "fasta" | "fa" => "text/plain",
        _ => "application/octet-stream",
    }
}

/// Query parameters for the /data/ endpoint (server-side pagination).
#[derive(Deserialize)]
struct DataQuery {
    #[serde(default)]
    page: Option<usize>,
    #[serde(default)]
    page_size: Option<usize>,
}

/// Build an SSH command from a DataSource template.
/// For "docker" type: wraps the command template in `docker run --rm -v "dir:/data:ro" image sh -c "cmd" 2>/dev/null`
/// For "text" type: uses the command template directly with the remote file path.
/// Placeholders: {file} → filename or remote_path, {start} → start line, {end} → end line
fn build_data_cmd(
    ds: &DataSource,
    template: &str,
    remote_path: &str,
    start: usize,
    end: usize,
) -> String {
    if ds.source_type == "docker" {
        let dir = std::path::Path::new(remote_path)
            .parent()
            .unwrap_or(std::path::Path::new("/"))
            .to_string_lossy()
            .to_string();
        let file = std::path::Path::new(remote_path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let cmd = template
            .replace("{file}", &format!("/data/{}", file))
            .replace("{start}", &start.to_string())
            .replace("{end}", &end.to_string());
        format!(
            "docker run --rm -v \"{}:/data:ro\" {} sh -c \"{}\" 2>/dev/null",
            dir, ds.image, cmd
        )
    } else {
        // text type: {file} = full remote path
        template
            .replace("{file}", remote_path)
            .replace("{start}", &start.to_string())
            .replace("{end}", &end.to_string())
    }
}

/// Find plugin data_source for a given file extension.
async fn find_data_source(ext: &str) -> Option<DataSource> {
    let plugins_lock = get_plugins_lock().await.lock().await;
    let plugins = plugins_lock.clone();
    for p in plugins.iter() {
        if p.extensions.iter().any(|e| e == ext) {
            if let Some(ref ds) = p.data_source {
                return Some(ds.clone());
            }
        }
    }
    None
}

/// Helper: run SSH command via spawn_blocking.
async fn ssh_run(config: &AppConfig, cmd: &str) -> Result<(String, i32), String> {
    let config = config.clone();
    // Wrap in login shell so ~/.bash_profile / conda / module PATH is loaded
    let escaped = cmd.replace('\'', "'\\''");
    let cmd = format!("bash -l -c '{}'", escaped);
    let (output, code) = tokio::task::spawn_blocking(move || ssh::ssh_exec(&config, &cmd))
        .await
        .map_err(|e| format!("Task error: {}", e))??;
    Ok((clean_content(&output), code))
}

/// Data handler: server-side pagination for genomics files.
/// If the plugin defines a `data_source` in manifest.json, uses those commands.
/// Otherwise falls back to built-in handlers for VCF/BED/GFF (text formats).
/// GET /data/{filename}?page=0&page_size=100
async fn data_handler(
    Path(filename): Path<String>,
    Query(query): Query<DataQuery>,
) -> impl IntoResponse {
    let page = query.page.unwrap_or(0);
    let page_size = query.page_size.unwrap_or(100);

    // Look up remote file
    let remote_files = get_remote_files_lock().await.lock().await;
    let entry = match remote_files.get(&filename) {
        Some(e) => (e.remote_path.clone(), e.mime.clone()),
        None => {
            return Json(serde_json::json!({"error": "File not found in remote files"}))
                .into_response();
        }
    };
    drop(remote_files);
    let (remote_path, _mime) = entry;

    // Get SSH config
    let ssh_cfg_lock = get_ssh_config_lock().await.lock().await;
    let ssh_cfg = match &*ssh_cfg_lock {
        Some(c) => c.clone(),
        None => {
            return Json(serde_json::json!({"error": "SSH not configured"})).into_response();
        }
    };
    drop(ssh_cfg_lock);

    let ext = filename
        .rsplit('.')
        .next()
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    let start = page * page_size + 1; // sed is 1-indexed
    let end = start + page_size - 1;

    // Try to find plugin data_source for this extension
    let ds = find_data_source(&ext).await;

    // ── Plugin-driven path: data_source found in manifest ──
    if let Some(ref ds) = ds {
        // Build candidate list: primary + fallbacks
        let mut candidates = vec![ds.clone()];
        candidates.extend(ds.fallback.clone());

        // Check if we already know which candidate works (cached index)
        let method_key = format!("__ds_method__{}", filename);
        let cached_idx = {
            let cache = get_row_count_cache().await.lock().await;
            cache.get(&method_key).copied()
        };

        // Find working data_source: try each candidate until one succeeds
        let working_ds = if let Some(idx) = cached_idx {
            // Use cached working candidate
            candidates.get(idx).cloned()
        } else {
            // Probe each candidate with row_count command
            let mut found: Option<(usize, DataSource)> = None;
            for (i, candidate) in candidates.iter().enumerate() {
                if candidate.row_count.is_empty() {
                    // No probe command — assume it works (text files)
                    found = Some((i, candidate.clone()));
                    break;
                }
                let cmd = build_data_cmd(candidate, &candidate.row_count, &remote_path, 0, 0);
                match ssh_run(&ssh_cfg, &cmd).await {
                    Ok((output, code)) => {
                        if code == 0 || candidate.allow_nonzero_exit {
                            // Verify we got a parseable number
                            if output.trim().parse::<usize>().is_ok() {
                                found = Some((i, candidate.clone()));
                                break;
                            }
                        }
                    }
                    Err(_) => continue,
                }
            }
            if let Some((idx, ref _ds)) = found {
                let mut cache = get_row_count_cache().await.lock().await;
                cache.insert(method_key.clone(), idx);
            }
            found.map(|(_, ds)| ds)
        };

        let active_ds = match working_ds {
            Some(ds) => ds,
            None => {
                // All candidates failed — return error
                return Json(serde_json::json!({
                    "error": "No working data source found. Check that required tools are installed on the server."
                })).into_response();
            }
        };

        // 1. Row count (cached)
        let total = {
            let cache = get_row_count_cache().await.lock().await;
            cache.get(&filename).copied()
        };
        let total = match total {
            Some(t) => t,
            None => {
                if !active_ds.row_count.is_empty() {
                    let cmd = build_data_cmd(&active_ds, &active_ds.row_count, &remote_path, 0, 0);
                    match ssh_run(&ssh_cfg, &cmd).await {
                        Ok((output, 0)) => {
                            let count: usize = output.trim().parse().unwrap_or(0);
                            let mut cache = get_row_count_cache().await.lock().await;
                            cache.insert(filename.clone(), count);
                            count
                        }
                        _ => 0,
                    }
                } else {
                    0
                }
            }
        };

        // 2. Metadata (first page only)
        let mut meta = serde_json::Value::Null;
        let mut header_val = serde_json::Value::Null;
        let mut refs_val = serde_json::Value::Null;
        let mut col_headers: Vec<String> = active_ds.col_headers.clone();

        if page == 0 && !active_ds.metadata.is_empty() {
            let meta_cmd = build_data_cmd(&active_ds, &active_ds.metadata, &remote_path, 0, 0);
            if let Ok((m, 0)) = ssh_run(&ssh_cfg, &meta_cmd).await {
                match active_ds.meta_parse.as_str() {
                    "bam_style" => {
                        // Full header
                        header_val = serde_json::Value::String(m.trim().to_string());
                        // Parse @SQ lines for references
                        let mut refs = Vec::new();
                        for line in m.trim().lines() {
                            if line.starts_with("@SQ") {
                                let mut name = String::new();
                                let mut length: u64 = 0;
                                for field in line.split('\t') {
                                    if let Some(val) = field.strip_prefix("SN:") {
                                        name = val.to_string();
                                    } else if let Some(val) = field.strip_prefix("LN:") {
                                        length = val.parse().unwrap_or(0);
                                    }
                                }
                                if !name.is_empty() {
                                    refs.push(serde_json::json!({"name": name, "length": length}));
                                }
                            }
                        }
                        if !refs.is_empty() {
                            refs_val = serde_json::Value::Array(refs);
                        }
                    }
                    "vcf_style" => {
                        // Parse #CHROM header line + ## meta lines
                        let lines: Vec<&str> = m.trim().lines().collect();
                        if col_headers.is_empty() {
                            if let Some(hdr_line) = lines.iter().find(|l| l.starts_with("#CHROM")) {
                                col_headers = hdr_line
                                    .trim_start_matches('#')
                                    .split('\t')
                                    .map(|s| s.to_string())
                                    .collect();
                            }
                        }
                        let meta_lines: Vec<&str> =
                            lines.iter().filter(|l| l.starts_with("##")).copied().collect();
                        if !meta_lines.is_empty() {
                            meta = serde_json::Value::String(meta_lines.join("\n"));
                        }
                    }
                    _ => {
                        // "none" or unknown: just pass raw metadata
                        if !m.trim().is_empty() {
                            meta = serde_json::Value::String(m.trim().to_string());
                        }
                    }
                }
            }
        }

        // 3. Data rows
        let rows_cmd = build_data_cmd(&active_ds, &active_ds.rows, &remote_path, start, end);
        let rows: Vec<Vec<String>> = match ssh_run(&ssh_cfg, &rows_cmd).await {
            Ok((output, code)) => {
                if code != 0 && !active_ds.allow_nonzero_exit {
                    return Json(serde_json::json!({"error": output.trim()})).into_response();
                }
                output
                    .trim()
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(|line| line.split('\t').map(|s| s.to_string()).collect())
                    .collect()
            }
            Err(e) => {
                return Json(serde_json::json!({"error": e})).into_response();
            }
        };

        let mut result = serde_json::json!({
            "rows": rows,
            "total": total,
            "page": page,
            "page_size": page_size,
        });
        if !meta.is_null() { result["meta"] = meta; }
        if !header_val.is_null() { result["header"] = header_val; }
        if !refs_val.is_null() { result["refs"] = refs_val; }
        if !col_headers.is_empty() { result["col_headers"] = serde_json::json!(col_headers); }
        return Json(result).into_response();
    }

    // ── Fallback: built-in handlers for formats without plugin data_source ──

    let start = page * page_size + 1;
    let end = start + page_size - 1;

    // Row count (cached)
    let total = {
        let cache = get_row_count_cache().await.lock().await;
        cache.get(&filename).copied()
    };
    let total = match total {
        Some(t) => t,
        None => {
            let count_cmd = match ext.as_str() {
                "vcf" => format!("grep -c -v '^#' '{}'", remote_path),
                "bed" => format!(
                    "grep -c -v -E '^#|^track|^browser' '{}'",
                    remote_path
                ),
                "gff" | "gtf" | "gff3" => format!("grep -c -v '^#' '{}'", remote_path),
                _ => format!("wc -l < '{}'", remote_path),
            };
            match ssh_run(&ssh_cfg, &count_cmd).await {
                Ok((output, 0)) => {
                    let count: usize = output.trim().parse().unwrap_or(0);
                    let mut cache = get_row_count_cache().await.lock().await;
                    cache.insert(filename.clone(), count);
                    count
                }
                _ => 0,
            }
        }
    };

    // Metadata (first page only)
    let mut meta = serde_json::Value::Null;
    let mut col_headers: Vec<String> = Vec::new();

    if page == 0 {
        match ext.as_str() {
            "vcf" => {
                if let Ok((m, 0)) =
                    ssh_run(&ssh_cfg, &format!("grep '^#' '{}'", remote_path)).await
                {
                    let lines: Vec<&str> = m.trim().lines().collect();
                    if let Some(hdr_line) = lines.iter().find(|l| l.starts_with("#CHROM")) {
                        col_headers = hdr_line
                            .trim_start_matches('#')
                            .split('\t')
                            .map(|s| s.to_string())
                            .collect();
                    }
                    let meta_lines: Vec<&str> =
                        lines.iter().filter(|l| l.starts_with("##")).copied().collect();
                    if !meta_lines.is_empty() {
                        meta = serde_json::Value::String(meta_lines.join("\n"));
                    }
                }
            }
            "bed" => {
                let bed_cols = [
                    "chrom", "chromStart", "chromEnd", "name", "score", "strand",
                    "thickStart", "thickEnd", "itemRgb", "blockCount", "blockSizes", "blockStarts",
                ];
                if let Ok((first_line, 0)) = ssh_run(
                    &ssh_cfg,
                    &format!("grep -v -E '^#|^track|^browser' '{}' | head -1", remote_path),
                ).await {
                    let ncols = first_line.trim().split('\t').count().min(12);
                    col_headers = bed_cols[..ncols].iter().map(|s| s.to_string()).collect();
                }
            }
            "gff" | "gtf" | "gff3" => {
                col_headers = vec![
                    "seqid", "source", "type", "start", "end", "score", "strand", "phase", "attributes",
                ].into_iter().map(|s| s.to_string()).collect();
                if let Ok((m, 0)) =
                    ssh_run(&ssh_cfg, &format!("grep '^#' '{}'", remote_path)).await
                {
                    if !m.trim().is_empty() {
                        meta = serde_json::Value::String(m.trim().to_string());
                    }
                }
            }
            _ => {}
        }
    }

    // Data rows
    let rows_cmd = match ext.as_str() {
        "vcf" => format!("grep -v '^#' '{}' | sed -n '{},{}p'", remote_path, start, end),
        "bed" => format!("grep -v -E '^#|^track|^browser' '{}' | sed -n '{},{}p'", remote_path, start, end),
        "gff" | "gtf" | "gff3" => format!("grep -v '^#' '{}' | sed -n '{},{}p'", remote_path, start, end),
        _ => format!("sed -n '{},{}p' '{}'", start, end, remote_path),
    };

    let rows: Vec<Vec<String>> = match ssh_run(&ssh_cfg, &rows_cmd).await {
        Ok((output, code)) => {
            if code != 0 {
                return Json(serde_json::json!({"error": output.trim()})).into_response();
            }
            output
                .trim()
                .lines()
                .filter(|l| !l.is_empty())
                .map(|line| line.split('\t').map(|s| s.to_string()).collect())
                .collect()
        }
        Err(e) => {
            return Json(serde_json::json!({"error": e})).into_response();
        }
    };

    let mut result = serde_json::json!({
        "rows": rows,
        "total": total,
        "page": page,
        "page_size": page_size,
    });

    if !meta.is_null() {
        result["meta"] = meta;
    }
    if !col_headers.is_empty() {
        result["col_headers"] = serde_json::json!(col_headers);
    }

    Json(result).into_response()
}

/// Serve plugin assets from the local plugins directory.
async fn plugin_asset_handler(Path((name, path)): Path<(String, String)>) -> impl IntoResponse {
    let plugins_dir = get_plugins_dir_lock().await.lock().await.clone();
    let file_path = std::path::Path::new(&plugins_dir).join(&name).join(&path);

    // Security: prevent directory traversal
    let canonical = match file_path.canonicalize() {
        Ok(p) => p,
        Err(_) => return (StatusCode::NOT_FOUND, [(header::CONTENT_TYPE, "text/plain".to_string())], b"Not found".to_vec()).into_response(),
    };
    let base = match std::path::Path::new(&plugins_dir).canonicalize() {
        Ok(p) => p,
        Err(_) => return (StatusCode::NOT_FOUND, [(header::CONTENT_TYPE, "text/plain".to_string())], b"Not found".to_vec()).into_response(),
    };
    if !canonical.starts_with(&base) {
        return (StatusCode::FORBIDDEN, [(header::CONTENT_TYPE, "text/plain".to_string())], b"Forbidden".to_vec()).into_response();
    }

    match std::fs::read(&canonical) {
        Ok(data) => {
            let mime = match canonical.extension().and_then(|e| e.to_str()) {
                Some("js") => "application/javascript",
                Some("css") => "text/css",
                Some("json") => "application/json",
                Some("html") => "text/html",
                Some("png") => "image/png",
                Some("jpg") | Some("jpeg") => "image/jpeg",
                Some("svg") => "image/svg+xml",
                Some("wasm") => "application/wasm",
                _ => "application/octet-stream",
            };
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime.to_string())],
                data,
            )
                .into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, [(header::CONTENT_TYPE, "text/plain".to_string())], b"Not found".to_vec()).into_response(),
    }
}

async fn index_handler(State(state): State<ViewerState>) -> Html<String> {
    // Read plugins dynamically so updates are visible without server restart
    let plugins = state.plugins.lock().await.clone();
    let plugins_json = serde_json::to_string(&*plugins).unwrap_or_else(|_| "[]".into());

    Html(format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Autopipe Viewer</title>
<link rel="icon" href="/logo.png" type="image/png">
<style>
  * {{ box-sizing: border-box; margin: 0; padding: 0; }}
  html, body {{ height: 100%; overflow: hidden; }}
  body {{ font-family: 'SF Mono', 'Fira Code', 'Consolas', monospace; background: #fafafa; color: #111; line-height: 1.5; display: flex; flex-direction: column; }}

  /* Header */
  .header {{ padding: 12px 24px; border-bottom: 1px solid #e5e5e5; background: #fff; display: flex; align-items: center; gap: 12px; flex-shrink: 0; }}
  .logo {{ font-size: 1.1rem; font-weight: 700; color: #111; letter-spacing: -0.02em; display: flex; align-items: center; gap: 8px; }}
  .logo-icon {{ height: 22px; width: auto; }}
  .header-sub {{ font-size: 13px; color: #999; font-weight: 400; }}

  /* Layout */
  .layout {{ display: flex; flex: 1; overflow: hidden; }}

  /* Sidebar */
  .sidebar {{ width: 260px; min-width: 260px; background: #fff; border-right: 1px solid #e5e5e5; display: flex; flex-direction: column; overflow: hidden; }}
  .sidebar-header {{ padding: 16px 16px 12px; font-size: 12px; font-weight: 600; color: #888; text-transform: uppercase; letter-spacing: 0.04em; border-bottom: 1px solid #f0f0f0; }}
  .file-list {{ flex: 1; overflow-y: auto; padding: 4px 0; }}
  .file-item {{ display: flex; align-items: center; gap: 10px; padding: 8px 16px; cursor: pointer; font-size: 13px; color: #333; transition: background 0.15s; border-left: 3px solid transparent; }}
  .file-item:hover {{ background: #f5f5f5; }}
  .file-item.active {{ background: #f0f7ff; border-left-color: #0366d6; color: #0366d6; font-weight: 500; }}
  .file-icon {{ width: 18px; text-align: center; font-size: 14px; flex-shrink: 0; opacity: 0.6; }}
  .file-name {{ overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }}
  .file-size {{ margin-left: auto; font-size: 11px; color: #999; flex-shrink: 0; }}
  .browse-crumb {{ padding: 8px 16px; font-size: 12px; color: #666; background: #f8f8f8; border-bottom: 1px solid #f0f0f0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }}

  /* Main viewer */
  .main {{ flex: 1; display: flex; flex-direction: column; overflow: hidden; }}
  .viewer-toolbar {{ padding: 10px 20px; background: #fff; border-bottom: 1px solid #e5e5e5; display: flex; align-items: center; gap: 10px; flex-shrink: 0; }}
  .viewer-toolbar h2 {{ font-size: 14px; font-weight: 600; color: #333; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }}
  .toolbar-actions {{ display: flex; align-items: center; gap: 6px; }}
  .btn {{ display: inline-flex; align-items: center; gap: 4px; padding: 5px 12px; border: 1px solid #ddd; border-radius: 6px; background: #f8f8f8; color: #555; font-size: 12px; font-weight: 500; cursor: pointer; text-decoration: none; white-space: nowrap; }}
  .btn:hover {{ background: #eee; border-color: #ccc; }}
  .zoom-btn {{ width: 30px; padding: 5px 0; justify-content: center; }}
  .zoom-label {{ min-width: 38px; text-align: center; }}

  .viewer-content {{ flex: 1; overflow: auto; padding: 20px; background: #fff; }}

  /* No preview */
.no-preview {{ display: flex; flex-direction: column; align-items: center; justify-content: center; height: 100%; text-align: center; color: #999; }}
  .no-preview-icon {{ font-size: 48px; margin-bottom: 16px; opacity: 0.4; }}
  .no-preview-title {{ font-size: 16px; font-weight: 600; color: #555; margin-bottom: 8px; }}
  .no-preview-msg {{ font-size: 13px; max-width: 400px; line-height: 1.6; margin-bottom: 16px; }}

  /* Empty state */
  .empty-state {{ display: flex; flex-direction: column; align-items: center; justify-content: center; height: 100%; color: #999; font-size: 14px; }}
</style>
</head>
<body>
<div class="header">
  <span class="logo"><img src="/logo.png" alt="" class="logo-icon">Autopipe Viewer</span>
</div>
<div class="layout">
  <div class="sidebar">
    <div class="sidebar-header">Files</div>
    <div class="file-list" id="fileList"></div>
  </div>
  <div class="main">
    <div class="viewer-toolbar" id="toolbar" style="display:none;">
      <h2 id="toolbarTitle"></h2>
      <div class="toolbar-actions" id="toolbarActions"></div>
    </div>
    <div class="viewer-content" id="viewerContent">
      <div class="empty-state">Select a file from the sidebar</div>
    </div>
  </div>
</div>

<script>
var PLUGINS = {plugins_json};
var FILES = [];
var REFERENCE = null;
var currentFile = null;
var loadedPlugins = {{}};
var pluginInstances = {{}};

// File icon mapping
function getFileIcon(name) {{
  var ext = name.split('.').pop().toLowerCase();
  var icons = {{
    'png':'🖼','jpg':'🖼','jpeg':'🖼','gif':'🖼','svg':'🖼','webp':'🖼',
    'pdf':'📕',
    'txt':'📄','log':'📄','csv':'📊','tsv':'📊','json':'📋',
    'bam':'🧬','cram':'🧬','vcf':'🧬','bcf':'🧬',
    'bed':'🧬','gff':'🧬','gtf':'🧬',
    'fasta':'🧬','fa':'🧬','fastq':'🧬','fq':'🧬',
    'h5ad':'🔬'
  }};
  return icons[ext] || '📁';
}}

function formatSize(bytes) {{
  if (bytes < 1024) return bytes + ' B';
  if (bytes < 1048576) return (bytes / 1024).toFixed(1) + ' KB';
  return (bytes / 1048576).toFixed(1) + ' MB';
}}

// Index file extensions to hide from sidebar
var indexExts = ['bai','crai','fai','csi','tbi','idx'];

// Load file list and reference info
var BROWSE = null;

async function loadFiles() {{
  var resp = await fetch('/api/files');
  var allFiles = await resp.json();
  FILES = allFiles.filter(function(f) {{
    var ext = f.name.split('.').pop().toLowerCase();
    return indexExts.indexOf(ext) < 0;
  }});
  // Fetch reference info
  try {{
    var refResp = await fetch('/api/reference');
    var refData = await refResp.json();
    REFERENCE = refData.reference || null;
  }} catch(e) {{ REFERENCE = null; }}

  // If directory browsing is enabled, show the output-directory browser in
  // the sidebar; otherwise fall back to the flat snapshot file list.
  try {{
    var bResp = await fetch('/api/browse');
    var listing = await bResp.json();
    if (listing && listing.enabled) {{
      BROWSE = listing;
      renderBrowseSidebar(listing);
      if (FILES.length > 0 && !currentFile) selectFile(FILES[0].name);
      return;
    }}
  }} catch(e) {{ /* fall through to flat list */ }}

  renderSidebar();
  if (FILES.length > 0 && !currentFile) selectFile(FILES[0].name);
}}

function renderSidebar() {{
  var list = document.getElementById('fileList');
  list.innerHTML = '';
  FILES.forEach(function(f) {{
    var el = document.createElement('div');
    el.className = 'file-item';
    el.dataset.name = f.name;
    el.innerHTML = '<span class="file-icon">' + getFileIcon(f.name) + '</span>' +
                   '<span class="file-name" title="' + f.name + '">' + f.name + '</span>' +
                   '<span class="file-size">' + formatSize(f.size) + '</span>';
    el.onclick = function() {{ selectFile(f.name); }};
    list.appendChild(el);
  }});
}}

// ── Output-directory browser (lazy: only the clicked file is loaded) ──
async function loadBrowse(dir) {{
  try {{
    var url = '/api/browse' + (dir ? ('?dir=' + encodeURIComponent(dir)) : '');
    var resp = await fetch(url);
    var listing = await resp.json();
    if (listing.error) {{ alert(listing.error); return; }}
    BROWSE = listing;
    renderBrowseSidebar(listing);
  }} catch(e) {{ console.error(e); }}
}}

function renderBrowseSidebar(listing) {{
  var list = document.getElementById('fileList');
  list.innerHTML = '';

  // Breadcrumb showing the path relative to the browse root.
  var crumb = document.createElement('div');
  crumb.className = 'browse-crumb';
  var rel = (listing.cwd === listing.root) ? '/' : listing.cwd.slice(listing.root.length);
  crumb.textContent = '📂 ' + (rel || '/');
  crumb.title = listing.cwd;
  list.appendChild(crumb);

  // Parent navigation (hidden at the root so the user cannot escape it).
  if (listing.parent) {{
    var up = document.createElement('div');
    up.className = 'file-item';
    up.innerHTML = '<span class="file-icon">↑</span><span class="file-name">.. (parent folder)</span>';
    up.onclick = function() {{ loadBrowse(listing.parent); }};
    list.appendChild(up);
  }}

  var dirs = listing.entries.filter(function(e) {{ return e.is_dir; }});
  var files = listing.entries.filter(function(e) {{ return !e.is_dir; }});
  dirs.sort(function(a, b) {{ return a.name.localeCompare(b.name); }});
  files.sort(function(a, b) {{ return a.name.localeCompare(b.name); }});

  dirs.forEach(function(e) {{
    var el = document.createElement('div');
    el.className = 'file-item';
    el.innerHTML = '<span class="file-icon">📁</span>' +
                   '<span class="file-name" title="' + e.name + '">' + e.name + '</span>';
    el.onclick = function() {{ loadBrowse(e.path); }};
    list.appendChild(el);
  }});

  files.forEach(function(e) {{
    var ext = e.name.split('.').pop().toLowerCase();
    if (indexExts.indexOf(ext) >= 0) return;
    var el = document.createElement('div');
    el.className = 'file-item';
    el.dataset.name = e.name;
    el.innerHTML = '<span class="file-icon">' + getFileIcon(e.name) + '</span>' +
                   '<span class="file-name" title="' + e.name + '">' + e.name + '</span>' +
                   '<span class="file-size">' + formatSize(e.size) + '</span>';
    el.onclick = function() {{ browseOpenFile(e.path); }};
    list.appendChild(el);
  }});
}}

// Register the clicked file on the server (lazy), then render it through the
// standard plugin pipeline. Only this one file is fetched into the browser.
async function browseOpenFile(path) {{
  try {{
    var resp = await fetch('/api/browse-open?path=' + encodeURIComponent(path));
    var result = await resp.json();
    if (result.error) {{ alert(result.error); return; }}
    selectFile(result.filename);
  }} catch(e) {{ console.error(e); }}
}}

function selectFile(name) {{
  selectFileWithMode(name, 'data');
}}

function selectFileWithMode(name, mode) {{
  currentFile = name;
  currentScale = 1;
  currentViewMode = mode;

  // Update sidebar active state
  document.querySelectorAll('.file-item').forEach(function(el) {{
    el.classList.toggle('active', el.dataset.name === name);
  }});

  var ext = name.split('.').pop().toLowerCase();
  var toolbar = document.getElementById('toolbar');
  var title = document.getElementById('toolbarTitle');
  var actions = document.getElementById('toolbarActions');
  var content = document.getElementById('viewerContent');
  content.style.padding = '20px';
  content.style.overflow = 'auto';

  toolbar.style.display = 'flex';
  title.textContent = name;

  // Plugin-first routing: check installed plugins, fallback to no-preview
  var plugin = findPlugin(ext);
  if (plugin) {{
    renderPluginViewer(name, plugin, actions, content);
  }} else {{
    renderNoPreview(name, ext, actions, content);
  }}
}}

// ── Plugin Viewer ──
function findPlugin(ext) {{
  for (var i = 0; i < PLUGINS.length; i++) {{
    if (PLUGINS[i].extensions.indexOf(ext) >= 0) return PLUGINS[i];
  }}
  return null;
}}

async function renderPluginViewer(name, plugin, actions, content) {{
  actions.innerHTML = '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a>';
  content.innerHTML = '<div id="pluginContainer"></div>';

  try {{
    // Load plugin CSS if specified
    if (plugin.style && !loadedPlugins[plugin.name + '_css']) {{
      var link = document.createElement('link');
      link.rel = 'stylesheet';
      link.href = '/plugin/' + encodeURIComponent(plugin.name) + '/' + plugin.style;
      document.head.appendChild(link);
      loadedPlugins[plugin.name + '_css'] = true;
    }}

    // Load plugin JS and capture instance
    if (!loadedPlugins[plugin.name + '_js']) {{
      await new Promise(function(resolve, reject) {{
        window.AutoPipePlugin = null;
        var s = document.createElement('script');
        s.src = '/plugin/' + encodeURIComponent(plugin.name) + '/' + plugin.entry;
        s.onload = function() {{
          loadedPlugins[plugin.name + '_js'] = true;
          if (window.AutoPipePlugin) {{
            pluginInstances[plugin.name] = window.AutoPipePlugin;
          }}
          resolve();
        }};
        s.onerror = function() {{ reject(new Error('Failed to load plugin JS')); }};
        document.head.appendChild(s);
      }});
    }}

    var inst = pluginInstances[plugin.name];
    var container = document.getElementById('pluginContainer');
    if (container && inst && inst.render) {{
      container.innerHTML = '';
      inst.render(container, '/file/' + encodeURIComponent(name), name);
    }} else {{
      throw new Error('Plugin does not export AutoPipePlugin.render()');
    }}
  }} catch(e) {{
    content.innerHTML = '<div class="no-preview"><p class="no-preview-icon">⚠️</p>' +
      '<p class="no-preview-title">Plugin Error</p>' +
      '<p class="no-preview-msg">' + e.message + '</p>' +
      '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a></div>';
  }}
}}

// ── No Preview ──
function renderNoPreview(name, ext, actions, content) {{
  actions.innerHTML = '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a>';
  content.innerHTML =
    '<div class="no-preview">' +
      '<div class="no-preview-icon">📄</div>' +
      '<p class="no-preview-title">' + name + '</p>' +
      '<p class="no-preview-msg">No viewer plugin installed for .' + ext + ' files.<br>Install a plugin from the <b>Plugins</b> tab in the AutoPipe desktop app.</p>' +
      '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a>' +
    '</div>';
}}

// Auto-refresh file list when tab gets focus (e.g. after new show_results call)
window.addEventListener('focus', function() {{
  loadFiles();
}});

// Initialize
loadFiles();
</script>
</body>
</html>"##,
        plugins_json = plugins_json
    ))
}

async fn file_handler(
    State(state): State<ViewerState>,
    Path(filename): Path<String>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // 1. Check local file store first
    {
        let files = state.files.lock().await;
        if let Some(entry) = files.get(&filename) {
            let data = entry.data.clone();
            let mime = entry.mime.clone();
            let total_size = data.len();

            // Support Range requests for local files too
            if let Some(range_val) = headers.get(header::RANGE) {
                if let Ok(range_str) = range_val.to_str() {
                    if let Some((range_start, range_end)) = parse_range_header(range_str, total_size) {
                        let length = range_end - range_start + 1;
                        let slice = data[range_start..=range_end].to_vec();
                        return (
                            StatusCode::PARTIAL_CONTENT,
                            [
                                (header::CONTENT_TYPE, mime),
                                (header::CONTENT_LENGTH, length.to_string()),
                                (header::HeaderName::from_static("content-range"),
                                 format!("bytes {}-{}/{}", range_start, range_end, total_size).parse().unwrap()),
                                (header::HeaderName::from_static("accept-ranges"),
                                 "bytes".parse().unwrap()),
                            ],
                            slice,
                        ).into_response();
                    }
                }
            }

            return (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, mime),
                    (header::CONTENT_LENGTH, total_size.to_string()),
                    (header::HeaderName::from_static("accept-ranges"), "bytes".parse().unwrap()),
                ],
                data,
            ).into_response();
        }
    }

    // 2. Check remote files — SSH Range proxy
    let remote_files = get_remote_files_lock().await.lock().await;
    let entry = remote_files.get(&filename).map(|e| (e.remote_path.clone(), e.size, e.mime.clone()));
    drop(remote_files);

    if let Some((remote_path, total_size, mime)) = entry {
        let ssh_cfg_lock = get_ssh_config_lock().await.lock().await;
        let ssh_cfg = match &*ssh_cfg_lock {
            Some(c) => c.clone(),
            None => {
                return (StatusCode::INTERNAL_SERVER_ERROR, "SSH not configured").into_response();
            }
        };
        drop(ssh_cfg_lock);

        // Index files (.bai, .tbi, .fai, .csi, .crai, .idx) are small — transfer entirely
        let is_index = filename.ends_with(".bai") || filename.ends_with(".tbi")
            || filename.ends_with(".fai") || filename.ends_with(".csi")
            || filename.ends_with(".crai") || filename.ends_with(".idx");

        if let Some(range_val) = headers.get(header::RANGE) {
            if let Ok(range_str) = range_val.to_str() {
                if let Some((range_start, range_end)) = parse_range_header(range_str, total_size as usize) {
                    let length = range_end - range_start + 1;
                    // Use dd to extract byte range via SSH
                    let cmd = format!(
                        "dd if='{}' bs=1 skip={} count={} 2>/dev/null | base64 -w 0",
                        remote_path, range_start, length
                    );
                    match ssh_run(&ssh_cfg, &cmd).await {
                        Ok((b64, 0)) => {
                            let trimmed = b64.trim().to_string();
                            match base64::Engine::decode(
                                &base64::engine::general_purpose::STANDARD,
                                &trimmed,
                            ) {
                                Ok(data) => {
                                    return (
                                        StatusCode::PARTIAL_CONTENT,
                                        [
                                            (header::CONTENT_TYPE, mime),
                                            (header::CONTENT_LENGTH, data.len().to_string()),
                                            (header::HeaderName::from_static("content-range"),
                                             format!("bytes {}-{}/{}", range_start, range_end, total_size).parse().unwrap()),
                                            (header::HeaderName::from_static("accept-ranges"),
                                             "bytes".parse().unwrap()),
                                        ],
                                        data,
                                    ).into_response();
                                }
                                Err(e) => {
                                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("decode error: {}", e)).into_response();
                                }
                            }
                        }
                        Ok((err, _)) => {
                            return (StatusCode::INTERNAL_SERVER_ERROR, format!("SSH error: {}", err.trim())).into_response();
                        }
                        Err(e) => {
                            return (StatusCode::INTERNAL_SERVER_ERROR, format!("SSH error: {}", e)).into_response();
                        }
                    }
                }
            }
        }

        // No Range header — for small/index files, transfer entire; otherwise return headers only
        if is_index || total_size < 10_000_000 {
            // Transfer entire file (index files are small)
            let cmd = format!("base64 -w 0 '{}'", remote_path);
            match ssh_run(&ssh_cfg, &cmd).await {
                Ok((b64, 0)) => {
                    let trimmed = b64.trim().to_string();
                    match base64::Engine::decode(
                        &base64::engine::general_purpose::STANDARD,
                        &trimmed,
                    ) {
                        Ok(data) => {
                            return (
                                StatusCode::OK,
                                [
                                    (header::CONTENT_TYPE, mime),
                                    (header::CONTENT_LENGTH, data.len().to_string()),
                                    (header::HeaderName::from_static("accept-ranges"), "bytes".parse().unwrap()),
                                ],
                                data,
                            ).into_response();
                        }
                        Err(_) => {}
                    }
                }
                _ => {}
            }
        }

        // Large file without Range — return empty body with Content-Length + Accept-Ranges
        return (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, mime),
                (header::CONTENT_LENGTH, total_size.to_string()),
                (header::HeaderName::from_static("accept-ranges"), "bytes".parse().unwrap()),
            ],
            Vec::new(),
        ).into_response();
    }

    (StatusCode::NOT_FOUND, "File not found").into_response()
}

/// Parse HTTP Range header: "bytes=START-END" or "bytes=START-"
fn parse_range_header(range: &str, total: usize) -> Option<(usize, usize)> {
    let range = range.strip_prefix("bytes=")?;
    let parts: Vec<&str> = range.split('-').collect();
    if parts.len() != 2 {
        return None;
    }
    let start: usize = parts[0].parse().ok()?;
    let end: usize = if parts[1].is_empty() {
        total.saturating_sub(1)
    } else {
        parts[1].parse().ok()?
    };
    if start > end || start >= total {
        return None;
    }
    let end = end.min(total - 1);
    Some((start, end))
}
