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

  /* IGV viewer */
  .igv-viewer {{ width: 100%; min-height: 500px; }}

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
  var igvExts = ['bam','cram','vcf','bcf','bed','gff','gtf','gff3','fasta','fa','bigwig','bw','bigbed','bb'];
  var hdf5Exts = ['h5ad','h5','hdf5'];

  if (imageExts.indexOf(ext) >= 0) {{
    renderImageViewer(name, actions, content);
  }} else if (ext === 'pdf') {{
    renderPdfViewer(name, actions, content);
  }} else if (textExts.indexOf(ext) >= 0) {{
    renderTextViewer(name, actions, content);
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

// ── IGV.js Viewer ──
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
  content.innerHTML = '<div class="igv-viewer" id="igvDiv">Loading IGV.js...</div>';

  try {{
    await loadIgv();
    var div = document.getElementById('igvDiv');
    if (!div) return;
    div.innerHTML = '';

    var fileUrl = '/file/' + encodeURIComponent(name);
    var tracks = [];

    // Determine track type based on extension
    if (ext === 'bam' || ext === 'cram') {{
      tracks.push({{
        type: 'alignment',
        format: ext,
        url: fileUrl,
        name: name
      }});
    }} else if (ext === 'vcf' || ext === 'bcf') {{
      tracks.push({{
        type: 'variant',
        format: ext === 'bcf' ? 'vcf' : ext,
        url: fileUrl,
        name: name
      }});
    }} else if (ext === 'bed') {{
      tracks.push({{
        type: 'annotation',
        format: 'bed',
        url: fileUrl,
        name: name
      }});
    }} else if (ext === 'gff' || ext === 'gtf' || ext === 'gff3') {{
      tracks.push({{
        type: 'annotation',
        format: ext,
        url: fileUrl,
        name: name
      }});
    }} else if (ext === 'fasta' || ext === 'fa') {{
      // FASTA as reference
      igv.createBrowser(div, {{
        reference: {{
          fastaURL: fileUrl,
          indexed: false
        }}
      }});
      return;
    }} else if (ext === 'bigwig' || ext === 'bw') {{
      tracks.push({{
        type: 'wig',
        format: 'bigwig',
        url: fileUrl,
        name: name
      }});
    }} else if (ext === 'bigbed' || ext === 'bb') {{
      tracks.push({{
        type: 'annotation',
        format: 'bigbed',
        url: fileUrl,
        name: name
      }});
    }}

    igv.createBrowser(div, {{
      genome: 'hg38',
      tracks: tracks
    }});
  }} catch(e) {{
    content.innerHTML = '<div class="no-preview"><p class="no-preview-icon">⚠️</p>' +
      '<p class="no-preview-title">IGV.js Load Error</p>' +
      '<p class="no-preview-msg">' + e.message + '</p>' +
      '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a></div>';
  }}
}}

// ── HDF5 (h5ad) Viewer ──
var jsfiveLoaded = false;
function loadJsfive() {{
  return new Promise(function(resolve, reject) {{
    if (jsfiveLoaded) {{ resolve(); return; }}
    var s = document.createElement('script');
    s.src = 'https://cdn.jsdelivr.net/npm/jsfive@0.3.10/dist/browser/hdf5.js';
    s.onload = function() {{ jsfiveLoaded = true; resolve(); }};
    s.onerror = function() {{ reject(new Error('Failed to load jsfive')); }};
    document.head.appendChild(s);
  }});
}}

async function renderHdf5Viewer(name, actions, content) {{
  actions.innerHTML = '<a class="btn" href="/file/' + encodeURIComponent(name) + '" download>Download</a>';
  content.innerHTML = '<div class="hdf5-viewer" id="hdf5Div">Loading HDF5 viewer...</div>';

  try {{
    await loadJsfive();
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
      '<p class="no-preview-msg">' + e.message + '</p>' +
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
      '<p class="no-preview-msg">.' + ext + ' 형식은 내장 뷰어가 제공되지 않습니다.<br>이 형식을 지원하는 플러그인을 설치하면 미리보기가 가능합니다.</p>' +
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
