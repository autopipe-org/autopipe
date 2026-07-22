//! Boots the real viewer HTTP server against real files so the SSH/base64
//! data path can be exercised outside the desktop app.
//!
//! usage: viewer_repro <dir-with-bam-and-reference> <reference-filename>

use autopipe_desktop::config::AppConfig;
use autopipe_desktop::mcp::viewer;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("dir required");
    let reference = args.next().expect("reference filename required");

    let cfg = AppConfig::load();
    eprintln!("ssh: {}@{}:{}", cfg.ssh_user, cfg.ssh_host, cfg.ssh_port);

    // Register exactly like show_results does for genomics files: remote
    // entries served through the SSH Range proxy.
    let mut remote_files = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("read_dir") {
        let path = entry.expect("entry").path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let size = std::fs::metadata(&path).expect("metadata").len();
        let listed = !matches!(
            path.extension().and_then(|e| e.to_str()).unwrap_or(""),
            "bai" | "crai" | "tbi" | "csi" | "fai" | "idx"
        );
        remote_files.push((
            name,
            path.to_string_lossy().to_string(),
            size,
            "application/octet-stream".to_string(),
            listed,
        ));
    }
    for (n, _, s, _, listed) in &remote_files {
        eprintln!("  registered {} ({} bytes){}", n, s, if *listed { "" } else { "  [hidden]" });
    }

    let plugins_dir = std::env::var("PLUGINS_DIR").expect("PLUGINS_DIR required");

    // `open::that` would try to launch a browser; BROWSER=/bin/true keeps it
    // quiet so we still get the URL back.
    std::env::set_var("BROWSER", "/bin/true");

    match viewer::show_files(
        Vec::new(),
        remote_files,
        plugins_dir,
        Some(reference),
        Some(cfg),
        None,
        None,
    )
    .await
    {
        Ok(url) => println!("VIEWER_URL={}", url),
        Err(e) => println!("VIEWER_ERR={}", e),
    }

    // Keep the server alive for the driver.
    tokio::time::sleep(std::time::Duration::from_secs(600)).await;
}
