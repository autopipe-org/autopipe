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
  <h3>{name}</h3>
  <p>{desc}</p>
  <div class="meta">
    <div>{tools}</div>
    <div>{tags}</div>
  </div>
  <div class="info">v{version} &middot; {author}</div>
</a>"#,
            id = p.pipeline_id,
            name = html_escape(&p.name),
            desc = html_escape(&p.description),
            tools = tools_html,
            tags = tags_html,
            version = html_escape(&p.version),
            author = if p.author.is_empty() {
                "unknown"
            } else {
                &p.author
            },
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
<title>AutoPipe Registry</title>
<style>{CSS}</style>
</head>
<body>
<header>
  <h1>AutoPipe Registry</h1>
  <p>Bioinformatics Snakemake Pipeline Registry</p>
</header>
<main>
  <form class="search" method="get" action="/">
    <input type="text" name="q" placeholder="Search pipelines by name, tool, or tag..." value="{search_value}">
    <button type="submit">Search</button>
  </form>
  <div class="count">{count} pipeline(s)</div>
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
                r#"<!DOCTYPE html><html><head><title>Not Found</title><style>{CSS}</style></head>
<body><header><h1>AutoPipe Registry</h1></header>
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
    let io_html = format!(
        "Input: {} &rarr; Output: {}",
        pipeline.input_formats.join(", "),
        pipeline.output_formats.join(", "),
    );

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

    let mut file_sections = String::new();
    for (name, content) in &files {
        if !content.is_empty() {
            file_sections.push_str(&format!(
                r#"<details open>
<summary>{name}</summary>
<pre><code>{content}</code></pre>
</details>"#,
                name = name,
                content = html_escape(content),
            ));
        }
    }

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{name} - AutoPipe Registry</title>
<style>{CSS}</style>
</head>
<body>
<header>
  <h1><a href="/" style="color:inherit;text-decoration:none">AutoPipe Registry</a></h1>
</header>
<main>
  <a href="/">&larr; Back to list</a>
  <h2>{name}</h2>
  <p>{desc}</p>
  <div class="detail-meta">
    <div><strong>Version:</strong> {version}</div>
    <div><strong>Author:</strong> {author}</div>
    <div><strong>Formats:</strong> {io}</div>
    <div><strong>Tools:</strong> {tools}</div>
    <div><strong>Tags:</strong> {tags}</div>
  </div>
  <div class="actions">
    <a href="/pipelines/{id}/download" class="btn">Download ZIP</a>
  </div>
  <h3>Files</h3>
  {files}
</main>
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
        io = io_html,
        tools = tools_html,
        tags = tags_html,
        id = id,
        files = file_sections,
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
body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; background: #f5f5f5; color: #333; }
header { background: #1a73e8; color: white; padding: 20px 30px; }
header h1 { font-size: 1.5em; }
header p { opacity: 0.8; margin-top: 4px; }
main { max-width: 900px; margin: 20px auto; padding: 0 20px; }
.search { display: flex; gap: 8px; margin-bottom: 16px; }
.search input { flex: 1; padding: 10px 14px; border: 1px solid #ddd; border-radius: 6px; font-size: 15px; }
.search button { padding: 10px 20px; background: #1a73e8; color: white; border: none; border-radius: 6px; cursor: pointer; font-size: 15px; }
.search button:hover { background: #1557b0; }
.count { color: #666; margin-bottom: 12px; font-size: 14px; }
.grid { display: flex; flex-direction: column; gap: 12px; }
.card { display: block; background: white; border-radius: 8px; padding: 16px 20px; text-decoration: none; color: inherit; border: 1px solid #e0e0e0; transition: box-shadow 0.2s; }
.card:hover { box-shadow: 0 2px 8px rgba(0,0,0,0.12); }
.card h3 { color: #1a73e8; margin-bottom: 6px; }
.card p { color: #555; font-size: 14px; margin-bottom: 8px; }
.meta { display: flex; flex-wrap: wrap; gap: 4px; margin-bottom: 6px; }
.tag { display: inline-block; padding: 2px 8px; border-radius: 12px; font-size: 12px; background: #e8f0fe; color: #1a73e8; }
.tag.tool { background: #e6f4ea; color: #137333; }
.info { font-size: 12px; color: #888; }
.empty { text-align: center; color: #888; padding: 40px; }
.detail-meta { background: white; border-radius: 8px; padding: 16px; margin: 16px 0; border: 1px solid #e0e0e0; }
.detail-meta div { margin-bottom: 6px; }
.actions { margin: 16px 0; }
.btn { display: inline-block; padding: 10px 24px; background: #1a73e8; color: white; text-decoration: none; border-radius: 6px; font-size: 15px; }
.btn:hover { background: #1557b0; }
details { margin-bottom: 12px; background: white; border: 1px solid #e0e0e0; border-radius: 8px; }
summary { padding: 10px 16px; cursor: pointer; font-weight: 600; background: #fafafa; border-radius: 8px; }
pre { padding: 16px; overflow-x: auto; font-size: 13px; line-height: 1.5; background: #f8f9fa; }
h2 { margin: 16px 0 8px; }
h3 { margin: 20px 0 10px; }
a { color: #1a73e8; }
"#;
