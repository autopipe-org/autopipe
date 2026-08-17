#![allow(dead_code)]

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::config::AppConfig;
use crate::ssh;
use base64::Engine as _;
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

/// How a "pipeline" plugin claims a whole result directory (vs an extension
/// plugin claiming a single file). A directory matches when its marker's
/// pipeline name is in `names`, OR — for older results with no marker — every
/// file in `required_files` is present.
#[derive(Clone, Serialize, Deserialize, Default)]
struct PipelineMatch {
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    required_files: Vec<String>,
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
    /// "pipeline" for directory-level viewers; None (treated as "file") for
    /// ordinary extension viewers.
    #[serde(skip_serializing_if = "Option::is_none")]
    plugin_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pipeline_match: Option<PipelineMatch>,
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

/// Name of the pipeline viewer plugin to render for this directory, when
/// show_results routed a whole result folder to a pipeline viewer. None for
/// ordinary (extension-routed) views.
static PIPELINE_VIEWER: tokio::sync::OnceCell<Arc<Mutex<Option<String>>>> =
    tokio::sync::OnceCell::const_new();

async fn get_pipeline_viewer_lock() -> &'static Arc<Mutex<Option<String>>> {
    PIPELINE_VIEWER
        .get_or_init(|| async { Arc::new(Mutex::new(None)) })
        .await
}

/// Remote file entry — files that stay on the remote server (not transferred).
struct RemoteFileEntry {
    remote_path: String,
    size: u64,
    mime: String,
    /// Whether the file appears in the viewer's file list. Index files and a
    /// reference pulled in from outside the opened directory are served but
    /// hidden: they exist to support another file, not to be opened directly.
    listed: bool,
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

/// The output directory the viewer is allowed to browse, as passed by
/// show_results (a raw, not-yet-canonicalized path). All /api/browse and
/// /api/browse-open requests are sandboxed under its canonical form.
/// `None` disables in-viewer directory navigation (snapshot-only mode).
static BROWSE_ROOT: tokio::sync::OnceCell<Arc<Mutex<Option<String>>>> =
    tokio::sync::OnceCell::const_new();

async fn get_browse_root_lock() -> &'static Arc<Mutex<Option<String>>> {
    BROWSE_ROOT
        .get_or_init(|| async { Arc::new(Mutex::new(None)) })
        .await
}

/// Directory the browse sidebar opens at on first load — the parent folder of
/// a single opened file, so its siblings show immediately. Navigation is still
/// capped at BROWSE_ROOT. `None` means open at the browse root (used when a
/// whole directory was opened).
static INITIAL_BROWSE_DIR: tokio::sync::OnceCell<Arc<Mutex<Option<String>>>> =
    tokio::sync::OnceCell::const_new();

async fn get_initial_browse_dir_lock() -> &'static Arc<Mutex<Option<String>>> {
    INITIAL_BROWSE_DIR
        .get_or_init(|| async { Arc::new(Mutex::new(None)) })
        .await
}

/// Cached canonical form of BROWSE_ROOT. Computed lazily on the first browse
/// request (one SSH `realpath`) and reused, so show_results does not pay for
/// canonicalization on its critical path. Cleared whenever show_files sets a
/// new browse root.
static CANONICAL_ROOT: tokio::sync::OnceCell<Arc<Mutex<Option<String>>>> =
    tokio::sync::OnceCell::const_new();

async fn get_canonical_root_lock() -> &'static Arc<Mutex<Option<String>>> {
    CANONICAL_ROOT
        .get_or_init(|| async { Arc::new(Mutex::new(None)) })
        .await
}

/// Return the canonical browse root. On the first call after a new browse root
/// is set, this canonicalizes the raw root on the remote (one SSH call) and
/// caches it; subsequent calls reuse the cache. Returns `None` when browsing
/// is disabled or canonicalization fails.
async fn get_canonical_root(ssh_cfg: &AppConfig) -> Option<String> {
    {
        let cache = get_canonical_root_lock().await.lock().await;
        if cache.is_some() {
            return cache.clone();
        }
    }
    let raw = {
        let r = get_browse_root_lock().await.lock().await;
        r.clone()
    }?;
    let cmd = format!("realpath '{}' 2>/dev/null", raw.replace('\'', "'\\''"));
    let canon = match ssh_run_fast(ssh_cfg, &cmd).await {
        Ok((out, 0)) => {
            let c = clean_content(&out).trim().to_string();
            if c.is_empty() {
                return None;
            }
            c
        }
        _ => return None,
    };
    {
        let mut cache = get_canonical_root_lock().await.lock().await;
        *cache = Some(canon.clone());
    }
    Some(canon)
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
                    let plugin_type = v["plugin_type"].as_str().map(|s| s.to_string());
                    let pipeline_match: Option<PipelineMatch> = v.get("pipeline_match")
                        .and_then(|pm| serde_json::from_value(pm.clone()).ok());
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
                        plugin_type,
                        pipeline_match,
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
        .route("/input", get(input_page_handler))
        .route("/api/input/config", get(input_config_handler))
        .route("/api/input/browse", get(input_browse_handler))
        .route("/api/input/save", post(input_save_handler))
        // Wildcard ({*filename}, not {filename}) so nested paths like
        // meme_out/meme.xml reach the handler; a single-segment capture 404s on
        // any '/' in the name, which broke pipeline viewers serving subfolders.
        .route("/file/{*filename}", get(file_handler))
        .route("/data/{*filename}", get(data_handler))
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
/// `remote_files`: (filename, remote_path, size, mime, listed) — files kept on
/// the remote server, fetched on demand. `listed` false serves the file without
/// offering it in the viewer's file list (indexes, an out-of-tree reference).
/// `ssh_config`: SSH credentials for on-demand remote data fetching.
pub async fn show_files(
    files: Vec<(String, Vec<u8>, String)>,
    remote_files: Vec<(String, String, u64, String, bool)>,
    plugins_dir: String,
    reference: Option<String>,
    ssh_config: Option<AppConfig>,
    browse_root: Option<String>,
    initial_dir: Option<String>,
    pipeline_viewer: Option<String>,
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
        for (name, remote_path, size, mime, listed) in remote_files {
            rmap.insert(name, RemoteFileEntry { remote_path, size, mime, listed });
        }
    }

    // Store SSH config
    {
        let mut cfg = get_ssh_config_lock().await.lock().await;
        *cfg = ssh_config;
    }

    // Store the browse root (sandbox boundary for in-viewer navigation) and
    // clear the cached canonical form so it is recomputed for this new root.
    {
        let mut root = get_browse_root_lock().await.lock().await;
        *root = browse_root;
    }
    {
        let mut canon = get_canonical_root_lock().await.lock().await;
        *canon = None;
    }
    {
        let mut idir = get_initial_browse_dir_lock().await.lock().await;
        *idir = initial_dir;
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

    // Store the active pipeline viewer (if this is a pipeline-routed view)
    {
        let mut pv = get_pipeline_viewer_lock().await.lock().await;
        *pv = pipeline_viewer;
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

    // Supporting files (indexes, an out-of-tree reference) stay reachable via
    // /file/ but are not offered as things to open.
    let remote = get_remote_files_lock().await.lock().await;
    for (name, entry) in remote.iter() {
        if !entry.listed {
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
    // Browsing is enabled when a browse root was set by show_files.
    let enabled = {
        let r = get_browse_root_lock().await.lock().await;
        r.is_some()
    };
    if !enabled {
        return Json(BrowseListing {
            enabled: false,
            root: String::new(),
            cwd: String::new(),
            parent: None,
            entries: Vec::new(),
            error: None,
        });
    }

    let ssh_cfg = {
        let lock = get_ssh_config_lock().await.lock().await;
        lock.clone()
    };
    let ssh_cfg = match ssh_cfg {
        Some(c) => c,
        None => {
            return Json(BrowseListing {
                enabled: true,
                root: String::new(),
                cwd: String::new(),
                parent: None,
                entries: Vec::new(),
                error: Some("SSH not configured".into()),
            });
        }
    };

    // Canonicalize the browse root lazily (cached). This is the one SSH call
    // that show_results deferred off its critical path.
    let root = match get_canonical_root(&ssh_cfg).await {
        Some(r) => r,
        None => {
            return Json(BrowseListing {
                enabled: true,
                root: String::new(),
                cwd: String::new(),
                parent: None,
                entries: Vec::new(),
                error: Some("Could not resolve the output directory".into()),
            });
        }
    };

    // When no dir is given, open at the configured initial directory (the
    // opened file's folder), if any, otherwise at the browse root. Navigation
    // stays capped at the root either way (verified below).
    let requested = match query.dir {
        Some(d) => d,
        None => {
            let idir = get_initial_browse_dir_lock().await.lock().await.clone();
            idir.unwrap_or_else(|| root.clone())
        }
    };

    // Resolve the path AND list its entries in a SINGLE SSH command (was two:
    // realpath + listing) on a non-login shell. `realpath` follows symlinks;
    // the sandbox prefix check still runs in Rust on the returned canonical
    // path, so escapes are rejected exactly as before. Output: first line
    // "ROOT\t<canonical cwd>", then one "<type>\t<size>\t<name>" line per
    // entry. `printf` (not `echo`) emits real tabs; the glob is `*` only so
    // dotfiles such as `.snakemake_timestamp` stay hidden.
    let esc = requested.replace('\'', "'\\''");
    let combined = format!(
        "p=$(realpath '{esc}') && [ -n \"$p\" ] && printf 'ROOT\\t%s\\n' \"$p\" && cd \"$p\" && for f in *; do [ -e \"$f\" ] || continue; if [ -d \"$f\" ]; then printf 'd\\t0\\t%s\\n' \"$f\"; else printf 'f\\t%s\\t%s\\n' \"$(stat -c%s \"$f\" 2>/dev/null || stat -f%z \"$f\" 2>/dev/null)\" \"$f\"; fi; done",
        esc = esc
    );
    let out = match ssh_run_fast(&ssh_cfg, &combined).await {
        Ok((o, _)) => clean_content(&o),
        Err(_) => String::new(),
    };

    // First line carries the canonical cwd; remaining lines are entries.
    let mut cwd = String::new();
    let mut raw_entries: Vec<(bool, u64, String)> = Vec::new();
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("ROOT\t") {
            cwd = rest.to_string();
            continue;
        }
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() != 3 {
            continue;
        }
        let name = parts[2].to_string();
        if name.is_empty() {
            continue;
        }
        raw_entries.push((parts[0] == "d", parts[1].parse().unwrap_or(0), name));
    }

    // Sandbox: the canonical cwd must stay within the browse root. An empty
    // cwd means realpath failed (missing path / error).
    let root_trimmed = root.trim_end_matches('/').to_string();
    if cwd.is_empty()
        || !(cwd == root_trimmed || cwd.starts_with(&format!("{}/", root_trimmed)))
    {
        return Json(BrowseListing {
            enabled: true,
            root: root_trimmed,
            cwd: root.clone(),
            parent: None,
            entries: Vec::new(),
            error: Some("Path is outside the allowed output directory".into()),
        });
    }

    let entries: Vec<BrowseEntry> = raw_entries
        .into_iter()
        .map(|(is_dir, size, name)| {
            let path = format!("{}/{}", cwd.trim_end_matches('/'), name);
            BrowseEntry { name, path, is_dir, size }
        })
        .collect();

    // Compute parent only if cwd is strictly inside the root.
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
    let enabled = {
        let r = get_browse_root_lock().await.lock().await;
        r.is_some()
    };
    if !enabled {
        return Json(serde_json::json!({ "error": "Browsing is not enabled" }));
    }
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

    let root = match get_canonical_root(&ssh_cfg).await {
        Some(r) => r,
        None => return Json(serde_json::json!({ "error": "Could not resolve the output directory" })),
    };

    // Resolve the path, verify it's a regular file, and read its size in a
    // SINGLE SSH round trip (previously three: realpath + test -f + stat).
    // `realpath` follows symlinks; the sandbox prefix check still runs in Rust
    // on the returned canonical path, so symlink/`..` escapes are rejected
    // exactly as realpath_within_root would. On success the command prints
    // "<canonical path>\t<size>"; if the path is missing or not a regular file
    // the `&&` chain short-circuits and prints nothing.
    let esc = requested.replace('\'', "'\\''");
    let combined = format!(
        "p=$(realpath '{esc}' 2>/dev/null) && [ -f \"$p\" ] && \
         printf '%s\\t%s\\n' \"$p\" \"$(stat -c%s \"$p\" 2>/dev/null || stat -f%z \"$p\" 2>/dev/null)\"",
        esc = esc
    );
    let (resolved, size): (String, u64) = match ssh_run_fast(&ssh_cfg, &combined).await {
        Ok((out, _)) => {
            let line = clean_content(&out);
            match line.trim().split_once('\t') {
                Some((p, s)) if !p.is_empty() => {
                    (p.to_string(), s.trim().parse().unwrap_or(0))
                }
                // No tab / empty path → missing, a directory, or realpath failed.
                _ => return Json(serde_json::json!({ "error": "Not a file" })),
            }
        }
        Err(_) => return Json(serde_json::json!({ "error": "Not a file" })),
    };

    // Sandbox: the canonical path must stay within the browse root.
    let root_trimmed = root.trim_end_matches('/');
    if !(resolved == root_trimmed || resolved.starts_with(&format!("{}/", root_trimmed))) {
        return Json(serde_json::json!({
            "error": "Path is outside the allowed output directory"
        }));
    }

    let filename = std::path::Path::new(&resolved)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    let ext = filename.rsplit('.').next().map(|e| e.to_lowercase()).unwrap_or_default();
    let mime = mime_for_ext(&ext);

    // Register into the remote file store so /file/{filename} + /data/{filename}
    // serve it lazily through the existing pipeline.
    {
        let mut rmap = get_remote_files_lock().await.lock().await;
        rmap.insert(
            filename.clone(),
            RemoteFileEntry {
                remote_path: resolved.clone(),
                size,
                mime: mime.to_string(),
                listed: true,
            },
        );
    }

    // An alignment file is useless to IGV without its index, and the sidebar
    // only ever registers the file that was clicked. Pull the sidecar in too,
    // hidden from the file list the same way indexes always have been.
    for (name, path, isize) in find_sidecar_indexes(&ssh_cfg, &resolved, &ext).await {
        let mut rmap = get_remote_files_lock().await.lock().await;
        rmap.insert(
            name,
            RemoteFileEntry {
                remote_path: path,
                size: isize,
                mime: "application/octet-stream".to_string(),
                listed: false,
            },
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

/// Index files that sit next to a data file, by the data file's extension.
/// Both naming conventions are tried: `reads.bam.bai` and `reads.bai`.
pub fn sidecar_index_exts(ext: &str) -> &'static [&'static str] {
    match ext {
        "bam" => &["bai", "csi"],
        "cram" => &["crai"],
        // bgzipped text — matched whether the caller passes the last segment
        // ("gz") or the compound viewer extension ("vcf.gz").
        "gz" | "bgz" | "vcf.gz" | "bed.gz" | "gff.gz" | "gff3.gz" | "gtf.gz" => &["tbi", "csi"],
        "fa" | "fasta" | "fna" => &["fai"],
        _ => &[],
    }
}

/// Locate the index files belonging to `path`, in a single SSH round trip.
/// Returns (filename, remote_path, size) for each one that exists.
pub async fn find_sidecar_indexes(
    ssh_cfg: &AppConfig,
    path: &str,
    ext: &str,
) -> Vec<(String, String, u64)> {
    let exts = sidecar_index_exts(ext);
    if exts.is_empty() {
        return Vec::new();
    }

    let stem = path.rsplit_once('.').map(|(s, _)| s).unwrap_or(path);
    let mut candidates: Vec<String> = Vec::new();
    for e in exts {
        candidates.push(format!("{}.{}", path, e)); // reads.bam.bai
        candidates.push(format!("{}.{}", stem, e)); // reads.bai
    }

    // One command prints "<path>\t<size>" for each candidate that exists.
    let probes: Vec<String> = candidates
        .iter()
        .map(|c| {
            let esc = c.replace('\'', "'\\''");
            format!(
                "[ -f '{esc}' ] && printf '%s\\t%s\\n' '{esc}' \
                 \"$(stat -c%s '{esc}' 2>/dev/null || stat -f%z '{esc}' 2>/dev/null)\"",
                esc = esc
            )
        })
        .collect();

    let out = match ssh_run_fast(ssh_cfg, &probes.join("; ")).await {
        Ok((o, _)) => clean_content(&o),
        Err(_) => return Vec::new(),
    };

    let mut found = Vec::new();
    for line in out.lines() {
        if let Some((p, s)) = line.trim().split_once('\t') {
            if p.is_empty() {
                continue;
            }
            let name = std::path::Path::new(p)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            if !name.is_empty() {
                found.push((name, p.to_string(), s.trim().parse().unwrap_or(0)));
            }
        }
    }
    found
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
    ssh_run_inner(config, cmd, true).await
}

/// Like `ssh_run` but runs the command in a NON-login shell (`bash -c` instead
/// of `bash -l -c`). Use only for browse/metadata commands that need nothing
/// beyond coreutils (realpath, ls, stat, test) — this skips ~/.bash_profile /
/// conda / module initialization, which can add seconds per call on HPC
/// servers. Do NOT use for plugin data_source commands that rely on
/// conda/module-provided tools (samtools, bcftools, …).
async fn ssh_run_fast(config: &AppConfig, cmd: &str) -> Result<(String, i32), String> {
    ssh_run_inner(config, cmd, false).await
}

async fn ssh_run_inner(
    config: &AppConfig,
    cmd: &str,
    login_shell: bool,
) -> Result<(String, i32), String> {
    let config = config.clone();
    let escaped = cmd.replace('\'', "'\\''");
    // A login shell loads ~/.bash_profile (conda/module PATH); a plain shell
    // skips it for speed when only coreutils are needed.
    let cmd = if login_shell {
        format!("bash -l -c '{}'", escaped)
    } else {
        format!("bash -c '{}'", escaped)
    };
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
    let pipeline_viewer_json = {
        let pv = get_pipeline_viewer_lock().await.lock().await;
        match &*pv {
            Some(name) => serde_json::to_string(name).unwrap_or_else(|_| "null".into()),
            None => "null".into(),
        }
    };

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
  .file-item.dimmed {{ opacity: 0.45; }}
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
var PIPELINE_VIEWER = {pipeline_viewer_json};
var FILES = [];
var REFERENCE = null;
var currentFile = null;
var loadedPlugins = {{}};
var pluginInstances = {{}};

// File icon mapping
function getFileIcon(name) {{
  // Use the compound-aware extension so ".gff3.gz" maps to the gff icon
  // rather than falling through as "gz".
  var ext = (typeof viewerExt === 'function') ? viewerExt(name) : name.split('.').pop().toLowerCase();
  var icons = {{
    'png':'🖼','jpg':'🖼','jpeg':'🖼','gif':'🖼','svg':'🖼','webp':'🖼',
    'pdf':'📕',
    'txt':'📄','log':'📄','csv':'📊','tsv':'📊','json':'📋',
    'bam':'🧬','cram':'🧬','vcf':'🧬','bcf':'🧬',
    'bed':'🧬','gff':'🧬','gtf':'🧬','gff3':'🧬',
    'vcf.gz':'🧬','bed.gz':'🧬','gff.gz':'🧬','gff3.gz':'🧬','gtf.gz':'🧬',
    'fasta.gz':'🧬','fa.gz':'🧬','fastq.gz':'🧬','fq.gz':'🧬',
    'csv.gz':'📊','tsv.gz':'📊','txt.gz':'📄','log.gz':'📄',
    'fasta':'🧬','fa':'🧬','fastq':'🧬','fq':'🧬',
    'h5ad':'🔬'
  }};
  // Fall back to a generic file icon, never the folder icon — the sidebar
  // uses 📁 for directories and the two must not collide.
  return icons[ext] || '📄';
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
  // Stay in whatever folder the user navigated to: without the explicit dir
  // the server answers with the initial directory, so a plain refresh (this
  // runs on every window focus) would throw them back to the top.
  try {{
    var dir = (BROWSE && BROWSE.cwd) ? ('?dir=' + encodeURIComponent(BROWSE.cwd)) : '';
    var bResp = await fetch('/api/browse' + dir);
    var listing = await bResp.json();
    if (listing && listing.enabled) {{
      BROWSE = listing;
      renderBrowseSidebar(listing);
      if (PIPELINE_VIEWER && !currentFile) {{ renderPipelineDashboard(); return; }}
      if (FILES.length > 0 && !currentFile) selectFile(FILES[0].name);
      return;
    }}
  }} catch(e) {{ /* fall through to flat list */ }}

  renderSidebar();
  if (PIPELINE_VIEWER && !currentFile) {{ renderPipelineDashboard(); return; }}
  if (FILES.length > 0 && !currentFile) selectFile(FILES[0].name);
}}

// Render a whole result directory through a pipeline viewer (one dashboard for
// the folder, not per-file). The sidebar stays usable so the user can still
// open individual files; picking one switches to the ordinary file view.
async function renderPipelineDashboard() {{
  var plugin = null;
  for (var i = 0; i < PLUGINS.length; i++) {{ if (PLUGINS[i].name === PIPELINE_VIEWER) {{ plugin = PLUGINS[i]; break; }} }}
  if (!plugin) {{ if (FILES.length > 0 && !currentFile) selectFile(FILES[0].name); return; }}
  currentFile = null;
  document.querySelectorAll('.file-item').forEach(function(el) {{ el.classList.remove('active'); }});
  // A pipeline viewer owns the whole pane — hide the file-tree sidebar so only
  // the dashboard shows.
  var sidebar = document.querySelector('.sidebar');
  if (sidebar) sidebar.style.display = 'none';
  var actions = document.getElementById('toolbarActions');
  var content = document.getElementById('viewerContent');
  // Keep the toolbar hidden: its title and the generic Download (which pointed
  // at the wrong file) are redundant — the dashboard has its own header and a
  // "Download all (.zip)" button.
  var toolbar = document.getElementById('toolbar');
  if (toolbar) toolbar.style.display = 'none';
  content.style.padding = '0';
  content.style.overflow = 'hidden';
  await renderPluginViewer(plugin.name, plugin, actions, content);
  // Let the dashboard fill the pane (it manages its own internal scrolling).
  var pc = document.getElementById('pluginContainer');
  if (pc) {{ pc.style.height = '100%'; }}
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
    el.dataset.name = e.name;
    // CRAM needs a reference genome to be decoded. When none has been
    // provided, dim the entry (it looks disabled) but keep it clickable so a
    // click can explain that a reference is required, instead of failing.
    var needsRef = (ext === 'cram' && !REFERENCE);
    el.className = needsRef ? 'file-item dimmed' : 'file-item';
    el.innerHTML = '<span class="file-icon">' + getFileIcon(e.name) + '</span>' +
                   '<span class="file-name" title="' + e.name + '">' + e.name + '</span>' +
                   '<span class="file-size">' + formatSize(e.size) + '</span>';
    if (needsRef) {{
      el.onclick = function() {{ showReferenceNeeded(e.name); }};
    }} else {{
      el.onclick = function() {{ browseOpenFile(e.path); }};
    }}
    list.appendChild(el);
  }});

  // Rebuilding the list drops the highlight, so restore it for the file that
  // is actually on screen — otherwise a background refresh makes the viewer
  // look like nothing is selected.
  if (currentFile) {{
    list.querySelectorAll('.file-item').forEach(function(el) {{
      el.classList.toggle('active', el.dataset.name === currentFile);
    }});
  }}
}}

// Shown when a CRAM file is clicked but no reference genome is available.
// CRAM cannot be decoded without its reference, so we explain that in the
// viewer pane rather than attempting (and failing) to render the file.
function showReferenceNeeded(name) {{
  currentFile = name;
  document.querySelectorAll('.file-item').forEach(function(el) {{
    el.classList.toggle('active', el.dataset.name === name);
  }});
  var toolbar = document.getElementById('toolbar');
  var title = document.getElementById('toolbarTitle');
  var actions = document.getElementById('toolbarActions');
  var content = document.getElementById('viewerContent');
  content.style.padding = '20px';
  content.style.overflow = 'auto';
  toolbar.style.display = 'flex';
  title.textContent = name;
  actions.innerHTML = '';
  content.innerHTML =
    '<div class="no-preview">' +
      '<div class="no-preview-icon">🧬</div>' +
      '<p class="no-preview-title">Reference genome required</p>' +
      '<p class="no-preview-msg">This CRAM file cannot be displayed without a reference genome (FASTA).<br>Provide a reference genome, then reopen the results to view this file.</p>' +
    '</div>';
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

  var ext = viewerExt(name);
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

// Effective viewer extension: recognise bgzipped text (.vcf.gz etc.) so it
// routes to the same plugin as the uncompressed form instead of matching a
// nonexistent "gz" plugin. Mirrors viewer_ext() on the Rust side.
function viewerExt(name) {{
  var lower = name.toLowerCase();
  var compound = ['vcf.gz','bed.gz','gff.gz','gff3.gz','gtf.gz','fasta.gz','fa.gz','fastq.gz','fq.gz','csv.gz','tsv.gz','txt.gz','log.gz'];
  for (var i = 0; i < compound.length; i++) {{
    if (lower.slice(-(compound[i].length + 1)) === '.' + compound[i]) return compound[i];
  }}
  return lower.split('.').pop();
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

// Auto-refresh file list when tab gets focus (e.g. after new show_results call).
// A pipeline dashboard owns its own state and refetches on demand, so reloading
// on every focus (window switch, fullscreen toggle) would needlessly rebuild it.
window.addEventListener('focus', function() {{
  if (!PIPELINE_VIEWER) loadFiles();
}});

// Initialize
loadFiles();
</script>
</body>
</html>"##,
        plugins_json = plugins_json,
        pipeline_viewer_json = pipeline_viewer_json
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

// ── Input configuration page (file explorer + config editor) ─────────────
// Opened via the configure_input MCP tool. Lets the user pick input files from
// the analysis machine's own store (S3 bucket for AWS, the SSH server's FS
// otherwise) and edit the pipeline's config.yaml values, then Save — which
// symlinks the picked files into the input dir and writes config.yaml back.

#[derive(Clone)]
struct InputSession {
    pipeline_dir: String,
    config: AppConfig,
    /// Optional AI-supplied fallback descriptions (key -> text), used only when
    /// config.yaml has no comment for a variable.
    ai_descriptions: std::collections::HashMap<String, String>,
}

static INPUT_SESSION: tokio::sync::OnceCell<Arc<Mutex<Option<InputSession>>>> =
    tokio::sync::OnceCell::const_new();

async fn input_session_lock() -> &'static Arc<Mutex<Option<InputSession>>> {
    INPUT_SESSION
        .get_or_init(|| async { Arc::new(Mutex::new(None)) })
        .await
}

/// Set up an input-config session and open the page in the browser. Returns the
/// input directory (where picked files are symlinked) so the caller can pass it
/// to execute_pipeline once the user saves.
pub async fn show_input_config(
    pipeline_dir: String,
    config: AppConfig,
    ai_descriptions: std::collections::HashMap<String, String>,
) -> Result<String, String> {
    let plugins_dir = config.full_plugins_dir();
    let input_dir = config.full_input_dir();
    {
        let mut s = input_session_lock().await.lock().await;
        *s = Some(InputSession {
            pipeline_dir,
            config,
            ai_descriptions,
        });
    }
    let port = ensure_server(&plugins_dir).await?;
    let url = format!("http://127.0.0.1:{}/input", port);
    open::that(&url).map_err(|e| format!("Failed to open browser: {}", e))?;
    Ok(input_dir)
}

/// Whether a config field is a pickable INPUT file. Precise on purpose: a value
/// that merely ends in a data extension (e.g. an internal output filename such
/// as top_seq_source: "stage4_sort2_ranked.tsv") must NOT be treated as a file,
/// or Save would create a broken symlink. Only raw-input keys, or defaults that
/// already point into the /input mount, qualify.
fn input_is_file_field(key: &str, value: &str) -> bool {
    let k = key.to_lowercase();
    const FILE_KEYS: &[&str] = &[
        "r1", "r2", "reads", "input", "fastq", "fq", "reference", "genome",
        "fasta", "fa", "bam",
    ];
    if FILE_KEYS
        .iter()
        .any(|fk| k == *fk || k.ends_with(&format!("_{}", fk)) || k.starts_with(&format!("{}_", fk)))
    {
        return true;
    }
    value.trim().starts_with("/input/")
}

/// Detect a YAML scalar's type from its RAW (unstripped) form so it can be
/// written back with the same type. Quoted values stay strings.
fn input_detect_type(raw: &str) -> &'static str {
    let r = raw.trim();
    if r.starts_with('"') || r.starts_with('\'') || r.is_empty() {
        return "string";
    }
    if r == "true" || r == "false" {
        return "bool";
    }
    if r.parse::<i64>().is_ok() {
        return "int";
    }
    if r.parse::<f64>().is_ok() {
        return "float";
    }
    "string"
}

/// Split "value  # comment" respecting a leading quoted string.
fn input_split_inline_comment(s: &str) -> (String, String) {
    let s = s.trim();
    if s.starts_with('"') {
        if let Some(end) = s[1..].find('"') {
            let val = &s[..end + 2];
            let rest = s[end + 2..].trim_start();
            let comment = rest.strip_prefix('#').map(|c| c.trim().to_string()).unwrap_or_default();
            return (val.to_string(), comment);
        }
    }
    if let Some(hpos) = s.find('#') {
        return (s[..hpos].trim().to_string(), s[hpos + 1..].trim().to_string());
    }
    (s.to_string(), String::new())
}

/// Parse top-level scalar config fields with type, required flag (comment
/// contains "required"), and description (preceding comment lines + inline).
fn input_parse_config_fields(
    yaml: &str,
    ai_desc: &std::collections::HashMap<String, String>,
) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    let mut pending: Vec<String> = Vec::new();
    // A comment block placed above a GROUP of keys (e.g. "# Required: Paired-end
    // FASTQ files" above both r1 and r2) describes every key in that group, so it
    // persists across consecutive key lines and is cleared only by a blank line
    // or by the start of a NEW comment block after some keys.
    let mut last_was_key = false;
    for line in yaml.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            pending.clear();
            last_was_key = false;
            continue;
        }
        if line.starts_with(char::is_whitespace) {
            continue; // nested / indented — not a top-level scalar
        }
        if trimmed.starts_with('#') {
            if last_was_key {
                pending.clear();
                last_was_key = false;
            }
            let c = trimmed.trim_start_matches('#').trim();
            if !c.is_empty() && !c.chars().all(|ch| ch == '=' || ch == '-') {
                pending.push(c.to_string());
            }
            continue;
        }
        let idx = match line.find(':') {
            Some(i) => i,
            None => {
                pending.clear();
                last_was_key = false;
                continue;
            }
        };
        let key = line[..idx].trim();
        if key.is_empty() || key.contains(' ') {
            pending.clear();
            last_was_key = false;
            continue;
        }
        let after = line[idx + 1..].trim();
        let (raw_val, inline_comment) = input_split_inline_comment(after);
        let ty = input_detect_type(&raw_val);
        let display = raw_val.trim().trim_matches('"').trim_matches('\'').to_string();
        let is_file = input_is_file_field(key, &display);

        // The raw config comment (preceding block + inline) is used to detect the
        // required flag and as a LAST-RESORT description. The description SHOWN is
        // the AI's clean one-line summary when available (primary), so we never
        // dump possibly-messy raw comment text as the help line.
        let mut config_comment = pending.join(" ");
        if !inline_comment.is_empty() {
            if config_comment.is_empty() {
                config_comment = inline_comment.clone();
            } else {
                config_comment = format!("{} {}", config_comment, inline_comment);
            }
        }
        let required = config_comment.to_lowercase().contains("required");
        let desc = ai_desc.get(key).cloned().unwrap_or(config_comment);

        out.push(serde_json::json!({
            "key": key,
            "value": display,
            "is_file": is_file,
            "type": ty,
            "required": required,
            "description": desc,
        }));
        last_was_key = true;
    }
    out
}

/// Double-quote a YAML string value, escaping as needed.
fn input_dquote(v: &str) -> String {
    format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Format a value for YAML in the given type, so ints stay ints, bools stay
/// bools, and strings stay quoted strings.
fn input_format_value(value: &str, ty: &str) -> String {
    let v = value.trim();
    match ty {
        "bool" => {
            if v == "true" || v == "false" {
                v.to_string()
            } else {
                input_dquote(v)
            }
        }
        "int" => {
            if v.parse::<i64>().is_ok() {
                v.to_string()
            } else {
                input_dquote(v)
            }
        }
        "float" => {
            if v.parse::<f64>().is_ok() {
                v.to_string()
            } else {
                input_dquote(v)
            }
        }
        _ => input_dquote(v),
    }
}

/// Replace the value of a top-level key in-place, preserving comments/structure,
/// INCLUDING any inline `# comment` on that key's line (so descriptions survive
/// repeated saves). `formatted_value` is written verbatim (already formatted).
fn input_set_yaml_value(yaml: &str, key: &str, formatted_value: &str) -> String {
    let mut found = false;
    let lines: Vec<String> = yaml
        .lines()
        .map(|line| {
            if found || line.starts_with(char::is_whitespace) {
                return line.to_string();
            }
            if let Some(rest) = line.strip_prefix(key) {
                if rest.trim_start().starts_with(':') {
                    found = true;
                    // Keep the original inline comment, if any.
                    let after = match line.find(':') {
                        Some(c) => &line[c + 1..],
                        None => "",
                    };
                    let (_v, comment) = input_split_inline_comment(after.trim());
                    let tail = if comment.is_empty() {
                        String::new()
                    } else {
                        format!("  # {}", comment)
                    };
                    return format!("{}: {}{}", key, formatted_value, tail);
                }
            }
            line.to_string()
        })
        .collect();
    let mut result = lines.join("\n");
    if yaml.ends_with('\n') {
        result.push('\n');
    }
    result
}

async fn input_page_handler() -> Html<String> {
    Html(INPUT_PAGE_HTML.to_string())
}

async fn input_config_handler() -> Json<serde_json::Value> {
    let session = { input_session_lock().await.lock().await.clone() };
    let session = match session {
        Some(s) => s,
        None => return Json(serde_json::json!({ "ok": false, "error": "No input session is active." })),
    };
    let cfg_path = format!("{}/config.yaml", session.pipeline_dir.trim_end_matches('/'));
    let esc = cfg_path.replace('\'', "'\\''");
    let raw = match ssh_run_fast(&session.config, &format!("cat '{}'", esc)).await {
        Ok((out, 0)) => clean_content(&out),
        Ok((out, _)) => {
            return Json(serde_json::json!({ "ok": false, "error": format!("Cannot read config.yaml: {}", out.trim()) }))
        }
        Err(e) => return Json(serde_json::json!({ "ok": false, "error": e })),
    };
    let fields = input_parse_config_fields(&raw, &session.ai_descriptions);
    let is_cloud = session.config.connection_type == "cloud" && session.config.cloud_provider == "aws";
    Json(serde_json::json!({
        "ok": true,
        "fields": fields,
        "source": if is_cloud { "s3" } else { "ssh" },
        "bucket": session.config.aws_bucket,
        "pipeline_dir": session.pipeline_dir,
    }))
}

#[derive(Deserialize)]
struct InputBrowseQuery {
    path: Option<String>,
}

async fn input_browse_handler(Query(q): Query<InputBrowseQuery>) -> Json<serde_json::Value> {
    let session = { input_session_lock().await.lock().await.clone() };
    let session = match session {
        Some(s) => s,
        None => return Json(serde_json::json!({ "ok": false, "error": "No input session is active." })),
    };
    let cfg = &session.config;
    let is_cloud = cfg.connection_type == "cloud" && cfg.cloud_provider == "aws";

    if is_cloud {
        if cfg.aws_bucket.trim().is_empty() {
            return Json(serde_json::json!({ "ok": false, "error": "No S3 bucket is selected in AutoPipe." }));
        }
        let prefix = q.path.unwrap_or_default();
        match crate::aws::list_objects(
            &cfg.aws_access_key,
            &cfg.aws_secret_key,
            &cfg.aws_region,
            &cfg.aws_bucket,
            &prefix,
        )
        .await
        {
            Ok((folders, files)) => {
                let folder_json: Vec<_> = folders
                    .iter()
                    .map(|p| {
                        let name = p.trim_end_matches('/').rsplit('/').next().unwrap_or(p).to_string();
                        serde_json::json!({ "name": name, "path": p })
                    })
                    .collect();
                let file_json: Vec<_> = files
                    .iter()
                    .map(|(k, sz)| {
                        let name = k.rsplit('/').next().unwrap_or(k).to_string();
                        serde_json::json!({ "name": name, "path": k, "size": sz })
                    })
                    .collect();
                let parent = if prefix.is_empty() {
                    serde_json::Value::Null
                } else {
                    let p = prefix.trim_end_matches('/');
                    match p.rfind('/') {
                        Some(i) => serde_json::Value::String(p[..i + 1].to_string()),
                        None => serde_json::Value::String(String::new()),
                    }
                };
                Json(serde_json::json!({
                    "ok": true, "source": "s3", "cwd": prefix, "parent": parent,
                    "folders": folder_json, "files": file_json,
                }))
            }
            Err(e) => Json(serde_json::json!({ "ok": false, "error": e })),
        }
    } else {
        let path = q.path.unwrap_or_default();
        let cd = if path.is_empty() || path == "~" {
            "cd \"$HOME\"".to_string()
        } else {
            format!("cd '{}'", path.replace('\'', "'\\''"))
        };
        let cmd = format!(
            "{} 2>/dev/null && pwd && for f in * ; do [ -e \"$f\" ] || continue; \
             if [ -d \"$f\" ]; then printf 'd\\t0\\t%s\\n' \"$f\"; \
             else printf 'f\\t%s\\t%s\\n' \"$(stat -c%s \"$f\" 2>/dev/null || echo 0)\" \"$f\"; fi; done",
            cd
        );
        match ssh_run_fast(cfg, &cmd).await {
            Ok((out, 0)) => {
                let out = clean_content(&out);
                let mut lines = out.lines();
                let cwd = lines.next().unwrap_or("").trim().to_string();
                let mut folders = Vec::new();
                let mut files = Vec::new();
                for line in lines {
                    let parts: Vec<&str> = line.splitn(3, '\t').collect();
                    if parts.len() != 3 {
                        continue;
                    }
                    let name = parts[2].to_string();
                    let full = format!("{}/{}", cwd.trim_end_matches('/'), name);
                    if parts[0] == "d" {
                        folders.push(serde_json::json!({ "name": name, "path": full }));
                    } else {
                        let sz: i64 = parts[1].parse().unwrap_or(0);
                        files.push(serde_json::json!({ "name": name, "path": full, "size": sz }));
                    }
                }
                let c = cwd.trim_end_matches('/');
                let parent = match c.rfind('/') {
                    Some(0) => serde_json::Value::String("/".to_string()),
                    Some(i) => serde_json::Value::String(c[..i].to_string()),
                    None => serde_json::Value::Null,
                };
                Json(serde_json::json!({
                    "ok": true, "source": "ssh", "cwd": cwd, "parent": parent,
                    "folders": folders, "files": files,
                }))
            }
            Ok((out, _)) => Json(serde_json::json!({ "ok": false, "error": format!("Cannot list directory: {}", out.trim()) })),
            Err(e) => Json(serde_json::json!({ "ok": false, "error": e })),
        }
    }
}

#[derive(Deserialize)]
struct InputSaveField {
    key: String,
    value: String,
    is_file: bool,
    #[serde(rename = "type", default)]
    ty: String,
}

#[derive(Deserialize)]
struct InputSaveBody {
    fields: Vec<InputSaveField>,
}

/// A file field needs symlinking only when the user actually picked an external
/// source: a non-empty value that is NOT already an /input/ path (an unchanged
/// default or a previously prepared path stays as-is, avoiding broken symlinks).
fn input_needs_symlink(f: &InputSaveField) -> bool {
    let v = f.value.trim();
    f.is_file && !v.is_empty() && !v.starts_with("/input/")
}

async fn input_save_handler(Json(body): Json<InputSaveBody>) -> Json<serde_json::Value> {
    let session = { input_session_lock().await.lock().await.clone() };
    let session = match session {
        Some(s) => s,
        None => return Json(serde_json::json!({ "ok": false, "error": "No input session is active." })),
    };
    let cfg = &session.config;
    let input_dir = cfg.full_input_dir();
    let is_cloud = cfg.connection_type == "cloud" && cfg.cloud_provider == "aws";

    let _ = ssh_run_fast(cfg, &format!("mkdir -p '{}'", input_dir.replace('\'', "'\\''"))).await;

    let any_pick = body.fields.iter().any(input_needs_symlink);
    if is_cloud && any_pick {
        let region = if cfg.aws_region.trim().is_empty() { "us-east-1" } else { cfg.aws_region.as_str() };
        let mount_cmd = format!(
            "sudo apt-get install -y -qq fuse3 >/dev/null 2>&1 || true; \
             grep -qxF 'user_allow_other' /etc/fuse.conf 2>/dev/null || echo 'user_allow_other' | sudo tee -a /etc/fuse.conf >/dev/null; \
             mkdir -p /home/ubuntu/s3input; \
             mountpoint -q /home/ubuntu/s3input || rclone mount ':s3,provider=AWS,env_auth=true,region={}:{}' /home/ubuntu/s3input --read-only --allow-other --daemon --dir-cache-time 10s --vfs-cache-mode minimal >/tmp/autopipe-rclone-mount.log 2>&1; \
             sleep 2",
            region, cfg.aws_bucket
        );
        if let Err(e) = ssh_run_fast(cfg, &mount_cmd).await {
            return Json(serde_json::json!({ "ok": false, "error": format!("S3 mount failed: {}", e) }));
        }
    }

    let mut new_values: Vec<(String, String)> = Vec::new();
    let mut prepared: Vec<String> = Vec::new();
    for f in &body.fields {
        if input_needs_symlink(f) {
            // The user picked a file: value is an S3 key (cloud) or a server path.
            let src = if is_cloud {
                format!("/home/ubuntu/s3input/{}", f.value.trim())
            } else {
                f.value.trim().to_string()
            };
            let base = src.rsplit('/').next().unwrap_or(&src).to_string();
            let link = format!("{}/{}", input_dir.trim_end_matches('/'), base);
            let cmd = format!(
                "ln -sf '{}' '{}'",
                src.replace('\'', "'\\''"),
                link.replace('\'', "'\\''")
            );
            let _ = ssh_run_fast(cfg, &cmd).await;
            // File paths are always quoted strings in config.yaml.
            new_values.push((f.key.clone(), input_dquote(&format!("/input/{}", base))));
            prepared.push(base);
        } else {
            // Non-file, or a file left at its /input default: write with the
            // field's original type so ints stay ints and strings stay strings.
            let ty = if f.is_file { "string" } else { f.ty.as_str() };
            new_values.push((f.key.clone(), input_format_value(&f.value, ty)));
        }
    }

    let cfg_path = format!("{}/config.yaml", session.pipeline_dir.trim_end_matches('/'));
    let esc = cfg_path.replace('\'', "'\\''");
    let raw = match ssh_run_fast(cfg, &format!("cat '{}'", esc)).await {
        Ok((out, 0)) => clean_content(&out),
        Ok((out, _)) => return Json(serde_json::json!({ "ok": false, "error": format!("Cannot read config.yaml: {}", out.trim()) })),
        Err(e) => return Json(serde_json::json!({ "ok": false, "error": e })),
    };
    let mut updated = raw;
    for (k, v) in &new_values {
        updated = input_set_yaml_value(&updated, k, v);
    }
    let b64 = base64::engine::general_purpose::STANDARD.encode(updated.as_bytes());
    let write_cmd = format!("printf '%s' '{}' | base64 -d > '{}'", b64, esc);
    match ssh_run_fast(cfg, &write_cmd).await {
        Ok((_, 0)) => {}
        Ok((out, _)) => return Json(serde_json::json!({ "ok": false, "error": format!("Cannot write config.yaml: {}", out.trim()) })),
        Err(e) => return Json(serde_json::json!({ "ok": false, "error": e })),
    }

    Json(serde_json::json!({ "ok": true, "input_dir": input_dir, "prepared": prepared }))
}

const INPUT_PAGE_HTML: &str = r####"<!doctype html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>AutoPipe - Pipeline input</title>
<link rel="icon" href="/logo.png" type="image/png">
<style>
  :root{--bg:#f8fafc;--card:#fff;--border:#e2e8f0;--strong:#cbd5e1;--text:#0f172a;--muted:#64748b;--accent:#0f4c5c;--accent2:#0d3d4a;--req:#dc2626}
  *{box-sizing:border-box}
  body{margin:0;font-family:system-ui,-apple-system,Segoe UI,Roboto,sans-serif;background:var(--bg);color:var(--text)}
  .top{display:flex;align-items:center;gap:10px;padding:14px 20px;border-bottom:1px solid var(--border);background:var(--card)}
  .top img{width:26px;height:26px;border-radius:6px}
  .top .name{font-weight:700;font-size:1.05rem}
  .top .ptitle{color:var(--muted);font-weight:500;font-size:1.02rem}
  .wrap{max-width:780px;margin:0 auto;padding:22px 20px 90px}
  .sub{color:var(--muted);font-size:.9rem;margin:0 0 2px}
  .reqnote{color:var(--muted);font-size:.82rem;margin:0 0 20px}
  .reqnote b{color:var(--req)}
  .field{display:grid;grid-template-columns:190px 1fr auto;gap:8px 12px;align-items:start;margin-bottom:14px}
  .field .lab{font-weight:600;font-size:.88rem;padding-top:8px}
  .field .lab .star{color:var(--req);margin-right:3px}
  .field .ctl input,.field .ctl select{padding:8px 10px;border:1px solid var(--strong);border-radius:6px;font-size:.9rem;width:100%;font-family:inherit;background:var(--card);color:var(--text)}
  .field .ctl input:focus,.field .ctl select:focus{outline:none;border-color:var(--accent);box-shadow:0 0 0 3px rgba(15,76,92,.1)}
  .field .desc{grid-column:2 / 4;color:var(--muted);font-size:.78rem;margin-top:4px;line-height:1.4}
  .browse{padding:8px 12px;border:1px solid var(--strong);border-radius:6px;background:var(--card);cursor:pointer;font-size:.85rem;white-space:nowrap}
  .browse:hover{border-color:var(--accent);color:var(--accent)}
  .bar{position:fixed;left:0;right:0;bottom:0;background:var(--card);border-top:1px solid var(--border);padding:12px 20px;display:flex;justify-content:flex-end;gap:12px;align-items:center}
  .save{padding:9px 18px;border:none;border-radius:6px;background:var(--accent);color:#fff;font-weight:600;cursor:pointer;font-size:.9rem}
  .save:hover{background:var(--accent2)}
  .save:disabled{opacity:.5;cursor:default}
  .msg{font-size:.85rem;color:var(--muted)}
  .msg.ok{color:var(--accent)}
  .msg.err{color:var(--req)}
  .overlay{position:fixed;inset:0;background:rgba(0,0,0,.45);display:none;align-items:center;justify-content:center;padding:24px}
  .overlay.show{display:flex}
  .modal{background:var(--card);border:1px solid var(--border);border-radius:10px;width:100%;max-width:560px;max-height:80vh;display:flex;flex-direction:column;box-shadow:0 12px 32px rgba(0,0,0,.25)}
  .mhead{padding:12px 16px;border-bottom:1px solid var(--border);display:flex;align-items:center;gap:10px}
  .mhead .cwd{font-size:.8rem;color:var(--muted);flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .mbody{overflow:auto;padding:6px 0}
  .row{display:flex;align-items:center;gap:8px;padding:8px 16px;cursor:pointer;font-size:.9rem}
  .row:hover{background:var(--bg)}
  .row .ic{width:18px;text-align:center}
  .row .sz{margin-left:auto;color:var(--muted);font-size:.78rem}
  .mfoot{padding:10px 16px;border-top:1px solid var(--border);display:flex;justify-content:flex-end;gap:10px}
  .btn{padding:7px 14px;border:1px solid var(--strong);border-radius:6px;background:var(--card);cursor:pointer;font-size:.85rem}
  .up{padding:4px 10px;border:1px solid var(--strong);border-radius:6px;background:var(--card);cursor:pointer;font-size:.8rem}
  .empty{padding:16px;color:var(--muted);font-size:.85rem}
</style></head>
<body>
<div class="top"><img src="/logo.png" alt=""><span class="name">AutoPipe</span><span class="ptitle">- Pipeline input</span></div>
<div class="wrap">
  <p class="sub" id="sub">Edit input values. Defaults are pre-filled.</p>
  <p class="reqnote"><b>*</b> indicates a required field.</p>
  <div id="fields"></div>
</div>
<div class="bar">
  <span class="msg" id="msg"></span>
  <button class="save" id="save" disabled>Save &amp; prepare input</button>
</div>
<div class="overlay" id="ov"><div class="modal">
  <div class="mhead"><button class="up" id="up">↑ Up</button><span class="cwd" id="cwd"></span></div>
  <div class="mbody" id="mbody"></div>
  <div class="mfoot"><button class="btn" id="close">Close</button></div>
</div></div>
<script>
let SOURCE="ssh", FIELDS=[], activeKey=null, curPath="";
const $=id=>document.getElementById(id);
function fmtSize(n){if(n==null)return"";const u=["B","KB","MB","GB","TB"];let i=0,v=n;while(v>=1024&&i<u.length-1){v/=1024;i++}return v.toFixed(v<10&&i>0?1:0)+u[i]}
async function load(){
  const r=await fetch("/api/input/config");const d=await r.json();
  if(!d.ok){$("sub").textContent="Error: "+d.error;return}
  SOURCE=d.source;FIELDS=d.fields;
  const c=$("fields");c.innerHTML="";
  FIELDS.forEach(f=>{
    const row=document.createElement("div");row.className="field";
    const lab=document.createElement("div");lab.className="lab";
    lab.innerHTML=(f.required?'<span class="star">*</span>':'')+f.key;
    const ctl=document.createElement("div");ctl.className="ctl";
    let inp;
    if(f.type==="bool"){
      inp=document.createElement("select");
      ["true","false"].forEach(o=>{const op=document.createElement("option");op.value=o;op.textContent=o;if(String(f.value)===o)op.selected=true;inp.appendChild(op)});
    } else {
      inp=document.createElement("input");
      inp.type=(f.type==="int"||f.type==="float")?"number":"text";
      if(f.type==="float")inp.step="any";
      inp.value=f.value;
    }
    inp.dataset.key=f.key;
    ctl.appendChild(inp);
    row.appendChild(lab);row.appendChild(ctl);
    if(f.is_file){const b=document.createElement("button");b.className="browse";b.textContent="Browse…";b.onclick=()=>openBrowser(f.key);row.appendChild(b);}
    else{const sp=document.createElement("span");row.appendChild(sp);}
    if(f.description){const d2=document.createElement("div");d2.className="desc";d2.textContent=f.description;row.appendChild(d2);}
    c.appendChild(row);
  });
  $("save").disabled=false;
}
function openBrowser(key){activeKey=key;curPath="";$("ov").classList.add("show");list("")}
async function list(path){
  curPath=path;$("mbody").innerHTML='<div class="empty">Loading…</div>';
  const r=await fetch("/api/input/browse?path="+encodeURIComponent(path));const d=await r.json();
  if(!d.ok){$("mbody").innerHTML='<div class="empty">Error: '+d.error+'</div>';return}
  $("cwd").textContent=(SOURCE==="s3"?("s3://"+(d.cwd||"")):(d.cwd||""));
  $("up").disabled=(d.parent==null);$("up").dataset.p=d.parent==null?"":d.parent;
  const b=$("mbody");b.innerHTML="";
  (d.folders||[]).forEach(f=>{const row=el(f.name,"📁",null);row.onclick=()=>list(f.path);b.appendChild(row)});
  (d.files||[]).forEach(f=>{const row=el(f.name,"📄",f.size);row.onclick=()=>pick(f.path);b.appendChild(row)});
  if((d.folders||[]).length===0&&(d.files||[]).length===0)b.innerHTML='<div class="empty">Empty.</div>';
}
function el(name,ic,sz){const r=document.createElement("div");r.className="row";r.innerHTML='<span class="ic">'+ic+'</span><span>'+name+'</span>'+(sz!=null?'<span class="sz">'+fmtSize(sz)+'</span>':'');return r}
function pick(path){
  document.querySelectorAll('#fields .ctl input, #fields .ctl select').forEach(i=>{if(i.dataset.key===activeKey)i.value=path});
  $("ov").classList.remove("show");
}
$("up").onclick=()=>{const p=$("up").dataset.p;list(p)};
$("close").onclick=()=>$("ov").classList.remove("show");
$("save").onclick=async()=>{
  $("save").disabled=true;setMsg("Preparing input…","");
  const fields=[...document.querySelectorAll('#fields .ctl input, #fields .ctl select')].map(i=>{
    const f=FIELDS.find(x=>x.key===i.dataset.key)||{};
    return{key:i.dataset.key,value:i.value,is_file:!!f.is_file,type:f.type||"string"};
  });
  const r=await fetch("/api/input/save",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({fields})});
  const d=await r.json();
  if(d.ok){setMsg("Saved. You can close this tab.","ok");}
  else{setMsg("Error: "+d.error,"err");$("save").disabled=false;}
};
function setMsg(t,c){const m=$("msg");m.textContent=t;m.className="msg"+(c?" "+c:"")}
load();
</script></body></html>
"####;
