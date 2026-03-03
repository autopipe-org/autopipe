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
pub async fn show_files(
    files: Vec<(String, Vec<u8>, String)>,
    plugins_dir: String,
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

  /* IGV viewer (CRAM/BCF only) */
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
var currentFile = null;
var currentScale = 1;
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

// Load file list
async function loadFiles() {{
  var resp = await fetch('/api/files');
  FILES = await resp.json();
  renderSidebar();
  if (FILES.length > 0) selectFile(FILES[0].name);
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

function selectFile(name) {{
  currentFile = name;
  currentScale = 1;

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

  // Determine viewer type
  var imageExts = ['png','jpg','jpeg','gif','svg','webp','bmp','tiff','tif'];
  var textExts = ['txt','log','csv','tsv','json','yaml','yml','xml','md','sh','py','r','R','nf','smk','cfg','ini','toml','fastq','fq'];
  var igvExts = ['cram','bcf'];
  var vcfExts = ['vcf'];
  var bedExts = ['bed'];
  var gffExts = ['gff','gtf','gff3'];
  var fastaExts = ['fasta','fa'];
  var bamExts = ['bam'];
  var hdf5Exts = ['h5ad','h5','hdf5'];

  if (imageExts.indexOf(ext) >= 0) {{
    renderImageViewer(name, actions, content);
  }} else if (ext === 'pdf') {{
    renderPdfViewer(name, actions, content);
  }} else if (textExts.indexOf(ext) >= 0) {{
    renderTextViewer(name, actions, content);
  }} else if (vcfExts.indexOf(ext) >= 0) {{
    renderVcfViewer(name, actions, content);
  }} else if (bedExts.indexOf(ext) >= 0) {{
    renderBedViewer(name, actions, content);
  }} else if (gffExts.indexOf(ext) >= 0) {{
    renderGffViewer(name, actions, content);
  }} else if (fastaExts.indexOf(ext) >= 0) {{
    renderFastaViewer(name, actions, content);
  }} else if (bamExts.indexOf(ext) >= 0) {{
    renderBamViewer(name, actions, content);
  }} else if (igvExts.indexOf(ext) >= 0) {{
    renderIgvViewer(name, ext, actions, content);
  }} else if (hdf5Exts.indexOf(ext) >= 0) {{
    renderHdf5Viewer(name, actions, content);
  }} else {{
    // Check plugins
    var plugin = findPlugin(ext);
    if (plugin) {{
      renderPluginViewer(name, plugin, actions, content);
    }} else {{
      renderNoPreview(name, ext, actions, content);
    }}
  }}
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

// ── VCF Viewer (text parse → table) ──
async function renderVcfViewer(name, actions, content) {{
  actions.innerHTML = '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a>';
  content.innerHTML = '<div class="genomics-viewer" id="vcfDiv">Loading VCF...</div>';
  try {{
    var resp = await fetch('/file/' + encodeURIComponent(name));
    var text = await resp.text();
    var lines = text.split('\n');
    var div = document.getElementById('vcfDiv');
    if (!div) return;

    var metaLines = [];
    var headerCols = [];
    var records = [];

    lines.forEach(function(line) {{
      if (line.startsWith('##')) {{
        metaLines.push(line);
      }} else if (line.startsWith('#CHROM')) {{
        headerCols = line.substring(1).split('\t');
      }} else if (line.trim()) {{
        records.push(line.split('\t'));
      }}
    }});

    var html = '<p class="meta">' + records.length + ' variant(s) &middot; ' + metaLines.length + ' metadata lines</p>';

    // Metadata (collapsible)
    if (metaLines.length > 0) {{
      html += '<details style="margin-bottom:12px"><summary style="cursor:pointer;font-size:13px;color:#666">Show metadata (' + metaLines.length + ' lines)</summary>';
      html += '<pre style="font-size:11px;color:#888;margin-top:4px;max-height:200px;overflow:auto">' + metaLines.join('\n').replace(/</g,'&lt;') + '</pre></details>';
    }}

    // Variant table
    html += '<table><tr>';
    headerCols.forEach(function(col) {{ html += '<th>' + col + '</th>'; }});
    html += '</tr>';
    records.forEach(function(rec) {{
      html += '<tr>';
      rec.forEach(function(val, i) {{
        if (headerCols[i] === 'REF' || headerCols[i] === 'ALT') {{
          html += '<td class="seq">' + colorBases(val) + '</td>';
        }} else {{
          html += '<td>' + val + '</td>';
        }}
      }});
      html += '</tr>';
    }});
    html += '</table>';
    div.innerHTML = html;
  }} catch(e) {{
    document.getElementById('vcfDiv').innerHTML = 'Error: ' + e.message;
  }}
}}

// ── BED Viewer (text parse → table) ──
async function renderBedViewer(name, actions, content) {{
  actions.innerHTML = '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a>';
  content.innerHTML = '<div class="genomics-viewer" id="bedDiv">Loading BED...</div>';
  try {{
    var resp = await fetch('/file/' + encodeURIComponent(name));
    var text = await resp.text();
    var lines = text.split('\n').filter(function(l) {{ return l.trim() && !l.startsWith('#') && !l.startsWith('track') && !l.startsWith('browser'); }});
    var div = document.getElementById('bedDiv');
    if (!div) return;

    // Detect BED format (3-12 columns)
    var ncols = lines.length > 0 ? lines[0].split('\t').length : 3;
    var colNames = ['chrom','chromStart','chromEnd','name','score','strand','thickStart','thickEnd','itemRgb','blockCount','blockSizes','blockStarts'];

    var html = '<p class="meta">' + lines.length + ' region(s) &middot; BED' + Math.min(ncols, 12) + ' format</p>';
    html += '<table><tr>';
    for (var i = 0; i < Math.min(ncols, colNames.length); i++) {{
      html += '<th>' + colNames[i] + '</th>';
    }}
    html += '</tr>';
    lines.forEach(function(line) {{
      var cols = line.split('\t');
      html += '<tr>';
      cols.forEach(function(val) {{ html += '<td>' + val + '</td>'; }});
      html += '</tr>';
    }});
    html += '</table>';
    div.innerHTML = html;
  }} catch(e) {{
    document.getElementById('bedDiv').innerHTML = 'Error: ' + e.message;
  }}
}}

// ── GFF/GTF Viewer (text parse → table) ──
async function renderGffViewer(name, actions, content) {{
  actions.innerHTML = '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a>';
  content.innerHTML = '<div class="genomics-viewer" id="gffDiv">Loading GFF...</div>';
  try {{
    var resp = await fetch('/file/' + encodeURIComponent(name));
    var text = await resp.text();
    var lines = text.split('\n');
    var div = document.getElementById('gffDiv');
    if (!div) return;

    var comments = [];
    var records = [];
    lines.forEach(function(line) {{
      if (line.startsWith('#')) comments.push(line);
      else if (line.trim()) records.push(line.split('\t'));
    }});

    var colNames = ['seqid','source','type','start','end','score','strand','phase','attributes'];
    var html = '<p class="meta">' + records.length + ' feature(s)</p>';

    if (comments.length > 0) {{
      html += '<details style="margin-bottom:12px"><summary style="cursor:pointer;font-size:13px;color:#666">Show comments (' + comments.length + ' lines)</summary>';
      html += '<pre style="font-size:11px;color:#888;margin-top:4px;max-height:200px;overflow:auto">' + comments.join('\n').replace(/</g,'&lt;') + '</pre></details>';
    }}

    html += '<table><tr>';
    colNames.forEach(function(col) {{ html += '<th>' + col + '</th>'; }});
    html += '</tr>';
    records.forEach(function(rec) {{
      html += '<tr>';
      rec.forEach(function(val, i) {{
        if (i === 8) {{
          // Parse attributes for readability
          var pretty = val.replace(/;/g, '; ');
          html += '<td style="white-space:normal;max-width:400px;word-break:break-all;font-size:11px">' + pretty + '</td>';
        }} else {{
          html += '<td>' + val + '</td>';
        }}
      }});
      html += '</tr>';
    }});
    html += '</table>';
    div.innerHTML = html;
  }} catch(e) {{
    document.getElementById('gffDiv').innerHTML = 'Error: ' + e.message;
  }}
}}

// ── FASTA Viewer ──
async function renderFastaViewer(name, actions, content) {{
  actions.innerHTML = '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a>';
  content.innerHTML = '<div class="fasta-viewer" id="fastaDiv">Loading FASTA...</div>';
  try {{
    var resp = await fetch('/file/' + encodeURIComponent(name));
    var text = await resp.text();
    var div = document.getElementById('fastaDiv');
    if (!div) return;

    var lines = text.split('\n');
    var seqCount = 0;
    var totalBp = 0;
    var html = '';

    lines.forEach(function(line) {{
      if (line.startsWith('>')) {{
        seqCount++;
        html += '<div class="seq-header">' + line.replace(/</g,'&lt;') + '</div>';
      }} else if (line.trim()) {{
        totalBp += line.trim().length;
        html += '<div class="seq-line">' + colorBases(line.trim()) + '</div>';
      }}
    }});

    var meta = '<p class="meta" style="margin-bottom:12px">' + seqCount + ' sequence(s) &middot; ' + totalBp.toLocaleString() + ' bp total</p>';
    div.innerHTML = meta + html;
  }} catch(e) {{
    document.getElementById('fastaDiv').innerHTML = 'Error: ' + e.message;
  }}
}}

// ── BAM Viewer (parse header + reads as table) ──
async function renderBamViewer(name, actions, content) {{
  actions.innerHTML = '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a>';
  content.innerHTML = '<div class="genomics-viewer" id="bamDiv">Loading BAM...</div>';
  try {{
    var resp = await fetch('/file/' + encodeURIComponent(name));
    var buf = await resp.arrayBuffer();
    var div = document.getElementById('bamDiv');
    if (!div) return;

    // BAM is BGZF compressed. Decompress first block(s) to read header + reads.
    var data = new Uint8Array(buf);
    var decompressed = [];

    // Parse BGZF blocks
    var offset = 0;
    while (offset < data.length) {{
      // BGZF block header: 18 bytes minimum
      if (offset + 18 > data.length) break;
      // Check gzip magic
      if (data[offset] !== 0x1f || data[offset+1] !== 0x8b) break;
      // BSIZE is at offset+16 (2 bytes LE) = total block size - 1
      var bsize = data[offset+16] | (data[offset+17] << 8);
      var blockEnd = offset + bsize + 1;
      if (blockEnd > data.length) break;

      // Compressed data starts at offset+18, ends at blockEnd-8 (8 bytes for CRC32+ISIZE)
      var cdata = data.slice(offset + 18, blockEnd - 8);
      try {{
        var inflated = await asyncInflate(cdata);
        decompressed.push(inflated);
      }} catch(e) {{ break; }}
      offset = blockEnd;
      // Limit: decompress max 2MB to avoid hanging
      var totalLen = 0;
      decompressed.forEach(function(d) {{ totalLen += d.length; }});
      if (totalLen > 2 * 1024 * 1024) break;
    }}

    // Concatenate decompressed data
    var totalSize = 0;
    decompressed.forEach(function(d) {{ totalSize += d.length; }});
    var raw = new Uint8Array(totalSize);
    var pos = 0;
    decompressed.forEach(function(d) {{
      raw.set(d, pos);
      pos += d.length;
    }});

    // Parse BAM header
    var view = new DataView(raw.buffer);
    if (raw[0] !== 66 || raw[1] !== 65 || raw[2] !== 77 || raw[3] !== 1) {{
      throw new Error('Not a valid BAM file');
    }}
    var headerLen = view.getInt32(4, true);
    var headerText = new TextDecoder().decode(raw.slice(8, 8 + headerLen));
    var refOffset = 8 + headerLen;
    var nRef = view.getInt32(refOffset, true);
    refOffset += 4;

    // Parse reference sequences
    var refs = [];
    for (var r = 0; r < nRef; r++) {{
      var nameLen = view.getInt32(refOffset, true);
      refOffset += 4;
      var refName = new TextDecoder().decode(raw.slice(refOffset, refOffset + nameLen - 1));
      refOffset += nameLen;
      var refLen = view.getInt32(refOffset, true);
      refOffset += 4;
      refs.push({{ name: refName, length: refLen }});
    }}

    // Parse alignment records
    var reads = [];
    var readOffset = refOffset;
    var maxReads = 500;
    var seqLookup = 'NACMGRSVTWYHKDBN';

    while (readOffset + 4 < raw.length && reads.length < maxReads) {{
      var blockSize = view.getInt32(readOffset, true);
      if (blockSize <= 0 || readOffset + 4 + blockSize > raw.length) break;
      var rStart = readOffset + 4;

      var refID = view.getInt32(rStart, true);
      var posn = view.getInt32(rStart + 4, true);
      var nameLen2 = raw[rStart + 8];
      var mapq = raw[rStart + 9];
      var nCigarOp = view.getUint16(rStart + 12, true);
      var flag = view.getUint16(rStart + 14, true);
      var seqLen = view.getInt32(rStart + 16, true);

      var readName = new TextDecoder().decode(raw.slice(rStart + 32, rStart + 32 + nameLen2 - 1));

      // Parse CIGAR
      var cigarOff = rStart + 32 + nameLen2;
      var cigar = '';
      var cigarOps = 'MIDNSHP=X';
      for (var c = 0; c < nCigarOp; c++) {{
        var cigarVal = view.getUint32(cigarOff + c * 4, true);
        cigar += (cigarVal >> 4) + cigarOps[cigarVal & 0xf];
      }}

      // Parse sequence
      var seqOff = cigarOff + nCigarOp * 4;
      var seq = '';
      for (var s = 0; s < seqLen; s++) {{
        var b = raw[seqOff + (s >> 1)];
        seq += seqLookup[(s & 1) ? (b & 0x0f) : ((b >> 4) & 0x0f)];
      }}

      var refName2 = (refID >= 0 && refID < refs.length) ? refs[refID].name : '*';
      reads.push({{
        name: readName,
        flag: flag,
        chr: refName2,
        pos: posn + 1,
        mapq: mapq,
        cigar: cigar || '*',
        seq: seq
      }});

      readOffset += 4 + blockSize;
    }}

    // Build HTML
    var html = '<p class="meta">' + refs.length + ' reference(s) &middot; ' + reads.length + ' read(s) shown' + (reads.length >= maxReads ? ' (limited to ' + maxReads + ')' : '') + '</p>';

    // Reference info
    if (refs.length > 0) {{
      html += '<details style="margin-bottom:12px" open><summary style="cursor:pointer;font-size:13px;font-weight:600">References</summary>';
      html += '<table><tr><th>Name</th><th>Length</th></tr>';
      refs.forEach(function(ref) {{
        html += '<tr><td>' + ref.name + '</td><td>' + ref.length.toLocaleString() + ' bp</td></tr>';
      }});
      html += '</table></details>';
    }}

    // Header (collapsible)
    if (headerText.trim()) {{
      html += '<details style="margin-bottom:12px"><summary style="cursor:pointer;font-size:13px;color:#666">SAM header</summary>';
      html += '<pre style="font-size:11px;color:#888;margin-top:4px;max-height:200px;overflow:auto">' + headerText.replace(/</g,'&lt;') + '</pre></details>';
    }}

    // Reads table
    html += '<table><tr><th>Read Name</th><th>Flag</th><th>Chr</th><th>Pos</th><th>MAPQ</th><th>CIGAR</th><th>Sequence</th></tr>';
    reads.forEach(function(rd) {{
      html += '<tr><td>' + rd.name + '</td><td>' + rd.flag + '</td><td>' + rd.chr + '</td><td>' + rd.pos + '</td><td>' + rd.mapq + '</td><td>' + rd.cigar + '</td><td class="seq">' + colorBases(rd.seq) + '</td></tr>';
    }});
    html += '</table>';
    div.innerHTML = html;
  }} catch(e) {{
    document.getElementById('bamDiv').innerHTML =
      '<div class="no-preview"><p class="no-preview-icon">⚠️</p>' +
      '<p class="no-preview-title">BAM Parse Error</p>' +
      '<p class="no-preview-msg">' + e.message + '</p>' +
      '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a></div>';
  }}
}}

// ── BGZF decompression using browser DecompressionStream ──
async function asyncInflate(data) {{
  var ds = new DecompressionStream('deflate-raw');
  var writer = ds.writable.getWriter();
  writer.write(data);
  writer.close();
  var reader = ds.readable.getReader();
  var chunks = [];
  while (true) {{
    var result = await reader.read();
    if (result.done) break;
    chunks.push(result.value);
  }}
  var totalLen = 0;
  chunks.forEach(function(c) {{ totalLen += c.length; }});
  var out = new Uint8Array(totalLen);
  var off = 0;
  chunks.forEach(function(c) {{ out.set(c, off); off += c.length; }});
  return out;
}}

// ── Color bases helper ──
function colorBases(seq) {{
  return seq.replace(/[ATCGN]/gi, function(base) {{
    var upper = base.toUpperCase();
    if (upper === 'A') return '<span class="base-A">' + base + '</span>';
    if (upper === 'T') return '<span class="base-T">' + base + '</span>';
    if (upper === 'C') return '<span class="base-C">' + base + '</span>';
    if (upper === 'G') return '<span class="base-G">' + base + '</span>';
    return base;
  }});
}}

// ── IGV.js Viewer (CRAM/BCF only) ──
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

async function renderIgvViewer(name, ext, actions, content) {{
  actions.innerHTML = '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a>';
  content.innerHTML = '<div class="igv-viewer" id="igvDiv">Loading IGV.js (CRAM/BCF requires reference genome)...</div>';
  try {{
    await loadIgv();
    var div = document.getElementById('igvDiv');
    if (!div) return;
    div.innerHTML = '';

    var fileUrl = '/file/' + encodeURIComponent(name);
    var tracks = [];

    if (ext === 'cram') {{
      tracks.push({{ type: 'alignment', format: 'cram', url: fileUrl, name: name }});
    }} else if (ext === 'bcf') {{
      tracks.push({{ type: 'variant', format: 'vcf', url: fileUrl, name: name }});
    }}

    igv.createBrowser(div, {{
      genome: 'hg38',
      tracks: tracks
    }});
  }} catch(e) {{
    content.innerHTML = '<div class="no-preview"><p class="no-preview-icon">⚠️</p>' +
      '<p class="no-preview-title">IGV.js Load Error</p>' +
      '<p class="no-preview-msg">' + e.message + '<br><br>CRAM and BCF files require IGV.js with a reference genome.<br>Download and inspect with command-line tools.</p>' +
      '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a></div>';
  }}
}}

// ── HDF5 (h5ad) Viewer ──
var jsfiveLoaded = false;
function loadJsfive() {{
  return new Promise(function(resolve, reject) {{
    if (jsfiveLoaded) {{ resolve(); return; }}
    var s = document.createElement('script');
    s.src = 'https://cdn.jsdelivr.net/npm/jsfive@0.3.13/dist/browser/hdf5.js';
    s.onload = function() {{
      jsfiveLoaded = true;
      // jsfive exposes as window.jsfive or window.hdf5
      if (!window.jsfive && window.hdf5) window.jsfive = window.hdf5;
      resolve();
    }};
    s.onerror = function() {{ reject(new Error('Failed to load jsfive CDN')); }};
    document.head.appendChild(s);
  }});
}}

async function renderHdf5Viewer(name, actions, content) {{
  actions.innerHTML = '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a>';

  // Check file size first
  var fileInfo = FILES.find(function(f) {{ return f.name === name; }});
  var sizeMB = fileInfo ? (fileInfo.size / 1048576).toFixed(0) : 0;

  if (fileInfo && fileInfo.size > 500 * 1048576) {{
    // >500MB: too large for browser
    content.innerHTML =
      '<div class="no-preview">' +
        '<div class="no-preview-icon">🔬</div>' +
        '<p class="no-preview-title">' + name + '</p>' +
        '<p class="no-preview-msg">HDF5 file size: ' + sizeMB + ' MB<br>Files over 500 MB cannot be previewed in the browser.<br>Download and inspect with Python (scanpy/anndata).</p>' +
        '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download (' + sizeMB + ' MB)</a>' +
      '</div>';
    return;
  }}

  content.innerHTML = '<div class="hdf5-viewer" id="hdf5Div">Loading HDF5 viewer (' + sizeMB + ' MB)...</div>';

  try {{
    await loadJsfive();
    if (!window.jsfive) throw new Error('jsfive library not available');

    var resp = await fetch('/file/' + encodeURIComponent(name));
    var buf = await resp.arrayBuffer();
    var f = new jsfive.File(buf);

    var div = document.getElementById('hdf5Div');
    if (!div) return;
    var html = '';

    // Show file structure
    html += '<div class="hdf5-section"><h3>File Structure</h3><table>';
    html += '<tr><th>Key</th><th>Type</th><th>Shape</th><th>Dtype</th></tr>';

    function walkGroup(group, prefix) {{
      var keys = group.keys || [];
      if (typeof group.keys === 'function') keys = group.keys();
      keys.forEach(function(key) {{
        var fullKey = prefix ? prefix + '/' + key : key;
        try {{
          var item = group.get(key);
          if (item && item.shape) {{
            html += '<tr><td>' + fullKey + '</td><td>Dataset</td><td>[' + (item.shape || []).join(', ') + ']</td><td>' + (item.dtype || '-') + '</td></tr>';
          }} else if (item && (item.keys || typeof item.keys === 'function')) {{
            html += '<tr><td>' + fullKey + '/</td><td>Group</td><td>-</td><td>-</td></tr>';
            walkGroup(item, fullKey);
          }} else {{
            html += '<tr><td>' + fullKey + '</td><td>-</td><td>-</td><td>-</td></tr>';
          }}
        }} catch(e) {{
          html += '<tr><td>' + fullKey + '</td><td>Error</td><td colspan="2">' + e.message + '</td></tr>';
        }}
      }});
    }}

    walkGroup(f, '');
    html += '</table></div>';

    // Try to show obs metadata if available
    try {{
      var obs = f.get('obs');
      if (obs) {{
        var obsKeys = obs.keys ? (typeof obs.keys === 'function' ? obs.keys() : obs.keys) : [];
        if (obsKeys.length > 0) {{
          html += '<div class="hdf5-section"><h3>Observations (obs) columns</h3><table>';
          html += '<tr><th>Column</th><th>Dtype</th></tr>';
          obsKeys.forEach(function(k) {{
            try {{
              var col = obs.get(k);
              html += '<tr><td>' + k + '</td><td>' + (col && col.dtype ? col.dtype : '-') + '</td></tr>';
            }} catch(e) {{
              html += '<tr><td>' + k + '</td><td>Error</td></tr>';
            }}
          }});
          html += '</table></div>';
        }}
      }}
    }} catch(e) {{ /* obs not available */ }}

    // Try to show var metadata if available
    try {{
      var varData = f.get('var');
      if (varData) {{
        var varKeys = varData.keys ? (typeof varData.keys === 'function' ? varData.keys() : varData.keys) : [];
        if (varKeys.length > 0) {{
          html += '<div class="hdf5-section"><h3>Variables (var) columns</h3><table>';
          html += '<tr><th>Column</th><th>Dtype</th></tr>';
          varKeys.forEach(function(k) {{
            try {{
              var col = varData.get(k);
              html += '<tr><td>' + k + '</td><td>' + (col && col.dtype ? col.dtype : '-') + '</td></tr>';
            }} catch(e) {{
              html += '<tr><td>' + k + '</td><td>Error</td></tr>';
            }}
          }});
          html += '</table></div>';
        }}
      }}
    }} catch(e) {{ /* var not available */ }}

    div.innerHTML = html;
  }} catch(e) {{
    content.innerHTML = '<div class="no-preview"><p class="no-preview-icon">⚠️</p>' +
      '<p class="no-preview-title">HDF5 Load Error</p>' +
      '<p class="no-preview-msg">' + e.message + '<br><br>Download and inspect with Python:<br><code>import anndata; ad = anndata.read_h5ad("' + name + '")</code></p>' +
      '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a></div>';
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
