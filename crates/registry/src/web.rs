use std::io::Write;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{Html, IntoResponse};

use crate::db::{self, DbState};
use common::models::SearchQuery;

/// GET / — Main page with search bar and pipeline list
pub async fn index_page(
    State(state): State<Arc<DbState>>,
    Query(query): Query<SearchQuery>,
) -> Html<String> {
    let pipelines = if let Some(ref q) = query.q {
        if q.is_empty() {
            db::list_pipelines(&state.client).await.unwrap_or_default()
        } else {
            db::search_pipelines(&state.client, q)
                .await
                .unwrap_or_default()
        }
    } else {
        db::list_pipelines(&state.client).await.unwrap_or_default()
    };

    let search_value = query.q.as_deref().unwrap_or("");

    let mut cards = String::new();
    for p in &pipelines {
        let tools_html: String = p
            .tools
            .iter()
            .map(|t| format!(r#"<span class="tag tool">{}</span>"#, html_escape(t)))
            .collect::<Vec<_>>()
            .join(" ");
        let tags_html: String = p
            .tags
            .iter()
            .map(|t| format!(r#"<span class="tag">{}</span>"#, html_escape(t)))
            .collect::<Vec<_>>()
            .join(" ");

        cards.push_str(&format!(
            r#"<a href="/pipelines/{id}" class="card">
  <div class="card-title">{name} <span class="card-version">v{version}</span></div>
  <div class="card-desc">{desc}</div>
  <div class="card-tags">{tools} {tags}</div>
</a>"#,
            id = p.pipeline_id,
            name = html_escape(&p.name),
            desc = html_escape(&p.description),
            tools = tools_html,
            tags = tags_html,
            version = html_escape(&p.version),
        ));
    }

    if pipelines.is_empty() {
        cards = r#"<p class="empty">No pipelines found.</p>"#.to_string();
    }

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>AutoPipe</title>
<style>{CSS}</style>
</head>
<body>
<header>
  <a href="/" class="logo">AutoPipe</a>
</header>
<main>
  <form class="search" method="get" action="/">
    <input type="text" name="q" placeholder="Search by name, tool, or tag..." value="{search_value}">
    <button type="submit">Search</button>
  </form>
  <div class="count">{count} pipelines</div>
  <div class="grid">
    {cards}
  </div>
</main>
</body>
</html>"#,
        CSS = CSS,
        search_value = html_escape(search_value),
        count = pipelines.len(),
        cards = cards,
    );

    Html(html)
}

/// GET /pipelines/:id — Detail page with file previews and download
pub async fn detail_page(
    State(state): State<Arc<DbState>>,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    let pipeline = match db::get_pipeline(&state.client, id).await {
        Ok(Some(p)) => p,
        _ => {
            return Html(format!(
                r#"<!DOCTYPE html><html><head><title>Not Found - AutoPipe</title><style>{CSS}</style></head>
<body><header><a href="/" class="logo">AutoPipe</a></header>
<main><p>Pipeline not found. <a href="/">Back to list</a></p></main></body></html>"#,
                CSS = CSS,
            ));
        }
    };

    let tools_html: String = pipeline
        .tools
        .iter()
        .map(|t| format!(r#"<span class="tag tool">{}</span>"#, html_escape(t)))
        .collect::<Vec<_>>()
        .join(" ");
    let tags_html: String = pipeline
        .tags
        .iter()
        .map(|t| format!(r#"<span class="tag">{}</span>"#, html_escape(t)))
        .collect::<Vec<_>>()
        .join(" ");

    let files: Vec<(&str, &str)> = [
        ("Snakefile", pipeline.snakefile.as_str()),
        ("Dockerfile", pipeline.dockerfile.as_str()),
        ("config.yaml", pipeline.config_yaml.as_str()),
    ]
    .into_iter()
    .filter(|(_, c)| !c.is_empty())
    .collect();

    let metadata_json =
        serde_json::to_string_pretty(&pipeline.metadata_json).unwrap_or_default();
    let readme = &pipeline.readme;

    // Build tab buttons + tab panels
    let mut tab_buttons = String::new();
    let mut tab_panels = String::new();
    let mut idx = 0usize;

    for (name, content) in &files {
        let active = if idx == 0 { " active" } else { "" };
        tab_buttons.push_str(&format!(
            r#"<button class="tab-btn{active}" onclick="switchTab(event, 'tab-{idx}')">{name}</button>"#,
            active = active,
            idx = idx,
            name = name,
        ));
        tab_panels.push_str(&format!(
            r#"<div id="tab-{idx}" class="tab-panel{active}"><pre><code>{content}</code></pre></div>"#,
            idx = idx,
            active = active,
            content = html_escape(content),
        ));
        idx += 1;
    }

    if !metadata_json.is_empty() {
        let active = if idx == 0 { " active" } else { "" };
        tab_buttons.push_str(&format!(
            r#"<button class="tab-btn{active}" onclick="switchTab(event, 'tab-{idx}')">metadata.json</button>"#,
            active = active,
            idx = idx,
        ));
        tab_panels.push_str(&format!(
            r#"<div id="tab-{idx}" class="tab-panel{active}"><pre><code>{content}</code></pre></div>"#,
            idx = idx,
            active = active,
            content = html_escape(&metadata_json),
        ));
        idx += 1;
    }

    if !readme.is_empty() {
        let active = if idx == 0 { " active" } else { "" };
        tab_buttons.push_str(&format!(
            r#"<button class="tab-btn{active}" onclick="switchTab(event, 'tab-{idx}')">README.md</button>"#,
            active = active,
            idx = idx,
        ));
        tab_panels.push_str(&format!(
            r#"<div id="tab-{idx}" class="tab-panel{active}"><pre><code>{content}</code></pre></div>"#,
            idx = idx,
            active = active,
            content = html_escape(readme),
        ));
    }

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{name} - AutoPipe</title>
<style>{CSS}</style>
</head>
<body>
<header>
  <a href="/" class="logo">AutoPipe</a>
</header>
<main>
  <a href="/" class="back-link">&larr; Back to list</a>
  <div class="detail-header">
    <div>
      <h2>{name}</h2>
      <p class="detail-desc">{desc}</p>
    </div>
    <a href="/pipelines/{id}/download" class="btn">Download ZIP</a>
  </div>
  <div class="detail-info">
    <div class="detail-info-item"><span class="label">VERSION</span><span class="value">{version}</span></div>
    <div class="detail-info-item"><span class="label">AUTHOR</span><span class="value">{author}</span></div>
    <div class="detail-info-item"><span class="label">INPUT</span><span class="value">{input}</span></div>
    <div class="detail-info-item"><span class="label">OUTPUT</span><span class="value">{output}</span></div>
  </div>
  <div class="detail-tags">
    <span class="label">TOOLS</span> {tools}
    <span class="label" style="margin-left:16px">TAGS</span> {tags}
  </div>
  <div class="files-section">
    <div class="tab-bar">{tab_buttons}</div>
    <div class="tab-content">{tab_panels}</div>
  </div>
</main>
<script>
function switchTab(e, tabId) {{
  document.querySelectorAll('.tab-btn').forEach(b => b.classList.remove('active'));
  document.querySelectorAll('.tab-panel').forEach(p => p.classList.remove('active'));
  e.target.classList.add('active');
  document.getElementById(tabId).classList.add('active');
}}
</script>
</body>
</html>"#,
        CSS = CSS,
        name = html_escape(&pipeline.name),
        desc = html_escape(&pipeline.description),
        version = html_escape(&pipeline.version),
        author = if pipeline.author.is_empty() {
            "unknown".to_string()
        } else {
            html_escape(&pipeline.author)
        },
        input = html_escape(&pipeline.input_formats.join(", ")),
        output = html_escape(&pipeline.output_formats.join(", ")),
        tools = tools_html,
        tags = tags_html,
        id = id,
        tab_buttons = tab_buttons,
        tab_panels = tab_panels,
    );

    Html(html)
}

/// GET /pipelines/:id/download — Download all pipeline files as a zip
pub async fn download_zip(
    State(state): State<Arc<DbState>>,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    let pipeline = match db::get_pipeline(&state.client, id).await {
        Ok(Some(p)) => p,
        _ => {
            return (
                axum::http::StatusCode::NOT_FOUND,
                "Pipeline not found".to_string(),
            )
                .into_response();
        }
    };

    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        let files = [
            ("Snakefile", &pipeline.snakefile),
            ("Dockerfile", &pipeline.dockerfile),
            ("config.yaml", &pipeline.config_yaml),
            (
                "metadata.json",
                &serde_json::to_string_pretty(&pipeline.metadata_json).unwrap_or_default(),
            ),
            ("README.md", &pipeline.readme),
        ];

        for (name, content) in &files {
            let path = format!("{}/{}", pipeline.name, name);
            if zip.start_file(&path, options).is_ok() {
                let _ = zip.write_all(content.as_bytes());
            }
        }

        let _ = zip.finish();
    }

    let filename = format!("{}.zip", pipeline.name);
    (
        [
            (header::CONTENT_TYPE, "application/zip".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        buf,
    )
        .into_response()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const CSS: &str = r#"
* { box-sizing: border-box; margin: 0; padding: 0; }
body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Inter', sans-serif; background: #fafafa; color: #111; line-height: 1.5; }

/* Header */
header { padding: 14px 40px; border-bottom: 1px solid #eee; background: #fff; }
.logo { font-size: 1.15rem; font-weight: 700; color: #111; text-decoration: none; letter-spacing: -0.02em; }

/* Main layout - wide */
main { max-width: 1200px; margin: 0 auto; padding: 32px 40px; }

/* Search */
.search { margin-bottom: 20px; }
.search input { width: 100%; padding: 11px 16px; border: 1px solid #ddd; border-radius: 8px; font-size: 14px; background: #fff; transition: border-color 0.2s; outline: none; }
.search input:focus { border-color: #999; }
.search button { display: none; }
.count { color: #999; margin-bottom: 16px; font-size: 13px; }

/* Pipeline cards */
.grid { display: flex; flex-direction: column; gap: 1px; background: #e5e5e5; border: 1px solid #e5e5e5; border-radius: 10px; overflow: hidden; }
.card { display: block; background: #fff; padding: 24px 28px; text-decoration: none; color: inherit; transition: background 0.15s; }
.card:hover { background: #f8f8f8; }
.card-title { font-size: 15px; font-weight: 600; color: #111; margin-bottom: 12px; }
.card-version { font-weight: 400; color: #aaa; font-size: 13px; margin-left: 8px; }
.card-desc { color: #666; font-size: 13px; margin-bottom: 14px; line-height: 1.5; }
.card-tags { display: flex; flex-wrap: wrap; gap: 6px; }
.tag { display: inline-block; padding: 3px 10px; border-radius: 100px; font-size: 11px; background: #f0f0f0; color: #666; font-weight: 500; }
.tag.tool { background: #111; color: #fff; }
.empty { text-align: center; color: #999; padding: 60px 20px; font-size: 14px; background: #fff; }

/* Detail page */
.back-link { display: inline-block; font-size: 13px; color: #888; text-decoration: none; margin-bottom: 24px; }
.back-link:hover { color: #111; }
.detail-header { display: flex; justify-content: space-between; align-items: flex-start; gap: 24px; margin-bottom: 24px; }
.detail-header h2 { font-size: 1.5rem; font-weight: 700; letter-spacing: -0.02em; margin-bottom: 8px; }
.detail-desc { color: #666; font-size: 14px; }
.detail-info { display: flex; gap: 48px; padding: 20px 0; border-top: 1px solid #eee; border-bottom: 1px solid #eee; margin-bottom: 16px; }
.detail-info-item { display: flex; flex-direction: column; gap: 4px; }
.label { font-size: 11px; font-weight: 600; color: #999; letter-spacing: 0.04em; }
.value { font-size: 14px; color: #111; }
.detail-tags { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; padding: 16px 0; margin-bottom: 24px; }
.btn { display: inline-block; padding: 9px 22px; background: #111; color: #fff; text-decoration: none; border-radius: 8px; font-size: 13px; font-weight: 500; transition: background 0.2s; white-space: nowrap; }
.btn:hover { background: #333; }

/* File tabs */
.files-section { background: #fff; border: 1px solid #e5e5e5; border-radius: 10px; overflow: hidden; }
.tab-bar { display: flex; border-bottom: 1px solid #e5e5e5; background: #fafafa; overflow-x: auto; }
.tab-btn { padding: 10px 20px; border: none; background: none; font-size: 13px; font-weight: 500; color: #888; cursor: pointer; border-bottom: 2px solid transparent; transition: color 0.15s, border-color 0.15s; white-space: nowrap; }
.tab-btn:hover { color: #111; }
.tab-btn.active { color: #111; border-bottom-color: #111; }
.tab-panel { display: none; }
.tab-panel.active { display: block; }
.tab-panel pre { padding: 24px; overflow-x: auto; font-size: 13px; line-height: 1.6; background: #fff; margin: 0; }

/* General */
a { color: #111; }
a:hover { color: #555; }
"#;
