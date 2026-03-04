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

#[derive(Clone, Serialize)]
struct PluginManifest {
    name: String,
    version: String,
    description: String,
    extensions: Vec<String>,
    entry: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    style: Option<String>,
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
) -> Result<String, String> {
    // Check if server is already running (to decide whether to open a new tab)
    let already_running = {
        let lock = get_viewer_lock().await;
        let handle = lock.lock().await;
        handle.is_some()
    };

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

    // Only open a new browser tab on first call; subsequent calls
    // update files in-place and the existing tab auto-refreshes on focus.
    if !already_running {
        open::that(&url).map_err(|e| format!("Failed to open browser: {}", e))?;
    }

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

    // Add remote files
    let remote = get_remote_files_lock().await.lock().await;
    for (name, entry) in remote.iter() {
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

/// Query parameters for the /data/ endpoint (server-side pagination).
#[derive(Deserialize)]
struct DataQuery {
    #[serde(default)]
    page: Option<usize>,
    #[serde(default)]
    page_size: Option<usize>,
}

/// Docker image for samtools (used for BAM file parsing via Docker on remote server).
const SAMTOOLS_DOCKER: &str = "biocontainers/samtools:v1.9-4-deb_cv1";

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

/// Data handler: server-side pagination for genomics files (BAM/VCF/BED/GFF).
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

    // For BAM files: extract parent dir and filename for Docker volume mount
    let bam_dir = std::path::Path::new(&remote_path)
        .parent()
        .unwrap_or(std::path::Path::new("/"))
        .to_string_lossy()
        .to_string();
    let bam_file = std::path::Path::new(&remote_path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let start = page * page_size + 1; // sed is 1-indexed
    let end = start + page_size - 1;

    // Get total row count (cached)
    let total = {
        let cache = get_row_count_cache().await.lock().await;
        cache.get(&filename).copied()
    };
    let total = match total {
        Some(t) => t,
        None => {
            let count_cmd = match ext.as_str() {
                // BAM: only fetch first 100 reads to avoid timeout on large files
                "bam" => format!(
                    "docker run --rm -v \"{}:/data:ro\" {} sh -c \"samtools view /data/{} 2>/dev/null | head -100 | wc -l\"",
                    bam_dir, SAMTOOLS_DOCKER, bam_file
                ),
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

    // Get metadata (header/meta lines) — only on first page
    let mut meta = serde_json::Value::Null;
    let mut header_val = serde_json::Value::Null;
    let mut refs_val = serde_json::Value::Null;
    let mut col_headers: Vec<String> = Vec::new();

    if page == 0 {
        match ext.as_str() {
            "bam" => {
                // SAM header
                if let Ok((hdr, 0)) =
                    ssh_run(&ssh_cfg, &format!(
                        "docker run --rm -v \"{}:/data:ro\" {} samtools view -H \"/data/{}\" 2>/dev/null",
                        bam_dir, SAMTOOLS_DOCKER, bam_file
                    )).await
                {
                    header_val = serde_json::Value::String(hdr.trim().to_string());
                }
                // Reference sequences from header
                if let Ok((hdr_text, 0)) =
                    ssh_run(&ssh_cfg, &format!(
                        "docker run --rm -v \"{}:/data:ro\" {} sh -c \"samtools view -H /data/{} 2>/dev/null | grep ^@SQ\"",
                        bam_dir, SAMTOOLS_DOCKER, bam_file
                    )).await
                {
                    let mut refs = Vec::new();
                    for line in hdr_text.trim().lines() {
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
                    refs_val = serde_json::Value::Array(refs);
                }
                col_headers = vec![
                    "Read Name", "Flag", "Chr", "Pos", "MAPQ", "CIGAR", "Sequence",
                ]
                .into_iter()
                .map(|s| s.to_string())
                .collect();
            }
            "vcf" => {
                if let Ok((m, 0)) =
                    ssh_run(&ssh_cfg, &format!("grep '^#' '{}'", remote_path)).await
                {
                    let lines: Vec<&str> = m.trim().lines().collect();
                    // Last # line is the header row
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
                    "chrom",
                    "chromStart",
                    "chromEnd",
                    "name",
                    "score",
                    "strand",
                    "thickStart",
                    "thickEnd",
                    "itemRgb",
                    "blockCount",
                    "blockSizes",
                    "blockStarts",
                ];
                // Detect column count from first data line
                if let Ok((first_line, 0)) = ssh_run(
                    &ssh_cfg,
                    &format!(
                        "grep -v -E '^#|^track|^browser' '{}' | head -1",
                        remote_path
                    ),
                )
                .await
                {
                    let ncols = first_line.trim().split('\t').count().min(12);
                    col_headers = bed_cols[..ncols].iter().map(|s| s.to_string()).collect();
                }
            }
            "gff" | "gtf" | "gff3" => {
                col_headers = vec![
                    "seqid", "source", "type", "start", "end", "score", "strand", "phase",
                    "attributes",
                ]
                .into_iter()
                .map(|s| s.to_string())
                .collect();
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

    // Get rows for this page
    let rows_cmd = match ext.as_str() {
        // BAM: only first 100 reads (no pagination) to avoid timeout
        "bam" => format!(
            "docker run --rm -v \"{}:/data:ro\" {} sh -c \"samtools view /data/{} 2>/dev/null | head -100\"",
            bam_dir, SAMTOOLS_DOCKER, bam_file
        ),
        "vcf" => format!(
            "grep -v '^#' '{}' | sed -n '{},{}p'",
            remote_path, start, end
        ),
        "bed" => format!(
            "grep -v -E '^#|^track|^browser' '{}' | sed -n '{},{}p'",
            remote_path, start, end
        ),
        "gff" | "gtf" | "gff3" => format!(
            "grep -v '^#' '{}' | sed -n '{},{}p'",
            remote_path, start, end
        ),
        _ => format!("sed -n '{},{}p' '{}'", start, end, remote_path),
    };

    let rows: Vec<Vec<String>> = match ssh_run(&ssh_cfg, &rows_cmd).await {
        Ok((output, 0)) => output
            .trim()
            .lines()
            .filter(|l| !l.is_empty())
            .map(|line| line.split('\t').map(|s| s.to_string()).collect())
            .collect(),
        Ok((output, _)) => {
            return Json(serde_json::json!({"error": output.trim()})).into_response();
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
    if !header_val.is_null() {
        result["header"] = header_val;
    }
    if !refs_val.is_null() {
        result["refs"] = refs_val;
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
<title>AutoPipe Results Viewer</title>
<link rel="icon" href="/logo.png" type="image/png">
<script src="https://cdn.jsdelivr.net/npm/jsfive@0.3.10/dist/browser/hdf5.js"></script>
<link rel="stylesheet" href="https://cdn.jsdelivr.net/gh/highlightjs/cdn-release@11.9.0/build/styles/github.min.css">
<script src="https://cdn.jsdelivr.net/gh/highlightjs/cdn-release@11.9.0/build/highlight.min.js"></script>
<script src="https://cdn.jsdelivr.net/gh/highlightjs/cdn-release@11.9.0/build/languages/dockerfile.min.js"></script>
<script src="https://cdn.jsdelivr.net/gh/highlightjs/cdn-release@11.9.0/build/languages/yaml.min.js"></script>
<script src="https://cdn.jsdelivr.net/gh/highlightjs/cdn-release@11.9.0/build/languages/python.min.js"></script>
<script src="https://cdn.jsdelivr.net/gh/highlightjs/cdn-release@11.9.0/build/languages/markdown.min.js"></script>
<script src="https://cdn.jsdelivr.net/gh/highlightjs/cdn-release@11.9.0/build/languages/json.min.js"></script>
<script src="https://cdn.jsdelivr.net/gh/highlightjs/cdn-release@11.9.0/build/languages/bash.min.js"></script>
<script src="https://cdn.jsdelivr.net/gh/highlightjs/cdn-release@11.9.0/build/languages/r.min.js"></script>
<script src="https://cdn.jsdelivr.net/gh/highlightjs/cdn-release@11.9.0/build/languages/xml.min.js"></script>
<script src="https://cdn.jsdelivr.net/gh/highlightjs/cdn-release@11.9.0/build/languages/ini.min.js"></script>
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

  /* Image viewer */
  .img-viewer {{ overflow: auto; }}
  .img-viewer img {{ max-width: 100%; height: auto; transition: transform 0.15s; transform-origin: top left; }}

  /* Text viewer */
  .text-viewer {{ padding: 0; font-family: 'SF Mono', 'Fira Code', 'Consolas', monospace; font-size: 13px; line-height: 1.6; white-space: pre-wrap; word-break: break-all; overflow: auto; max-height: 100%; background: transparent; }}
  .text-viewer code {{ font-family: inherit; font-size: inherit; line-height: inherit; background: transparent; }}

  /* PDF viewer */
  .pdf-viewer {{ width: 100%; height: 100%; border: none; border-radius: 8px; }}

  /* View mode tabs */
  .view-tabs {{ display: flex; gap: 2px; margin-right: 12px; background: #eee; border-radius: 6px; padding: 2px; }}
  .view-tab {{ padding: 4px 14px; border: none; border-radius: 4px; background: transparent; color: #666; font-size: 12px; font-weight: 500; cursor: pointer; transition: all 0.15s; }}
  .view-tab:hover {{ color: #333; }}
  .view-tab.active {{ background: #fff; color: #0366d6; box-shadow: 0 1px 2px rgba(0,0,0,0.08); }}
  .view-tab.disabled {{ color: #bbb; cursor: not-allowed; }}

  /* Pagination */
  .pagination {{ display: flex; align-items: center; gap: 8px; padding: 10px 0; justify-content: center; font-size: 13px; color: #666; }}
  .pagination button {{ padding: 4px 12px; border: 1px solid #ddd; border-radius: 4px; background: #f8f8f8; cursor: pointer; font-size: 12px; }}
  .pagination button:hover {{ background: #eee; }}
  .pagination button:disabled {{ color: #ccc; cursor: not-allowed; background: #fafafa; }}

  /* IGV viewer */
  .igv-viewer {{ width: 100%; min-height: 500px; }}

  /* Genomics table viewer */
  .genomics-viewer {{ overflow: auto; }}
  .genomics-viewer table {{ width: 100%; border-collapse: collapse; font-size: 13px; table-layout: auto; }}
  .genomics-viewer th {{ background: #f5f5f5; padding: 8px 12px; text-align: left; font-weight: 600; border-bottom: 2px solid #e5e5e5; position: sticky; top: 0; white-space: nowrap; }}
  .genomics-viewer td {{ padding: 6px 12px; border-bottom: 1px solid #f0f0f0; font-family: 'SF Mono', 'Fira Code', 'Consolas', monospace; font-size: 12px; white-space: nowrap; }}
  .genomics-viewer tr:hover td {{ background: #f0f7ff; }}
  .genomics-viewer .section-title {{ font-size: 14px; font-weight: 600; margin: 16px 0 8px; color: #333; }}
  .genomics-viewer .meta {{ font-size: 12px; color: #666; margin-bottom: 12px; }}
  .genomics-viewer .seq {{ font-family: 'SF Mono', monospace; font-size: 11px; letter-spacing: 1px; }}
  .genomics-viewer .base-A {{ color: #2ecc71; font-weight: 600; }}
  .genomics-viewer .base-T {{ color: #e74c3c; font-weight: 600; }}
  .genomics-viewer .base-C {{ color: #3498db; font-weight: 600; }}
  .genomics-viewer .base-G {{ color: #f39c12; font-weight: 600; }}
  .fasta-viewer {{ overflow: auto; font-family: 'SF Mono', 'Fira Code', 'Consolas', monospace; font-size: 13px; line-height: 1.6; }}
  .fasta-viewer .seq-header {{ font-weight: 700; color: #2c3e50; margin-top: 12px; margin-bottom: 4px; }}
  .fasta-viewer .seq-line {{ letter-spacing: 1px; word-break: break-all; }}

  /* HDF5 viewer */
  .hdf5-viewer {{ overflow: auto; }}
  .hdf5-viewer table {{ width: 100%; border-collapse: collapse; font-size: 13px; }}
  .hdf5-viewer th {{ background: #f5f5f5; padding: 8px 12px; text-align: left; font-weight: 600; border-bottom: 2px solid #e5e5e5; position: sticky; top: 0; }}
  .hdf5-viewer td {{ padding: 6px 12px; border-bottom: 1px solid #f0f0f0; }}
  .hdf5-viewer tr:hover td {{ background: #f0f7ff; }}
  .hdf5-section {{ margin-bottom: 20px; }}
  .hdf5-section h3 {{ font-size: 14px; font-weight: 600; margin-bottom: 8px; color: #333; }}

  /* HDF5 tree layout */
  .hdf5-tree-layout {{ display: flex; height: 100%; }}
  .hdf5-tree {{ width: 280px; min-width: 280px; overflow-y: auto; border-right: 1px solid #e5e5e5; padding: 8px 0; font-size: 12px; }}
  .hdf5-tree-item {{ display: block; padding: 4px 12px; cursor: pointer; white-space: nowrap; color: #333; }}
  .hdf5-tree-item:hover {{ background: #f5f5f5; }}
  .hdf5-tree-item.active {{ background: #f0f7ff; color: #0366d6; }}
  .hdf5-tree-item.group {{ font-weight: 600; }}
  .hdf5-tree-toggle {{ display: inline-block; width: 12px; text-align: center; margin-right: 4px; font-size: 10px; }}
  .hdf5-detail {{ flex: 1; overflow: auto; padding: 16px; }}

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
  <span class="logo"><img src="/logo.png" alt="" class="logo-icon">AutoPipe</span>
  <span class="header-sub">Results Viewer</span>
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
var currentScale = 1;
var currentViewMode = 'data';
var loadedPlugins = {{}};

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

// IGV-compatible extensions (can show Data tab, IGV tab, or both)
var igvDualExts = ['bam','vcf','bed','gff','gtf','gff3','fasta','fa'];
var igvOnlyExts = ['cram','bcf'];

function hasReference() {{
  return !!REFERENCE;
}}

function getIgvReference() {{
  // If reference is a filename in our files → use local URL
  if (REFERENCE && FILES.some(function(f) {{ return f.name === REFERENCE; }})) {{
    return {{ fastaURL: '/file/' + encodeURIComponent(REFERENCE), indexed: false }};
  }}
  // Otherwise treat as genome ID (hg38, mm10, etc.)
  return null;
}}

function getIgvGenomeId() {{
  if (!REFERENCE) return null;
  var knownGenomes = KNOWN_GENOMES.map(function(g) {{ return g.id; }});
  if (knownGenomes.indexOf(REFERENCE) >= 0) return REFERENCE;
  if (FILES.some(function(f) {{ return f.name === REFERENCE; }})) return null;
  return REFERENCE;
}}

// ── Genome dropdown for IGV ──
var KNOWN_GENOMES = [
  {{id:'hg38', label:'Human (GRCh38/hg38)'}},
  {{id:'hg19', label:'Human (GRCh37/hg19)'}},
  {{id:'mm39', label:'Mouse (GRCm39/mm39)'}},
  {{id:'mm10', label:'Mouse (GRCm38/mm10)'}},
  {{id:'rn7',  label:'Rat (mRatBN7.2/rn7)'}},
  {{id:'rn6',  label:'Rat (Rnor_6.0/rn6)'}},
  {{id:'dm6',  label:'Fruit fly (BDGP6/dm6)'}},
  {{id:'ce11', label:'C. elegans (WBcel235/ce11)'}},
  {{id:'danRer11', label:'Zebrafish (GRCz11/danRer11)'}},
  {{id:'sacCer3',  label:'Yeast (sacCer3)'}},
  {{id:'tair10',   label:'Arabidopsis (TAIR10)'}},
  {{id:'galGal6',  label:'Chicken (GRCg6a/galGal6)'}}
];
var selectedGenome = null;

function buildGenomeDropdown() {{
  var current = selectedGenome || REFERENCE || '';
  var html = '<span style="font-size:12px;color:#888;font-weight:500;margin-right:4px">Reference:</span>';
  html += '<select class="btn" id="genomeSelect" onchange="onGenomeChange(this.value)" style="font-size:12px;padding:4px 8px;max-width:220px">';
  // If REFERENCE is a local FASTA file
  var localFasta = FILES.find(function(f) {{ return f.name === REFERENCE; }});
  if (localFasta) {{
    html += '<option value="' + REFERENCE + '"' + (current === REFERENCE ? ' selected' : '') + '>Local: ' + REFERENCE + '</option>';
  }}
  // Known genomes
  KNOWN_GENOMES.forEach(function(g) {{
    html += '<option value="' + g.id + '"' + (current === g.id ? ' selected' : '') + '>' + g.label + '</option>';
  }});
  html += '</select>';
  return html;
}}

function onGenomeChange(val) {{
  selectedGenome = val;
  if (currentFile) {{
    var ext = currentFile.split('.').pop().toLowerCase();
    var content = document.getElementById('viewerContent');
    if (currentViewMode === 'igv' || igvOnlyExts.indexOf(ext) >= 0) {{
      renderIgvViewer(currentFile, ext, content);
    }}
  }}
}}

// Files recognized as text by filename (no extension match needed)
var textFileNames = ['snakefile','dockerfile','makefile','readme','readme.md','license','license.md'];
function isTextByName(name) {{
  var lower = name.toLowerCase();
  var base = lower.split('/').pop() || lower;
  return textFileNames.indexOf(base) >= 0;
}}

// Map filename/extension to highlight.js language
function detectHljsLang(name) {{
  var lower = name.toLowerCase();
  var base = lower.split('/').pop() || lower;
  if (base === 'snakefile' || lower.endsWith('.smk')) return 'python';
  if (base === 'dockerfile' || lower.endsWith('.dockerfile')) return 'dockerfile';
  if (base === 'makefile') return 'makefile';
  var ext = lower.split('.').pop();
  var langMap = {{
    'py': 'python', 'r': 'r', 'sh': 'bash', 'bash': 'bash',
    'json': 'json', 'yaml': 'yaml', 'yml': 'yaml',
    'xml': 'xml', 'md': 'markdown',
    'cfg': 'ini', 'ini': 'ini', 'toml': 'ini',
    'nf': 'groovy', 'js': 'javascript', 'ts': 'typescript'
  }};
  return langMap[ext] || null;
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

  var imageExts = ['png','jpg','jpeg','gif','svg','webp','bmp','tiff','tif'];
  var delimExts = ['csv','tsv','tab'];
  var textExts = ['txt','log','json','yaml','yml','xml','md','sh','py','r','R','nf','smk','cfg','ini','toml','fastq','fq','dockerfile'];
  var hdf5Exts = ['h5ad','h5','hdf5'];

  // Dual-tab files: Data + IGV
  if (igvDualExts.indexOf(ext) >= 0) {{
    var ref = hasReference();
    if (ref) {{
      // Reference available: show both Data and IGV tabs
      var tabsHtml = '<div class="view-tabs">';
      tabsHtml += '<button class="view-tab' + (mode === 'data' ? ' active' : '') + '" onclick="selectFileWithMode(\'' + name.replace(/'/g,"\\'") + '\',\'data\')">Data</button>';
      tabsHtml += '<button class="view-tab' + (mode === 'igv' ? ' active' : '') + '" onclick="selectFileWithMode(\'' + name.replace(/'/g,"\\'") + '\',\'igv\')">IGV</button>';
      tabsHtml += '</div>';
      var genomeHtml = (mode === 'igv') ? buildGenomeDropdown() : '';
      actions.innerHTML = tabsHtml + genomeHtml + '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a>';
      if (mode === 'igv') {{
        renderIgvViewer(name, ext, content);
      }} else {{
        renderDataViewer(name, ext, content);
      }}
    }} else {{
      // No reference: show Data only (no tabs)
      actions.innerHTML = '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a>';
      renderDataViewer(name, ext, content);
    }}
    return;
  }}

  // IGV-only files: CRAM/BCF
  if (igvOnlyExts.indexOf(ext) >= 0) {{
    if (hasReference()) {{
      var genomeHtml = buildGenomeDropdown();
      actions.innerHTML = genomeHtml + '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a>';
      renderIgvViewer(name, ext, content);
    }} else {{
      actions.innerHTML = '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a>';
      content.innerHTML =
        '<div class="no-preview">' +
          '<div class="no-preview-icon">🧬</div>' +
          '<p class="no-preview-title">' + name + '</p>' +
          '<p class="no-preview-msg">.' + ext + ' files require a reference genome for IGV viewer.<br>Provide a reference path or genome ID (e.g., hg38) when calling show_results.</p>' +
          '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a>' +
        '</div>';
    }}
    return;
  }}

  // Other file types (no tabs)
  if (imageExts.indexOf(ext) >= 0) {{
    renderImageViewer(name, actions, content);
  }} else if (ext === 'pdf') {{
    renderPdfViewer(name, actions, content);
  }} else if (delimExts.indexOf(ext) >= 0) {{
    renderDelimitedViewer(name, ext, actions, content);
  }} else if (textExts.indexOf(ext) >= 0 || isTextByName(name)) {{
    renderTextViewer(name, actions, content);
  }} else if (hdf5Exts.indexOf(ext) >= 0) {{
    renderHdf5Viewer(name, actions, content);
  }} else {{
    var plugin = findPlugin(ext);
    if (plugin) {{
      renderPluginViewer(name, plugin, actions, content);
    }} else {{
      renderNoPreview(name, ext, actions, content);
    }}
  }}
}}

// Route to the correct Data viewer based on extension
function renderDataViewer(name, ext, content) {{
  if (ext === 'vcf') renderVcfViewer(name, content);
  else if (ext === 'bed') renderBedViewer(name, content);
  else if (ext === 'gff' || ext === 'gtf' || ext === 'gff3') renderGffViewer(name, content);
  else if (ext === 'fasta' || ext === 'fa') renderFastaViewer(name, content);
  else if (ext === 'bam') renderBamViewer(name, content);
}}

// ── Image Viewer ──
function renderImageViewer(name, actions, content) {{
  actions.innerHTML =
    '<button class="btn zoom-btn" onclick="zoomImage(-1)">−</button>' +
    '<span class="btn zoom-label" id="zoomLabel">100%</span>' +
    '<button class="btn zoom-btn" onclick="zoomImage(1)">+</button>' +
    '<button class="btn zoom-btn" onclick="zoomImage(0)">↺</button>' +
    '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a>';
  content.innerHTML = '<div class="img-viewer"><img id="viewerImg" src="/file/' + encodeURIComponent(name) + '" alt="' + name + '"></div>';
}}

function zoomImage(dir) {{
  var img = document.getElementById('viewerImg');
  if (!img) return;
  if (dir === 0) currentScale = 1;
  else currentScale = Math.max(0.1, Math.min(5, currentScale + dir * 0.25));
  img.style.transform = 'scale(' + currentScale + ')';
  img.style.maxWidth = currentScale > 1 ? 'none' : '100%';
  var label = document.getElementById('zoomLabel');
  if (label) label.textContent = Math.round(currentScale * 100) + '%';
}}

// ── PDF Viewer ──
function renderPdfViewer(name, actions, content) {{
  actions.innerHTML = '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a>';
  content.innerHTML = '<embed class="pdf-viewer" src="/file/' + encodeURIComponent(name) + '" type="application/pdf">';
}}

// ── Text Viewer ──
async function renderTextViewer(name, actions, content) {{
  actions.innerHTML = '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a>';
  var lang = detectHljsLang(name);
  if (lang && typeof hljs !== 'undefined') {{
    content.innerHTML = '<pre class="text-viewer"><code id="textContent" class="language-' + lang + '">Loading...</code></pre>';
  }} else {{
    content.innerHTML = '<div class="text-viewer" id="textContent">Loading...</div>';
  }}
  try {{
    var resp = await fetch('/file/' + encodeURIComponent(name));
    var text = await resp.text();
    var el = document.getElementById('textContent');
    el.textContent = text;
    if (lang && typeof hljs !== 'undefined') {{
      hljs.highlightElement(el);
    }}
  }} catch(e) {{
    document.getElementById('textContent').textContent = 'Error loading file: ' + e.message;
  }}
}}

// ── Delimited File Viewer (CSV/TSV) ──
var _delimCache = {{}};
var _delimPage = {{}};
async function renderDelimitedViewer(name, ext, actions, content) {{
  actions.innerHTML = '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a>';
  content.innerHTML = '<div class="genomics-viewer" id="delimDiv">Loading...</div>';
  try {{
    var resp = await fetch('/file/' + encodeURIComponent(name));
    var text = await resp.text();
    // Detect delimiter: tab for tsv/tab, comma for csv, auto-detect for others
    var delim = (ext === 'tsv' || ext === 'tab') ? '\t' : ',';
    var lines = text.split('\n').filter(function(l) {{ return l.trim().length > 0; }});
    if (lines.length === 0) {{
      document.getElementById('delimDiv').innerHTML = '<p class="meta">Empty file</p>';
      return;
    }}
    // Auto-detect: if first line has more tabs than commas, use tab
    if (ext !== 'tsv' && ext !== 'tab' && ext !== 'csv') {{
      var tabs = (lines[0].match(/\t/g)||[]).length;
      var commas = (lines[0].match(/,/g)||[]).length;
      if (tabs > commas) delim = '\t';
    }}
    // Parse rows
    var allRows = lines.map(function(line) {{
      if (delim === ',') {{
        // Simple CSV parse (handles quoted fields)
        var row = []; var cur = ''; var inQuote = false;
        for (var ci = 0; ci < line.length; ci++) {{
          var ch = line[ci];
          if (inQuote) {{
            if (ch === '"' && ci+1 < line.length && line[ci+1] === '"') {{ cur += '"'; ci++; }}
            else if (ch === '"') {{ inQuote = false; }}
            else {{ cur += ch; }}
          }} else {{
            if (ch === '"') {{ inQuote = true; }}
            else if (ch === ',') {{ row.push(cur); cur = ''; }}
            else {{ cur += ch; }}
          }}
        }}
        row.push(cur);
        return row;
      }}
      return line.split(delim);
    }});
    // First row as header
    var headers = allRows[0];
    var dataRows = allRows.slice(1);
    _delimCache[name] = {{ headers: headers, rows: dataRows }};
    _delimPage[name] = 0;
    _renderDelimPage(name);
  }} catch(e) {{
    document.getElementById('delimDiv').innerHTML = 'Error: ' + e.message;
  }}
}}
function _renderDelimPage(name) {{
  var div = document.getElementById('delimDiv'); if (!div) return;
  var cached = _delimCache[name]; if (!cached) return;
  var page = _delimPage[name] || 0;
  var headers = cached.headers;
  var dataRows = cached.rows;
  var html = '<p class="meta">' + dataRows.length.toLocaleString() + ' row(s) &middot; ' + headers.length + ' column(s)</p>';
  html += renderPaginatedTable('delimDiv', headers, dataRows, page, function(row) {{
    var r = '<tr>';
    for (var ci = 0; ci < headers.length; ci++) {{ r += '<td>' + ((row[ci]||'').replace(/</g,'&lt;')) + '</td>'; }}
    return r + '</tr>';
  }});
  div.innerHTML = html;
}}
window._paginate = function(divId, page) {{
  if (divId === 'delimDiv') {{
    var name = currentFile;
    if (page < 0) return;
    _delimPage[name] = page;
    _renderDelimPage(name);
  }}
}};

// ── Pagination helper ──
var PAGE_SIZE = 100;
function renderPaginatedTable(divId, headers, allRows, page, renderRow) {{
  var totalPages = Math.ceil(allRows.length / PAGE_SIZE) || 1;
  if (page < 0) page = 0;
  if (page >= totalPages) page = totalPages - 1;
  var start = page * PAGE_SIZE;
  var pageRows = allRows.slice(start, start + PAGE_SIZE);

  var html = '<table><tr>';
  headers.forEach(function(h) {{ html += '<th>' + h + '</th>'; }});
  html += '</tr>';
  pageRows.forEach(function(row, i) {{ html += renderRow(row, start + i); }});
  html += '</table>';

  if (totalPages > 1) {{
    html += '<div class="pagination">';
    html += '<button onclick="window._paginate(\'' + divId + '\',' + (page-1) + ')"' + (page <= 0 ? ' disabled' : '') + '>&laquo; Prev</button>';
    html += '<span>Page ' + (page+1) + ' / ' + totalPages + ' (' + allRows.length.toLocaleString() + ' rows)</span>';
    html += '<button onclick="window._paginate(\'' + divId + '\',' + (page+1) + ')"' + (page >= totalPages-1 ? ' disabled' : '') + '>Next &raquo;</button>';
    html += '</div>';
  }}
  return html;
}}

// ── Server-side paginated genomics viewers ──
// All genomics viewers (BAM/VCF/BED/GFF) use /data/ API for server-side pagination.
// Metadata (headers, refs) is cached; rows are fetched per page.
var _genomicsMetaCache = {{}};

async function fetchGenomicsPage(name, page) {{
  var resp = await fetch('/data/' + encodeURIComponent(name) + '?page=' + page + '&page_size=' + PAGE_SIZE);
  return await resp.json();
}}

function renderServerPaginatedTable(divId, name, headers, rows, total, page, renderRow) {{
  var totalPages = Math.ceil(total / PAGE_SIZE) || 1;
  var html = '<table><tr>';
  headers.forEach(function(h) {{ html += '<th>' + h + '</th>'; }});
  html += '</tr>';
  rows.forEach(function(row, i) {{ html += renderRow(row, page * PAGE_SIZE + i); }});
  html += '</table>';
  if (totalPages > 1) {{
    html += '<div class="pagination">';
    html += '<button onclick="window._serverPaginate(\'' + divId + '\',\'' + name.replace(/'/g,"\\'") + '\',' + (page-1) + ')"' + (page <= 0 ? ' disabled' : '') + '>&laquo; Prev</button>';
    html += '<span>Page ' + (page+1) + ' / ' + totalPages + ' (' + total.toLocaleString() + ' rows)</span>';
    html += '<button onclick="window._serverPaginate(\'' + divId + '\',\'' + name.replace(/'/g,"\\'") + '\',' + (page+1) + ')"' + (page >= totalPages-1 ? ' disabled' : '') + '>Next &raquo;</button>';
    html += '</div>';
  }}
  return html;
}}

window._serverPaginate = function(divId, name, page) {{
  if (page < 0) return;
  if (divId === 'bamDiv') _fetchAndRenderBam(name, page);
  else if (divId === 'vcfDiv') _fetchAndRenderVcf(name, page);
  else if (divId === 'bedDiv') _fetchAndRenderBed(name, page);
  else if (divId === 'gffDiv') _fetchAndRenderGff(name, page);
}};

// ── VCF Viewer (server-side) ──
async function renderVcfViewer(name, content) {{
  content.innerHTML = '<div class="genomics-viewer" id="vcfDiv">Loading VCF...</div>';
  try {{ await _fetchAndRenderVcf(name, 0); }}
  catch(e) {{ document.getElementById('vcfDiv').innerHTML = 'Error: ' + e.message; }}
}}
async function _fetchAndRenderVcf(name, page) {{
  var div = document.getElementById('vcfDiv'); if (!div) return;
  if (page > 0) div.innerHTML = '<div class="genomics-viewer">Loading page...</div>';
  var data = await fetchGenomicsPage(name, page);
  if (data.error) {{ div.innerHTML = 'Error: ' + data.error; return; }}
  if (page === 0 && data.meta) _genomicsMetaCache[name] = {{ meta: data.meta, col_headers: data.col_headers || [] }};
  var cached = _genomicsMetaCache[name] || {{}};
  var hdrs = cached.col_headers || [];
  var html = '<p class="meta">' + (data.total||0).toLocaleString() + ' variant(s)</p>';
  if (cached.meta) {{
    var metaLines = cached.meta.split('\n');
    html += '<details style="margin-bottom:12px"><summary style="cursor:pointer;font-size:13px;color:#666">Show metadata (' + metaLines.length + ' lines)</summary>';
    html += '<pre style="font-size:11px;color:#888;margin-top:4px;max-height:200px;overflow:auto">' + cached.meta.replace(/</g,'&lt;') + '</pre></details>';
  }}
  html += renderServerPaginatedTable('vcfDiv', name, hdrs, data.rows || [], data.total || 0, page, function(rec) {{
    var r = '<tr>';
    rec.forEach(function(val, i) {{
      r += (hdrs[i]==='REF'||hdrs[i]==='ALT') ? '<td class="seq">'+colorBases(val)+'</td>' : '<td>'+val+'</td>';
    }});
    return r + '</tr>';
  }});
  div.innerHTML = html;
}}

// ── BED Viewer (server-side) ──
async function renderBedViewer(name, content) {{
  content.innerHTML = '<div class="genomics-viewer" id="bedDiv">Loading BED...</div>';
  try {{ await _fetchAndRenderBed(name, 0); }}
  catch(e) {{ document.getElementById('bedDiv').innerHTML = 'Error: ' + e.message; }}
}}
async function _fetchAndRenderBed(name, page) {{
  var div = document.getElementById('bedDiv'); if (!div) return;
  if (page > 0) div.innerHTML = '<div class="genomics-viewer">Loading page...</div>';
  var data = await fetchGenomicsPage(name, page);
  if (data.error) {{ div.innerHTML = 'Error: ' + data.error; return; }}
  if (page === 0 && data.col_headers) _genomicsMetaCache[name] = {{ col_headers: data.col_headers }};
  var cached = _genomicsMetaCache[name] || {{}};
  var hdrs = cached.col_headers || ['chrom','chromStart','chromEnd'];
  var html = '<p class="meta">' + (data.total||0).toLocaleString() + ' region(s) &middot; BED' + hdrs.length + ' format</p>';
  html += renderServerPaginatedTable('bedDiv', name, hdrs, data.rows || [], data.total || 0, page, function(rec) {{
    var r = '<tr>'; rec.forEach(function(v) {{ r += '<td>'+v+'</td>'; }}); return r + '</tr>';
  }});
  div.innerHTML = html;
}}

// ── GFF/GTF Viewer (server-side) ──
async function renderGffViewer(name, content) {{
  content.innerHTML = '<div class="genomics-viewer" id="gffDiv">Loading GFF...</div>';
  try {{ await _fetchAndRenderGff(name, 0); }}
  catch(e) {{ document.getElementById('gffDiv').innerHTML = 'Error: ' + e.message; }}
}}
async function _fetchAndRenderGff(name, page) {{
  var div = document.getElementById('gffDiv'); if (!div) return;
  if (page > 0) div.innerHTML = '<div class="genomics-viewer">Loading page...</div>';
  var data = await fetchGenomicsPage(name, page);
  if (data.error) {{ div.innerHTML = 'Error: ' + data.error; return; }}
  if (page === 0 && data.meta) _genomicsMetaCache[name] = {{ meta: data.meta }};
  var cached = _genomicsMetaCache[name] || {{}};
  var colNames = ['seqid','source','type','start','end','score','strand','phase','attributes'];
  var html = '<p class="meta">' + (data.total||0).toLocaleString() + ' feature(s)</p>';
  if (cached.meta) {{
    var metaLines = cached.meta.split('\n');
    html += '<details style="margin-bottom:12px"><summary style="cursor:pointer;font-size:13px;color:#666">Show comments (' + metaLines.length + ' lines)</summary>';
    html += '<pre style="font-size:11px;color:#888;margin-top:4px;max-height:200px;overflow:auto">' + cached.meta.replace(/</g,'&lt;') + '</pre></details>';
  }}
  html += renderServerPaginatedTable('gffDiv', name, colNames, data.rows || [], data.total || 0, page, function(rec) {{
    var r = '<tr>';
    rec.forEach(function(v, i) {{
      r += (i===8) ? '<td style="white-space:normal;max-width:400px;word-break:break-all;font-size:11px">'+(v||'').replace(/;/g,'; ')+'</td>' : '<td>'+(v||'')+'</td>';
    }});
    return r + '</tr>';
  }});
  div.innerHTML = html;
}}

// ── FASTA Viewer ──
async function renderFastaViewer(name, content) {{
  content.innerHTML = '<div class="fasta-viewer" id="fastaDiv">Loading FASTA...</div>';
  try {{
    var resp = await fetch('/file/' + encodeURIComponent(name));
    var text = await resp.text();
    var div = document.getElementById('fastaDiv'); if (!div) return;
    var lines = text.split('\n');
    var seqCount = 0, totalBp = 0, html = '';
    lines.forEach(function(line) {{
      if (line.startsWith('>')) {{ seqCount++; html += '<div class="seq-header">' + line.replace(/</g,'&lt;') + '</div>'; }}
      else if (line.trim()) {{ totalBp += line.trim().length; html += '<div class="seq-line">' + colorBases(line.trim()) + '</div>'; }}
    }});
    div.innerHTML = '<p class="meta" style="margin-bottom:12px">' + seqCount + ' sequence(s) &middot; ' + totalBp.toLocaleString() + ' bp total</p>' + html;
  }} catch(e) {{
    document.getElementById('fastaDiv').innerHTML = 'Error: ' + e.message;
  }}
}}

// ── BAM Viewer (server-side) ──
async function renderBamViewer(name, content) {{
  content.innerHTML = '<div class="genomics-viewer" id="bamDiv">Loading BAM...</div>';
  try {{ await _fetchAndRenderBam(name, 0); }}
  catch(e) {{
    document.getElementById('bamDiv').innerHTML =
      '<div class="no-preview"><p class="no-preview-icon">⚠️</p><p class="no-preview-title">BAM Load Error</p><p class="no-preview-msg">' + e.message + '</p></div>';
  }}
}}
async function _fetchAndRenderBam(name, page) {{
  var div = document.getElementById('bamDiv'); if (!div) return;
  var data = await fetchGenomicsPage(name, 0);
  if (data.error) {{ div.innerHTML = 'Error: ' + data.error; return; }}
  _genomicsMetaCache[name] = {{ refs: data.refs || [], header: data.header || '', total: data.total || 0 }};
  var cached = _genomicsMetaCache[name];
  var rowCount = (data.rows || []).length;
  var html = '<p class="meta">' + (cached.refs||[]).length + ' reference(s) &middot; First ' + rowCount + ' read(s)</p>';
  if ((cached.refs||[]).length > 0) {{
    html += '<details style="margin-bottom:12px" open><summary style="cursor:pointer;font-size:13px;font-weight:600">References</summary><table><tr><th>Name</th><th>Length</th></tr>';
    cached.refs.forEach(function(r) {{ html += '<tr><td>'+r.name+'</td><td>'+r.length.toLocaleString()+' bp</td></tr>'; }});
    html += '</table></details>';
  }}
  if (cached.header) {{
    html += '<details style="margin-bottom:12px"><summary style="cursor:pointer;font-size:13px;color:#666">SAM header</summary>';
    html += '<pre style="font-size:11px;color:#888;margin-top:4px;max-height:200px;overflow:auto">' + cached.header.replace(/</g,'&lt;') + '</pre></details>';
  }}
  var bamHdrs = ['Read Name','Flag','Chr','Pos','MAPQ','CIGAR','Sequence'];
  html += '<table><tr>';
  bamHdrs.forEach(function(h) {{ html += '<th>' + h + '</th>'; }});
  html += '</tr>';
  (data.rows || []).forEach(function(row) {{
    var qname = row[0]||'*', flag = row[1]||'0', rname = row[2]||'*', pos = row[3]||'0', mapq = row[4]||'0', cigar = row[5]||'*';
    var seq = row.length > 9 ? row[9] : '*';
    html += '<tr><td>'+qname+'</td><td>'+flag+'</td><td>'+rname+'</td><td>'+pos+'</td><td>'+mapq+'</td><td>'+cigar+'</td><td class="seq">'+colorBases(seq)+'</td></tr>';
  }});
  html += '</table>';
  div.innerHTML = html;
}}

// ── Color bases helper ──
function colorBases(seq) {{
  return seq.replace(/[ATCGN]/gi, function(b) {{
    var u = b.toUpperCase();
    if (u==='A') return '<span class="base-A">'+b+'</span>';
    if (u==='T') return '<span class="base-T">'+b+'</span>';
    if (u==='C') return '<span class="base-C">'+b+'</span>';
    if (u==='G') return '<span class="base-G">'+b+'</span>';
    return b;
  }});
}}

// ── IGV.js Viewer (dual-tab or CRAM/BCF) ──
var igvLoaded = false;
function loadIgv() {{
  return new Promise(function(resolve, reject) {{
    if (igvLoaded) {{ resolve(); return; }}
    var s = document.createElement('script');
    s.src = 'https://cdn.jsdelivr.net/npm/igv@3/dist/igv.min.js';
    s.onload = function() {{ igvLoaded = true; resolve(); }};
    s.onerror = function() {{ reject(new Error('Failed to load igv.js')); }};
    document.head.appendChild(s);
  }});
}}

async function renderIgvViewer(name, ext, content) {{
  content.innerHTML = '<div class="igv-viewer" id="igvDiv">Loading IGV.js...</div>';
  try {{
    await loadIgv();
    var div = document.getElementById('igvDiv'); if (!div) return;
    div.innerHTML = '';

    var fileUrl = '/file/' + encodeURIComponent(name);
    var trackType = 'annotation';
    var trackFormat = ext;
    if (ext === 'bam' || ext === 'cram') {{ trackType = 'alignment'; }}
    else if (ext === 'vcf' || ext === 'bcf') {{ trackType = 'variant'; trackFormat = 'vcf'; }}
    else if (ext === 'bed') {{ trackType = 'annotation'; trackFormat = 'bed'; }}
    else if (ext === 'gff' || ext === 'gtf' || ext === 'gff3') {{ trackType = 'annotation'; }}

    var opts = {{}};
    // Use dropdown selection if available, otherwise fall back to REFERENCE
    var activeRef = selectedGenome || REFERENCE;
    var isLocalFasta = activeRef && FILES.some(function(f) {{ return f.name === activeRef; }});
    var knownIds = KNOWN_GENOMES.map(function(g) {{ return g.id; }});
    var isKnownGenome = activeRef && knownIds.indexOf(activeRef) >= 0;

    if (ext === 'fasta' || ext === 'fa') {{
      // FASTA is the reference itself
      opts.reference = {{ fastaURL: fileUrl, indexed: false }};
    }} else if (isLocalFasta) {{
      opts.reference = {{ fastaURL: '/file/' + encodeURIComponent(activeRef), indexed: false }};
      opts.tracks = [{{ type: trackType, format: trackFormat, url: fileUrl, name: name }}];
    }} else if (isKnownGenome || activeRef) {{
      opts.genome = activeRef;
      opts.tracks = [{{ type: trackType, format: trackFormat, url: fileUrl, name: name }}];
    }} else {{
      throw new Error('No reference genome available');
    }}

    igv.createBrowser(div, opts);
  }} catch(e) {{
    content.innerHTML = '<div class="no-preview"><p class="no-preview-icon">⚠️</p>' +
      '<p class="no-preview-title">IGV.js Error</p>' +
      '<p class="no-preview-msg">' + e.message + '<br><br>Provide a reference genome path or ID (e.g., hg38) when calling show_results.</p>' +
      '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a></div>';
  }}
}}

// ── HDF5 (h5ad) Viewer — tree layout with jsfive ──
var _hdf5FileCache = {{}};
var _hdf5TreeState = {{}};
var _hdf5ValueCache = {{}};
var _hdf5ValuePage = {{}};
var _hdf5ActiveFile = null;

async function renderHdf5Viewer(name, actions, content) {{
  actions.innerHTML = '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a>';
  content.style.padding = '0';
  content.style.overflow = 'hidden';
  content.innerHTML = '<div class="hdf5-tree-layout"><div class="hdf5-tree" id="hdf5Tree">Loading HDF5...</div><div class="hdf5-detail" id="hdf5Detail"><div class="empty-state">Select an item from the tree</div></div></div>';
  _hdf5ActiveFile = name;

  try {{
    if (!_hdf5FileCache[name]) {{
      var resp = await fetch('/file/' + encodeURIComponent(name));
      var buf = await resp.arrayBuffer();
      _hdf5FileCache[name] = new hdf5.File(buf);
    }}
    if (!_hdf5TreeState[name]) _hdf5TreeState[name] = {{}};
    _buildHdf5Tree(name);
  }} catch(e) {{
    content.style.padding = '20px';
    content.style.overflow = 'auto';
    content.innerHTML = '<div class="no-preview"><p class="no-preview-icon">&#x26A0;&#xFE0F;</p>' +
      '<p class="no-preview-title">HDF5 Load Error</p>' +
      '<p class="no-preview-msg">' + e.message + '<br><br>Download and inspect with Python:<br><code>import anndata; ad = anndata.read_h5ad("' + name + '")</code></p>' +
      '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a></div>';
  }}
}}

function _buildHdf5Tree(name) {{
  var f = _hdf5FileCache[name]; if (!f) return;
  var tree = document.getElementById('hdf5Tree'); if (!tree) return;
  var state = _hdf5TreeState[name];
  var html = '';

  function renderNode(group, path, depth) {{
    var keys;
    try {{ keys = group.keys || []; }} catch(e) {{ return; }}
    for (var i = 0; i < keys.length; i++) {{
      var key = keys[i];
      var fullPath = path ? path + '/' + key : key;
      try {{
        var obj = group.get(key);
        var esc = fullPath.replace(/\\/g,'\\\\').replace(/'/g,"\\'");
        if (obj && obj.keys !== undefined && !obj.shape) {{
          // Group node
          var isOpen = !!state[fullPath];
          var arrow = isOpen ? '\u25BC' : '\u25B6';
          html += '<div class="hdf5-tree-item group" data-path="' + fullPath + '" style="padding-left:' + (12 + depth * 16) + 'px" onclick="event.stopPropagation();_hdf5ToggleGroup(\'' + esc + '\')">';
          html += '<span class="hdf5-tree-toggle">' + arrow + '</span> ' + key;
          html += '</div>';
          if (isOpen) renderNode(obj, fullPath, depth + 1);
        }} else if (obj && obj.shape) {{
          // Dataset node
          html += '<div class="hdf5-tree-item" data-path="' + fullPath + '" style="padding-left:' + (12 + depth * 16) + 'px" onclick="_hdf5ShowItem(\'' + esc + '\')">';
          html += key + ' <span style="color:#999;font-size:11px">' + JSON.stringify(obj.shape) + '</span>';
          html += '</div>';
        }}
      }} catch(e) {{
        html += '<div class="hdf5-tree-item" style="padding-left:' + (12 + depth * 16) + 'px;color:#c00">' + key + ' (error)</div>';
      }}
    }}
  }}

  renderNode(f, '', 0);
  tree.innerHTML = html || '<div style="padding:16px;color:#999">Empty file</div>';
}}

window._hdf5ToggleGroup = function(path) {{
  var name = _hdf5ActiveFile; if (!name) return;
  var state = _hdf5TreeState[name];
  state[path] = !state[path];
  _buildHdf5Tree(name);
  _hdf5ShowItem(path);
}};

window._hdf5ShowItem = function(path) {{
  var name = _hdf5ActiveFile; if (!name) return;
  var f = _hdf5FileCache[name]; if (!f) return;
  var detail = document.getElementById('hdf5Detail'); if (!detail) return;

  // Highlight active item
  document.querySelectorAll('.hdf5-tree-item').forEach(function(el) {{
    el.classList.toggle('active', el.dataset.path === path);
  }});

  try {{
    var obj = f.get(path);
    if (!obj) {{ detail.innerHTML = '<p style="color:#888">Item not found</p>'; return; }}

    if (obj.keys !== undefined && !obj.shape) {{
      // Group: show children summary
      var html = '<h3 style="font-size:14px;font-weight:600;margin-bottom:12px">' + path + '/</h3>';
      html += '<div class="genomics-viewer"><table><tr><th>Key</th><th>Type</th><th>Shape</th><th>Dtype</th></tr>';
      var keys = obj.keys || [];
      keys.forEach(function(k) {{
        try {{
          var child = obj.get(k);
          if (child && child.keys !== undefined && !child.shape) {{
            html += '<tr><td>' + k + '/</td><td>Group</td><td>-</td><td>-</td></tr>';
          }} else if (child && child.shape) {{
            html += '<tr><td>' + k + '</td><td>Dataset</td><td>' + JSON.stringify(child.shape) + '</td><td>' + (child.dtype || '-') + '</td></tr>';
          }}
        }} catch(e) {{
          html += '<tr><td>' + k + '</td><td colspan="3" style="color:#c00">' + e + '</td></tr>';
        }}
      }});
      html += '</table></div>';
      detail.innerHTML = html;
      return;
    }}

    // Dataset: show shape/dtype and optionally values
    var shape = obj.shape || [];
    var dtype = String(obj.dtype || '-');
    var totalElements = 1;
    for (var s = 0; s < shape.length; s++) totalElements *= shape[s];

    var html = '<h3 style="font-size:14px;font-weight:600;margin-bottom:12px">' + path + '</h3>';
    html += '<div style="margin-bottom:12px;font-size:12px;color:#666">';
    html += 'Shape: ' + JSON.stringify(shape) + ' &middot; Dtype: ' + dtype + ' &middot; Elements: ' + totalElements.toLocaleString();
    html += '</div>';

    if (totalElements > 1000000) {{
      html += '<p style="color:#888;font-size:13px">Dataset too large to preview (' + totalElements.toLocaleString() + ' elements).<br>Download the file and inspect with Python.</p>';
      detail.innerHTML = html;
      return;
    }}

    // Load value (cached)
    var cacheKey = name + ':' + path;
    var value;
    if (_hdf5ValueCache[cacheKey] !== undefined) {{
      value = _hdf5ValueCache[cacheKey];
    }} else {{
      try {{
        value = obj.value;
        _hdf5ValueCache[cacheKey] = value;
      }} catch(e) {{
        html += '<p style="color:#c00;font-size:13px">Error reading value: ' + e + '</p>';
        detail.innerHTML = html;
        return;
      }}
    }}

    var page = _hdf5ValuePage[path] || 0;
    if (shape.length <= 1) {{
      html += _renderHdf5Values1D(value, path, page);
    }} else if (shape.length === 2) {{
      html += _renderHdf5Values2D(value, shape, path, page);
    }} else {{
      html += '<pre style="font-size:12px;max-height:400px;overflow:auto">' + String(value).substring(0, 5000) + '</pre>';
    }}

    detail.innerHTML = html;
  }} catch(e) {{
    detail.innerHTML = '<p style="color:#c00">Error: ' + e + '</p>';
  }}
}};

function _renderHdf5Values1D(value, path, page) {{
  var arr = Array.isArray(value) ? value : (value && value.length ? Array.from(value) : [value]);
  var pageSize = 100;
  var totalPages = Math.ceil(arr.length / pageSize) || 1;
  if (page >= totalPages) page = totalPages - 1;
  if (page < 0) page = 0;
  var start = page * pageSize;
  var pageArr = arr.slice(start, start + pageSize);
  var esc = path.replace(/\\/g,'\\\\').replace(/'/g,"\\'");

  var html = '<div class="genomics-viewer"><table><tr><th>Index</th><th>Value</th></tr>';
  pageArr.forEach(function(v, i) {{
    if (typeof v === 'number' && !Number.isInteger(v)) v = v.toFixed(4);
    html += '<tr><td>' + (start + i) + '</td><td>' + v + '</td></tr>';
  }});
  html += '</table>';
  if (totalPages > 1) {{
    html += '<div class="pagination">';
    html += '<button onclick="_hdf5PageVal(\'' + esc + '\',' + (page-1) + ')"' + (page <= 0 ? ' disabled' : '') + '>&laquo; Prev</button>';
    html += '<span>Page ' + (page+1) + ' / ' + totalPages + ' (' + arr.length.toLocaleString() + ' values)</span>';
    html += '<button onclick="_hdf5PageVal(\'' + esc + '\',' + (page+1) + ')"' + (page >= totalPages-1 ? ' disabled' : '') + '>Next &raquo;</button>';
    html += '</div>';
  }}
  html += '</div>';
  return html;
}}

function _renderHdf5Values2D(value, shape, path, page) {{
  var nRows = shape[0], nCols = shape[1];
  var maxCols = 20;
  var pageSize = 100;
  var totalPages = Math.ceil(nRows / pageSize) || 1;
  if (page >= totalPages) page = totalPages - 1;
  if (page < 0) page = 0;
  var start = page * pageSize;
  var endRow = Math.min(start + pageSize, nRows);
  var esc = path.replace(/\\/g,'\\\\').replace(/'/g,"\\'");

  var html = '<div class="genomics-viewer"><table><tr><th>#</th>';
  for (var c = 0; c < Math.min(nCols, maxCols); c++) html += '<th>' + c + '</th>';
  if (nCols > maxCols) html += '<th>...</th>';
  html += '</tr>';
  for (var r = start; r < endRow; r++) {{
    html += '<tr><td>' + r + '</td>';
    for (var c = 0; c < Math.min(nCols, maxCols); c++) {{
      var v;
      if (Array.isArray(value)) {{ v = value[r * nCols + c]; }}
      else if (value && value[r * nCols + c] !== undefined) {{ v = value[r * nCols + c]; }}
      else {{ v = '-'; }}
      if (typeof v === 'number' && !Number.isInteger(v)) v = v.toFixed(4);
      html += '<td>' + v + '</td>';
    }}
    if (nCols > maxCols) html += '<td>...</td>';
    html += '</tr>';
  }}
  html += '</table>';
  if (totalPages > 1) {{
    html += '<div class="pagination">';
    html += '<button onclick="_hdf5PageVal(\'' + esc + '\',' + (page-1) + ')"' + (page <= 0 ? ' disabled' : '') + '>&laquo; Prev</button>';
    html += '<span>Page ' + (page+1) + ' / ' + totalPages + ' (' + nRows.toLocaleString() + ' rows)</span>';
    html += '<button onclick="_hdf5PageVal(\'' + esc + '\',' + (page+1) + ')"' + (page >= totalPages-1 ? ' disabled' : '') + '>Next &raquo;</button>';
    html += '</div>';
  }}
  html += '</div>';
  return html;
}}

window._hdf5PageVal = function(path, page) {{
  if (page < 0) return;
  _hdf5ValuePage[path] = page;
  _hdf5ShowItem(path);
}};

// ── Plugin Viewer ──
function findPlugin(ext) {{
  for (var i = 0; i < PLUGINS.length; i++) {{
    if (PLUGINS[i].extensions.indexOf(ext) >= 0) return PLUGINS[i];
  }}
  return null;
}}

async function renderPluginViewer(name, plugin, actions, content) {{
  actions.innerHTML = '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a>';
  content.innerHTML = '<div id="pluginContainer">Loading plugin ' + plugin.name + '...</div>';

  try {{
    // Load plugin CSS if specified
    if (plugin.style && !loadedPlugins[plugin.name + '_css']) {{
      var link = document.createElement('link');
      link.rel = 'stylesheet';
      link.href = '/plugin/' + encodeURIComponent(plugin.name) + '/' + plugin.style;
      document.head.appendChild(link);
      loadedPlugins[plugin.name + '_css'] = true;
    }}

    // Load plugin JS
    await new Promise(function(resolve, reject) {{
      if (loadedPlugins[plugin.name + '_js']) {{ resolve(); return; }}
      var s = document.createElement('script');
      s.src = '/plugin/' + encodeURIComponent(plugin.name) + '/' + plugin.entry;
      s.onload = function() {{ loadedPlugins[plugin.name + '_js'] = true; resolve(); }};
      s.onerror = function() {{ reject(new Error('Failed to load plugin JS')); }};
      document.head.appendChild(s);
    }});

    var container = document.getElementById('pluginContainer');
    if (container && window.AutoPipePlugin && window.AutoPipePlugin.render) {{
      container.innerHTML = '';
      window.AutoPipePlugin.render(container, '/file/' + encodeURIComponent(name), name);
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
      '<p class="no-preview-msg">No built-in viewer for .' + ext + ' files.<br>Install a plugin that supports this format to enable preview.</p>' +
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
