#![allow(dead_code)]

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

/// Shared state: stores files keyed by filename.
#[derive(Clone)]
struct ViewerState {
    files: Arc<Mutex<HashMap<String, FileEntry>>>,
}

struct FileEntry {
    data: Vec<u8>,
    mime: String,
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

/// Start the viewer server (or reuse existing one).
/// Returns the port number.
async fn ensure_server() -> Result<u16, String> {
    let lock = get_viewer_lock().await;
    let mut handle = lock.lock().await;

    // If server is already running, return its port
    if let Some(h) = handle.as_ref() {
        return Ok(h.port());
    }

    let files = get_file_store().await.clone();
    let state = ViewerState { files };

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/logo.png", get(logo_handler))
        .route("/file/{filename}", get(file_handler))
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
pub async fn show_files(files: Vec<(String, Vec<u8>, String)>) -> Result<String, String> {
    let store = get_file_store().await;
    {
        let mut map = store.lock().await;
        map.clear();
        for (name, data, mime) in files {
            map.insert(name, FileEntry { data, mime });
        }
    }

    let port = ensure_server().await?;

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

async fn index_handler(State(state): State<ViewerState>) -> Html<String> {
    let files = state.files.lock().await;
    let mut items = String::new();

    for (name, entry) in files.iter() {
        let is_image = entry.mime.starts_with("image/");
        let is_pdf = entry.mime == "application/pdf";
        let is_text = entry.mime.starts_with("text/")
            || entry.mime == "application/json"
            || entry.mime == "application/csv";

        if is_image {
            items.push_str(&format!(
                r#"<div class="item">
                    <div class="item-header">
                        <h3>{name}</h3>
                        <div class="item-actions">
                            <button class="zoom-btn" onclick="zoom(this,-1)">−</button>
                            <button class="zoom-btn zoom-reset" onclick="zoom(this,0)">100%</button>
                            <button class="zoom-btn" onclick="zoom(this,1)">+</button>
                            <a class="dl-btn" href="/file/{name}" download>Download</a>
                        </div>
                    </div>
                    <div class="img-wrap"><img src="/file/{name}" alt="{name}" data-scale="1"></div>
                </div>"#,
                name = name
            ));
        } else if is_pdf {
            items.push_str(&format!(
                r#"<div class="item">
                    <div class="item-header">
                        <h3>{name}</h3>
                        <a class="dl-btn" href="/file/{name}" download>Download</a>
                    </div>
                    <embed src="/file/{name}" type="application/pdf" width="100%" height="600px">
                </div>"#,
                name = name
            ));
        } else if is_text {
            items.push_str(&format!(
                r#"<div class="item">
                    <div class="item-header">
                        <h3>{name}</h3>
                        <a class="dl-btn" href="/file/{name}" download>Download</a>
                    </div>
                    <iframe src="/file/{name}" width="100%" height="400px" style="border:1px solid #ddd;border-radius:8px;"></iframe>
                </div>"#,
                name = name
            ));
        } else {
            items.push_str(&format!(
                r#"<div class="item">
                    <div class="item-header">
                        <h3>{name}</h3>
                        <a class="dl-btn" href="/file/{name}" download>Download</a>
                    </div>
                </div>"#,
                name = name
            ));
        }
    }

    if items.is_empty() {
        items = "<p style=\"color:#999;text-align:center;padding:40px;\">No files to display.</p>"
            .to_string();
    }

    Html(format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>AutoPipe Results</title>
<style>
  * {{ box-sizing: border-box; margin: 0; padding: 0; }}
  body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Inter', sans-serif; background: #fafafa; color: #111; line-height: 1.5; }}
  header {{ padding: 14px 40px; border-bottom: 1px solid #eee; background: #fff; }}
  .header-top {{ display: flex; align-items: baseline; gap: 12px; }}
  .logo {{ font-size: 1.15rem; font-weight: 700; color: #111; text-decoration: none; letter-spacing: -0.02em; display: flex; align-items: center; gap: 8px; }}
  .logo-icon {{ height: 24px; width: auto; }}
  .header-sub {{ font-size: 14px; color: #999; font-weight: 400; }}
  .container {{ max-width: 1000px; margin: 24px auto; padding: 0 24px; }}
  .item {{ background: #fff; border: 1px solid #e5e5e5; border-radius: 10px; padding: 20px; margin-bottom: 16px; }}
  .item h3 {{ font-size: 14px; color: #333; margin: 0; padding: 0; border: none; }}
  .item-header {{ display: flex; justify-content: space-between; align-items: center; margin-bottom: 12px; padding-bottom: 8px; border-bottom: 1px solid #f0f0f0; }}
  .item-actions {{ display: flex; align-items: center; gap: 6px; }}
  .zoom-btn {{ width: 32px; height: 28px; border: 1px solid #ddd; border-radius: 6px; background: #f8f8f8; color: #555; font-size: 14px; cursor: pointer; display: flex; align-items: center; justify-content: center; }}
  .zoom-btn:hover {{ background: #eee; border-color: #ccc; }}
  .zoom-reset {{ width: auto; padding: 0 8px; font-size: 12px; }}
  .dl-btn {{ display: inline-block; padding: 4px 12px; border: 1px solid #ddd; border-radius: 6px; background: #f8f8f8; color: #555; font-size: 12px; font-weight: 500; text-decoration: none; }}
  .dl-btn:hover {{ background: #eee; border-color: #ccc; text-decoration: none; }}
  .img-wrap {{ overflow: auto; border-radius: 6px; }}
  .img-wrap img {{ max-width: 100%; height: auto; transition: transform 0.2s; transform-origin: top left; }}
  .item a {{ color: #0366d6; text-decoration: none; font-size: 14px; }}
  .item a:hover {{ text-decoration: underline; }}
</style>
</head>
<body>
<header>
  <div class="header-top">
    <span class="logo"><img src="/logo.png" alt="" class="logo-icon">AutoPipe</span>
    <span class="header-sub">Results Viewer</span>
  </div>
</header>
<div class="container">{items}</div>
<script>
function zoom(btn, dir) {{
  var wrap = btn.closest('.item').querySelector('.img-wrap');
  var img = wrap.querySelector('img');
  var scale = parseFloat(img.dataset.scale || '1');
  if (dir === 0) scale = 1;
  else scale = Math.max(0.25, Math.min(3, scale + dir * 0.25));
  img.dataset.scale = scale;
  img.style.transform = 'scale(' + scale + ')';
  if (scale > 1) {{ img.style.maxWidth = 'none'; }} else {{ img.style.maxWidth = '100%'; }}
  btn.closest('.item-actions').querySelector('.zoom-reset').textContent = Math.round(scale * 100) + '%';
}}
</script>
</body>
</html>"#,
        items = items
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
