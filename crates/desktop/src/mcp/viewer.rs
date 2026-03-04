#![allow(dead_code)]

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Json},
    routing::get,
    Router,
};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

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
pub async fn show_files(
    files: Vec<(String, Vec<u8>, String)>,
    plugins_dir: String,
    reference: Option<String>,
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

/// API: return file list as JSON.
async fn files_list_handler(State(state): State<ViewerState>) -> Json<Vec<FileListItem>> {
    let files = state.files.lock().await;
    let mut items: Vec<FileListItem> = files
        .iter()
        .map(|(name, entry)| FileListItem {
            name: name.clone(),
            mime: entry.mime.clone(),
            size: entry.data.len(),
        })
        .collect();
    items.sort_by(|a, b| a.name.cmp(&b.name));
    Json(items)
}

#[derive(Serialize)]
struct FileListItem {
    name: String,
    mime: String,
    size: usize,
}

/// API: return reference info as JSON.
async fn reference_handler() -> Json<serde_json::Value> {
    let r = get_reference_lock().await.lock().await;
    match &*r {
        Some(ref_val) => Json(serde_json::json!({ "reference": ref_val })),
        None => Json(serde_json::json!({ "reference": null })),
    }
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
<title>AutoPipe Results</title>
<script src="https://cdn.jsdelivr.net/npm/jsfive@0.3.10/dist/browser/hdf5.js"></script>
<style>
  * {{ box-sizing: border-box; margin: 0; padding: 0; }}
  html, body {{ height: 100%; overflow: hidden; }}
  body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Inter', sans-serif; background: #fafafa; color: #111; line-height: 1.5; display: flex; flex-direction: column; }}

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

  .viewer-content {{ flex: 1; overflow: auto; padding: 20px; background: #fafafa; }}

  /* Image viewer */
  .img-viewer {{ overflow: auto; }}
  .img-viewer img {{ max-width: 100%; height: auto; transition: transform 0.15s; transform-origin: top left; }}

  /* Text viewer */
  .text-viewer {{ background: #fff; border: 1px solid #e5e5e5; border-radius: 8px; padding: 16px; font-family: 'SF Mono', 'Fira Code', 'Consolas', monospace; font-size: 13px; line-height: 1.6; white-space: pre-wrap; word-break: break-all; overflow: auto; max-height: 100%; }}

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
  .hdf5-viewer tr:hover td {{ background: #f8f8f8; }}
  .hdf5-section {{ margin-bottom: 20px; }}
  .hdf5-section h3 {{ font-size: 14px; font-weight: 600; margin-bottom: 8px; color: #333; }}

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
  // If it's a known genome ID
  var knownGenomes = ['hg38','hg19','hg18','mm39','mm10','mm9','rn7','rn6','dm6','dm3','ce11','danRer11','sacCer3','tair10'];
  if (knownGenomes.indexOf(REFERENCE) >= 0) return REFERENCE;
  // If it's a file in our list, not a genome ID
  if (FILES.some(function(f) {{ return f.name === REFERENCE; }})) return null;
  // Treat unknown strings as genome IDs
  return REFERENCE;
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

  toolbar.style.display = 'flex';
  title.textContent = name;

  var imageExts = ['png','jpg','jpeg','gif','svg','webp','bmp','tiff','tif'];
  var textExts = ['txt','log','csv','tsv','json','yaml','yml','xml','md','sh','py','r','R','nf','smk','cfg','ini','toml','fastq','fq'];
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
      actions.innerHTML = tabsHtml + '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a>';
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
    actions.innerHTML = '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a>';
    if (hasReference()) {{
      renderIgvViewer(name, ext, content);
    }} else {{
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
  }} else if (textExts.indexOf(ext) >= 0) {{
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
  content.innerHTML = '<div class="text-viewer" id="textContent">Loading...</div>';
  try {{
    var resp = await fetch('/file/' + encodeURIComponent(name));
    var text = await resp.text();
    document.getElementById('textContent').textContent = text;
  }} catch(e) {{
    document.getElementById('textContent').textContent = 'Error loading file: ' + e.message;
  }}
}}

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

// ── VCF Viewer ──
var _vcfCache = {{}};
async function renderVcfViewer(name, content) {{
  content.innerHTML = '<div class="genomics-viewer" id="vcfDiv">Loading VCF...</div>';
  try {{
    if (!_vcfCache[name]) {{
      var resp = await fetch('/file/' + encodeURIComponent(name));
      var text = await resp.text();
      var meta = [], hdr = [], recs = [];
      text.split('\n').forEach(function(l) {{
        if (l.startsWith('##')) meta.push(l);
        else if (l.startsWith('#CHROM')) hdr = l.substring(1).split('\t');
        else if (l.trim()) recs.push(l.split('\t'));
      }});
      _vcfCache[name] = {{ meta: meta, hdr: hdr, recs: recs }};
    }}
    _renderVcfPage(name, 0);
  }} catch(e) {{
    document.getElementById('vcfDiv').innerHTML = 'Error: ' + e.message;
  }}
}}
function _renderVcfPage(name, page) {{
  var c = _vcfCache[name]; if (!c) return;
  var div = document.getElementById('vcfDiv'); if (!div) return;
  var html = '<p class="meta">' + c.recs.length + ' variant(s) &middot; ' + c.meta.length + ' metadata lines</p>';
  if (c.meta.length > 0) {{
    html += '<details style="margin-bottom:12px"><summary style="cursor:pointer;font-size:13px;color:#666">Show metadata (' + c.meta.length + ' lines)</summary>';
    html += '<pre style="font-size:11px;color:#888;margin-top:4px;max-height:200px;overflow:auto">' + c.meta.join('\n').replace(/</g,'&lt;') + '</pre></details>';
  }}
  html += renderPaginatedTable('vcfDiv', c.hdr, c.recs, page, function(rec) {{
    var r = '<tr>';
    rec.forEach(function(val, i) {{
      r += (c.hdr[i]==='REF'||c.hdr[i]==='ALT') ? '<td class="seq">'+colorBases(val)+'</td>' : '<td>'+val+'</td>';
    }});
    return r + '</tr>';
  }});
  div.innerHTML = html;
  window._paginate = function(id, p) {{ if (id==='vcfDiv') _renderVcfPage(name, p); else if (id==='bedDiv') _renderBedPage(name, p); else if (id==='gffDiv') _renderGffPage(name, p); else if (id==='bamDiv') _renderBamPage(name, p); else if (id==='hdf5Div') _renderHdf5Page(name, p); }};
}}

// ── BED Viewer ──
var _bedCache = {{}};
async function renderBedViewer(name, content) {{
  content.innerHTML = '<div class="genomics-viewer" id="bedDiv">Loading BED...</div>';
  try {{
    if (!_bedCache[name]) {{
      var resp = await fetch('/file/' + encodeURIComponent(name));
      var text = await resp.text();
      var lines = text.split('\n').filter(function(l) {{ return l.trim() && !l.startsWith('#') && !l.startsWith('track') && !l.startsWith('browser'); }});
      var recs = lines.map(function(l) {{ return l.split('\t'); }});
      var ncols = recs.length > 0 ? recs[0].length : 3;
      _bedCache[name] = {{ recs: recs, ncols: ncols }};
    }}
    _renderBedPage(name, 0);
  }} catch(e) {{
    document.getElementById('bedDiv').innerHTML = 'Error: ' + e.message;
  }}
}}
function _renderBedPage(name, page) {{
  var c = _bedCache[name]; if (!c) return;
  var div = document.getElementById('bedDiv'); if (!div) return;
  var colNames = ['chrom','chromStart','chromEnd','name','score','strand','thickStart','thickEnd','itemRgb','blockCount','blockSizes','blockStarts'];
  var hdrs = colNames.slice(0, Math.min(c.ncols, 12));
  var html = '<p class="meta">' + c.recs.length + ' region(s) &middot; BED' + Math.min(c.ncols,12) + ' format</p>';
  html += renderPaginatedTable('bedDiv', hdrs, c.recs, page, function(rec) {{
    var r = '<tr>'; rec.forEach(function(v) {{ r += '<td>'+v+'</td>'; }}); return r + '</tr>';
  }});
  div.innerHTML = html;
  window._paginate = function(id, p) {{ if (id==='vcfDiv') _renderVcfPage(name, p); else if (id==='bedDiv') _renderBedPage(name, p); else if (id==='gffDiv') _renderGffPage(name, p); else if (id==='bamDiv') _renderBamPage(name, p); else if (id==='hdf5Div') _renderHdf5Page(name, p); }};
}}

// ── GFF/GTF Viewer ──
var _gffCache = {{}};
async function renderGffViewer(name, content) {{
  content.innerHTML = '<div class="genomics-viewer" id="gffDiv">Loading GFF...</div>';
  try {{
    if (!_gffCache[name]) {{
      var resp = await fetch('/file/' + encodeURIComponent(name));
      var text = await resp.text();
      var comments = [], recs = [];
      text.split('\n').forEach(function(l) {{
        if (l.startsWith('#')) comments.push(l);
        else if (l.trim()) recs.push(l.split('\t'));
      }});
      _gffCache[name] = {{ comments: comments, recs: recs }};
    }}
    _renderGffPage(name, 0);
  }} catch(e) {{
    document.getElementById('gffDiv').innerHTML = 'Error: ' + e.message;
  }}
}}
function _renderGffPage(name, page) {{
  var c = _gffCache[name]; if (!c) return;
  var div = document.getElementById('gffDiv'); if (!div) return;
  var colNames = ['seqid','source','type','start','end','score','strand','phase','attributes'];
  var html = '<p class="meta">' + c.recs.length + ' feature(s)</p>';
  if (c.comments.length > 0) {{
    html += '<details style="margin-bottom:12px"><summary style="cursor:pointer;font-size:13px;color:#666">Show comments (' + c.comments.length + ' lines)</summary>';
    html += '<pre style="font-size:11px;color:#888;margin-top:4px;max-height:200px;overflow:auto">' + c.comments.join('\n').replace(/</g,'&lt;') + '</pre></details>';
  }}
  html += renderPaginatedTable('gffDiv', colNames, c.recs, page, function(rec) {{
    var r = '<tr>';
    rec.forEach(function(v, i) {{
      r += (i===8) ? '<td style="white-space:normal;max-width:400px;word-break:break-all;font-size:11px">'+v.replace(/;/g,'; ')+'</td>' : '<td>'+v+'</td>';
    }});
    return r + '</tr>';
  }});
  div.innerHTML = html;
  window._paginate = function(id, p) {{ if (id==='vcfDiv') _renderVcfPage(name, p); else if (id==='bedDiv') _renderBedPage(name, p); else if (id==='gffDiv') _renderGffPage(name, p); else if (id==='bamDiv') _renderBamPage(name, p); else if (id==='hdf5Div') _renderHdf5Page(name, p); }};
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

// ── BAM Viewer ──
var _bamCache = {{}};
async function renderBamViewer(name, content) {{
  content.innerHTML = '<div class="genomics-viewer" id="bamDiv">Loading BAM...</div>';
  try {{
    if (!_bamCache[name]) {{
      var resp = await fetch('/file/' + encodeURIComponent(name));
      var buf = await resp.arrayBuffer();
      var parsed = await parseBam(buf);
      _bamCache[name] = parsed;
    }}
    _renderBamPage(name, 0);
  }} catch(e) {{
    document.getElementById('bamDiv').innerHTML =
      '<div class="no-preview"><p class="no-preview-icon">⚠️</p><p class="no-preview-title">BAM Parse Error</p><p class="no-preview-msg">' + e.message + '</p>' +
      '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a></div>';
  }}
}}
function _renderBamPage(name, page) {{
  var c = _bamCache[name]; if (!c) return;
  var div = document.getElementById('bamDiv'); if (!div) return;
  var html = '<p class="meta">' + c.refs.length + ' reference(s) &middot; ' + c.reads.length + ' read(s)</p>';
  if (c.refs.length > 0) {{
    html += '<details style="margin-bottom:12px" open><summary style="cursor:pointer;font-size:13px;font-weight:600">References</summary><table><tr><th>Name</th><th>Length</th></tr>';
    c.refs.forEach(function(r) {{ html += '<tr><td>'+r.name+'</td><td>'+r.length.toLocaleString()+' bp</td></tr>'; }});
    html += '</table></details>';
  }}
  if (c.header) {{
    html += '<details style="margin-bottom:12px"><summary style="cursor:pointer;font-size:13px;color:#666">SAM header</summary>';
    html += '<pre style="font-size:11px;color:#888;margin-top:4px;max-height:200px;overflow:auto">' + c.header.replace(/</g,'&lt;') + '</pre></details>';
  }}
  var bamHdrs = ['Read Name','Flag','Chr','Pos','MAPQ','CIGAR','Sequence'];
  html += renderPaginatedTable('bamDiv', bamHdrs, c.reads, page, function(rd) {{
    return '<tr><td>'+rd.name+'</td><td>'+rd.flag+'</td><td>'+rd.chr+'</td><td>'+rd.pos+'</td><td>'+rd.mapq+'</td><td>'+rd.cigar+'</td><td class="seq">'+colorBases(rd.seq)+'</td></tr>';
  }});
  div.innerHTML = html;
  window._paginate = function(id, p) {{ if (id==='vcfDiv') _renderVcfPage(name, p); else if (id==='bedDiv') _renderBedPage(name, p); else if (id==='gffDiv') _renderGffPage(name, p); else if (id==='bamDiv') _renderBamPage(name, p); else if (id==='hdf5Div') _renderHdf5Page(name, p); }};
}}

async function parseBam(buf) {{
  var data = new Uint8Array(buf);
  var decompressed = [];
  var offset = 0;
  while (offset < data.length) {{
    if (offset + 18 > data.length) break;
    if (data[offset] !== 0x1f || data[offset+1] !== 0x8b) break;
    var bsize = data[offset+16] | (data[offset+17] << 8);
    var blockEnd = offset + bsize + 1;
    if (blockEnd > data.length) break;
    try {{ var inflated = await asyncInflate(data.slice(offset+18, blockEnd-8)); decompressed.push(inflated); }} catch(e) {{ break; }}
    offset = blockEnd;
    var tl = 0; decompressed.forEach(function(d){{ tl += d.length; }}); if (tl > 4*1024*1024) break;
  }}
  var ts = 0; decompressed.forEach(function(d){{ ts += d.length; }});
  var raw = new Uint8Array(ts); var p = 0;
  decompressed.forEach(function(d) {{ raw.set(d, p); p += d.length; }});
  var view = new DataView(raw.buffer);
  if (raw[0]!==66||raw[1]!==65||raw[2]!==77||raw[3]!==1) throw new Error('Not a valid BAM file');
  var hLen = view.getInt32(4,true);
  var header = new TextDecoder().decode(raw.slice(8, 8+hLen));
  var ro = 8+hLen;
  var nRef = view.getInt32(ro,true); ro += 4;
  var refs = [];
  for (var r=0; r<nRef; r++) {{
    var nl = view.getInt32(ro,true); ro += 4;
    var rn = new TextDecoder().decode(raw.slice(ro, ro+nl-1)); ro += nl;
    var rl = view.getInt32(ro,true); ro += 4;
    refs.push({{ name: rn, length: rl }});
  }}
  var reads = [], maxR = 2000, seqLU = 'NACMGRSVTWYHKDBN';
  while (ro + 4 < raw.length && reads.length < maxR) {{
    var bs = view.getInt32(ro,true);
    if (bs<=0 || ro+4+bs>raw.length) break;
    var rs = ro+4;
    var refID=view.getInt32(rs,true), pos2=view.getInt32(rs+4,true), nl2=raw[rs+8], mq=raw[rs+9];
    var nCig=view.getUint16(rs+12,true), fl=view.getUint16(rs+14,true), sl=view.getInt32(rs+16,true);
    var rName=new TextDecoder().decode(raw.slice(rs+32, rs+32+nl2-1));
    var co=rs+32+nl2, cig='', cops='MIDNSHP=X';
    for (var c2=0;c2<nCig;c2++) {{ var cv=view.getUint32(co+c2*4,true); cig += (cv>>4)+cops[cv&0xf]; }}
    var so=co+nCig*4, sq='';
    for (var s2=0;s2<sl;s2++) {{ var b2=raw[so+(s2>>1)]; sq += seqLU[(s2&1)?(b2&0x0f):((b2>>4)&0x0f)]; }}
    reads.push({{ name:rName, flag:fl, chr:(refID>=0&&refID<refs.length)?refs[refID].name:'*', pos:pos2+1, mapq:mq, cigar:cig||'*', seq:sq }});
    ro += 4+bs;
  }}
  return {{ refs:refs, reads:reads, header:header }};
}}

// ── BGZF decompression ──
async function asyncInflate(data) {{
  var ds = new DecompressionStream('deflate-raw');
  var writer = ds.writable.getWriter();
  writer.write(data); writer.close();
  var reader = ds.readable.getReader();
  var chunks = [];
  while (true) {{ var r = await reader.read(); if (r.done) break; chunks.push(r.value); }}
  var tl = 0; chunks.forEach(function(c) {{ tl += c.length; }});
  var out = new Uint8Array(tl); var off = 0;
  chunks.forEach(function(c) {{ out.set(c, off); off += c.length; }});
  return out;
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
    var localRef = getIgvReference();
    var genomeId = getIgvGenomeId();

    if (ext === 'fasta' || ext === 'fa') {{
      // FASTA is the reference itself
      opts.reference = {{ fastaURL: fileUrl, indexed: false }};
    }} else if (localRef) {{
      opts.reference = localRef;
      opts.tracks = [{{ type: trackType, format: trackFormat, url: fileUrl, name: name }}];
    }} else if (genomeId) {{
      opts.genome = genomeId;
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

// ── HDF5 (h5ad) Viewer — browser-side jsfive ──
var _hdf5Cache = {{}};
async function renderHdf5Viewer(name, actions, content) {{
  actions.innerHTML = '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a>';

  content.innerHTML = '<div class="hdf5-viewer" id="hdf5Div">Loading HDF5 file...</div>';

  try {{
    if (!_hdf5Cache[name]) {{
      var resp = await fetch('/file/' + encodeURIComponent(name));
      var buf = await resp.arrayBuffer();
      var f = new jsfive.File(buf);

      // Walk file structure (depth <= 3)
      var items = [];
      function walkHdf5(group, prefix, depth) {{
        if (depth > 3) return;
        var keys = group.keys || [];
        for (var i = 0; i < Math.min(keys.length, 200); i++) {{
          var key = keys[i];
          var full = prefix ? prefix + '/' + key : key;
          try {{
            var obj = group.get(key);
            if (obj && obj.shape) {{
              items.push({{key: full, type: 'Dataset', shape: JSON.stringify(obj.shape), dtype: String(obj.dtype || '-')}});
            }} else if (obj && obj.keys) {{
              items.push({{key: full + '/', type: 'Group', shape: '-', dtype: '-'}});
              walkHdf5(obj, full, depth + 1);
            }}
          }} catch(e) {{
            items.push({{key: full, type: 'Error', shape: '-', dtype: String(e)}});
          }}
        }}
      }}
      walkHdf5(f, '', 0);

      // Extract obs/var columns
      var obsItems = [];
      var obs = f.get('obs');
      if (obs && obs.keys) {{
        obs.keys.slice(0, 100).forEach(function(k) {{
          try {{
            var obj = obs.get(k);
            var dtype = (obj && obj.dtype) ? String(obj.dtype) : (obj && obj.keys ? 'categorical' : 'unknown');
            var nCats = 0;
            if (obj && obj.keys && obj.get && obj.get('categories')) {{
              try {{ nCats = obj.get('categories').shape[0]; }} catch(e) {{}}
            }}
            obsItems.push({{name: k, dtype: dtype, n_categories: nCats}});
          }} catch(e) {{
            obsItems.push({{name: k, dtype: 'error', n_categories: 0}});
          }}
        }});
      }}

      var varItems = [];
      var varGrp = f.get('var');
      if (varGrp && varGrp.keys) {{
        varGrp.keys.slice(0, 100).forEach(function(k) {{
          try {{
            var obj = varGrp.get(k);
            var dtype = (obj && obj.dtype) ? String(obj.dtype) : (obj && obj.keys ? 'categorical' : 'unknown');
            varItems.push({{name: k, dtype: dtype}});
          }} catch(e) {{
            varItems.push({{name: k, dtype: 'error'}});
          }}
        }});
      }}

      // Dimensions
      var nObs = 0, nVar = 0;
      var X = f.get('X');
      if (X && X.shape) {{
        nObs = X.shape[0] || 0;
        nVar = X.shape[1] || 0;
      }}

      // Obsm keys
      var obsmKeys = [];
      var obsm = f.get('obsm');
      if (obsm && obsm.keys) {{
        obsm.keys.forEach(function(k) {{
          try {{
            var obj = obsm.get(k);
            obsmKeys.push({{key: k, shape: obj && obj.shape ? JSON.stringify(obj.shape) : '-'}});
          }} catch(e) {{
            obsmKeys.push({{key: k, shape: '-'}});
          }}
        }});
      }}

      // Uns keys
      var unsKeys = [];
      var uns = f.get('uns');
      if (uns && uns.keys) {{ unsKeys = uns.keys.slice(0, 100); }}

      _hdf5Cache[name] = {{
        items: items,
        obsItems: obsItems,
        varItems: varItems,
        obsmKeys: obsmKeys,
        unsKeys: unsKeys,
        nObs: nObs,
        nVar: nVar,
        fileSize: buf.byteLength
      }};
    }}
    _renderHdf5Page(name, 0);
  }} catch(e) {{
    content.innerHTML = '<div class="no-preview"><p class="no-preview-icon">&#x26A0;&#xFE0F;</p>' +
      '<p class="no-preview-title">HDF5 Load Error</p>' +
      '<p class="no-preview-msg">' + e.message + '<br><br>Download and inspect with Python:<br><code>import anndata; ad = anndata.read_h5ad("' + name + '")</code></p>' +
      '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a></div>';
  }}
}}
function _renderHdf5Page(name, page) {{
  var c = _hdf5Cache[name]; if (!c) return;
  var div = document.getElementById('hdf5Div'); if (!div) return;
  var html = '';
  var sizeMB = (c.fileSize / 1048576).toFixed(1);

  html += '<div class="hdf5-section"><h3>Summary</h3><table>';
  html += '<tr><th>Property</th><th>Value</th></tr>';
  html += '<tr><td>File size</td><td>' + sizeMB + ' MB</td></tr>';
  html += '<tr><td>Observations (n_obs)</td><td>' + c.nObs.toLocaleString() + '</td></tr>';
  html += '<tr><td>Variables (n_var)</td><td>' + c.nVar.toLocaleString() + '</td></tr>';
  html += '</table></div>';

  html += '<div class="hdf5-section"><h3>File Structure (' + c.items.length + ' entries)</h3>';
  html += renderPaginatedTable('hdf5Div', ['Key', 'Type', 'Shape', 'Dtype'], c.items, page, function(item) {{
    return '<tr><td>' + item.key + '</td><td>' + item.type + '</td><td>' + item.shape + '</td><td>' + item.dtype + '</td></tr>';
  }});
  html += '</div>';

  if (c.obsItems.length > 0) {{
    html += '<div class="hdf5-section"><h3>Observations (obs) columns (' + c.obsItems.length + ')</h3><table>';
    html += '<tr><th>Column</th><th>Dtype</th><th>Categories</th></tr>';
    c.obsItems.forEach(function(item) {{
      var cats = item.n_categories > 0 ? item.n_categories : '-';
      html += '<tr><td>' + item.name + '</td><td>' + item.dtype + '</td><td>' + cats + '</td></tr>';
    }});
    html += '</table></div>';
  }}

  if (c.varItems.length > 0) {{
    html += '<div class="hdf5-section"><h3>Variables (var) columns (' + c.varItems.length + ')</h3><table>';
    html += '<tr><th>Column</th><th>Dtype</th></tr>';
    c.varItems.forEach(function(item) {{
      html += '<tr><td>' + item.name + '</td><td>' + item.dtype + '</td></tr>';
    }});
    html += '</table></div>';
  }}

  if (c.obsmKeys.length > 0) {{
    html += '<div class="hdf5-section"><h3>Embeddings (obsm) (' + c.obsmKeys.length + ')</h3><table>';
    html += '<tr><th>Key</th><th>Shape</th></tr>';
    c.obsmKeys.forEach(function(item) {{
      html += '<tr><td>' + item.key + '</td><td>' + item.shape + '</td></tr>';
    }});
    html += '</table></div>';
  }}

  if (c.unsKeys.length > 0) {{
    html += '<div class="hdf5-section"><h3>Unstructured (uns) (' + c.unsKeys.length + ')</h3><table>';
    html += '<tr><th>Key</th></tr>';
    c.unsKeys.forEach(function(k) {{
      html += '<tr><td>' + k + '</td></tr>';
    }});
    html += '</table></div>';
  }}

  div.innerHTML = html;
  window._paginate = function(id, p) {{ if (id==='vcfDiv') _renderVcfPage(name, p); else if (id==='bedDiv') _renderBedPage(name, p); else if (id==='gffDiv') _renderGffPage(name, p); else if (id==='bamDiv') _renderBamPage(name, p); else if (id==='hdf5Div') _renderHdf5Page(name, p); }};
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
) -> impl IntoResponse {
    let files = state.files.lock().await;
    match files.get(&filename) {
        Some(entry) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, entry.mime.clone())],
            entry.data.clone(),
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "File not found").into_response(),
    }
}
