use common::api_client::RegistryClient;
use common::models::{clean_content, parse_ro_crate_metadata, PipelineMetadata};
use common::templates;
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::{tool, tool_handler, tool_router, ErrorData, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::config::AppConfig;
use crate::mcp::viewer;
use crate::ssh;

// ── Parameter structs ───────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
struct SearchParams {
    /// Search keyword
    query: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DownloadParams {
    /// Pipeline ID to download
    pipeline_id: i32,
    /// Remote directory path (optional, defaults to configured pipelines directory)
    output_dir: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PipelineDirParams {
    /// Remote path to the pipeline directory on the SSH server
    pipeline_dir: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct BuildParams {
    /// Remote path to the pipeline directory (on the SSH server)
    pipeline_dir: String,
    /// Docker image name/tag
    image_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CheckBuildParams {
    /// Docker image name (same as used in build_image)
    image_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DryRunParams {
    /// Docker image name
    image_name: String,
    /// Remote input data directory (mounted as read-only in Docker)
    input_dir: String,
    /// Remote output directory (optional, defaults to configured output directory)
    output_dir: Option<String>,
    /// Number of CPU cores (default: 8)
    cores: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ExecuteParams {
    /// Docker image name
    image_name: String,
    /// Run name (used for log file and container naming)
    run_name: String,
    /// Remote input data directory (mounted as read-only in Docker)
    input_dir: String,
    /// Number of CPU cores (default: 8)
    cores: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct StatusParams {
    /// Run name (matches the run_name used in execute_pipeline)
    run_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CleanupFailedParams {
    /// Docker image name (e.g. autopipe-my-pipeline)
    image_name: String,
    /// Run name of the failed run (matches the output subdirectory name)
    run_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CreateSymlinkParams {
    /// Source path on the remote server (the existing file/directory to link to)
    source: String,
    /// Target symlink path to create on the remote server
    target: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RemoveSymlinkParams {
    /// Path of the symlink to remove on the remote server
    symlink_path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ListFilesParams {
    /// Remote directory path to list
    path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ReadFileParams {
    /// Remote file path to read
    path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct WriteFileParams {
    /// Remote file path to write to
    path: String,
    /// Content to write to the file
    content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ShowResultsParams {
    /// Remote file or directory path to display in the browser.
    path: String,
    /// Optional filter: "image", "text", "genomics", "pdf", "hdf5".
    /// When set, only files matching this type are loaded into the viewer.
    /// Omit to load all files.
    #[serde(default)]
    filter: Option<String>,
    /// Optional reference genome for IGV viewer. Can be:
    /// - A genome ID string like "hg38", "hg19", "mm10", "mm39", "dm6", "ce11", "danRer11", "sacCer3", "rn7", "galGal6"
    /// - A FASTA filename that exists among the result files (e.g., "reference.fasta")
    /// When provided, the IGV tab becomes available for genomics files (BAM, VCF, BED, GFF, FASTA).
    /// Without reference, only the Data tab is shown for these formats.
    /// CRAM and BCF files require a reference for viewing.
    /// If the user has a local reference FASTA in the results directory, pass its filename.
    /// Otherwise, determine the organism from context and pass the appropriate genome ID.
    #[serde(default)]
    reference: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DownloadResultsParams {
    /// Remote file or directory path to download from the SSH server
    remote_path: String,
    /// Local directory to save the downloaded file(s). If omitted, uses the OS default Downloads folder (e.g., ~/Downloads on macOS/Linux, C:\Users\<user>\Downloads on Windows). Tell the user the default path and ask if they want to change it before calling.
    #[serde(default)]
    local_dir: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct UploadWorkflowParams {
    /// Remote path to the pipeline directory on the SSH server
    pipeline_dir: String,
    /// Git commit message (optional, auto-generated if omitted)
    commit_message: Option<String>,
    /// Semantic version string (e.g., "1.0.0"). Claude should determine this based on changes.
    version: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PublishWorkflowParams {
    /// GitHub URL of the uploaded workflow (from upload_workflow result)
    github_url: String,
    /// Set to an existing pipeline_id to link this as a related/derived version. Same-name pipelines are auto-linked. Use this for cross-name forks (e.g., a similar pipeline with a different name).
    forked_from: Option<i32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct InstallPluginParams {
    /// Plugin name to install from the registry
    plugin_name: String,
}

// ── Server ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AutoPipeServer {
    registry: RegistryClient,
    config: AppConfig,
    tool_router: ToolRouter<Self>,
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Escape a string for safe use inside single-quoted shell arguments.
/// Rejects null bytes and newlines (which can break out of commands),
/// then replaces each `'` with `'\''` (end quote, escaped quote, start quote).
fn shell_escape(s: &str) -> String {
    // Strip characters that can break shell command boundaries
    let sanitized: String = s.chars()
        .filter(|c| *c != '\0' && *c != '\n' && *c != '\r')
        .collect();
    sanitized.replace('\'', "'\\''")
}

/// Normalize paths in config.yaml and Snakefile: replace absolute host paths
/// with Docker mount points (/input, /output).
fn normalize_paths(content: &str) -> String {
    let mut result = String::new();
    for line in content.lines() {
        let normalized = normalize_path_in_line(line);
        result.push_str(&normalized);
        result.push('\n');
    }
    // Remove trailing newline if original didn't have one
    if !content.ends_with('\n') {
        result.pop();
    }
    result
}

fn normalize_path_in_line(line: &str) -> String {
    // Skip comments
    if line.trim_start().starts_with('#') {
        return line.to_string();
    }
    let result = line.to_string();
    if let Some(colon_pos) = result.find(':') {
        let key = result[..colon_pos].trim().to_lowercase();
        let value = result[colon_pos + 1..].trim().to_string();
        // Normalize input-related paths
        if key.contains("input") && !key.contains("format") {
            if let Some(path) = extract_absolute_path(&value) {
                return result.replace(&path, "/input");
            }
        }
        // Normalize output-related paths
        if key.contains("output") && !key.contains("format") {
            if let Some(path) = extract_absolute_path(&value) {
                return result.replace(&path, "/output");
            }
        }
    }
    result
}

/// Extract an absolute path from a YAML value (quoted or unquoted).
fn extract_absolute_path(value: &str) -> Option<String> {
    let v = value.trim().trim_matches('"').trim_matches('\'');
    if v.starts_with('/') && v.len() > 1 && !v.starts_with("/input") && !v.starts_with("/output") && !v.starts_with("/pipeline") {
        Some(v.to_string())
    } else {
        None
    }
}

/// Parse a GitHub URL into (owner, repo, path).
/// Supports formats like:
///   https://github.com/{owner}/{repo}/tree/{branch}/{path}
///   https://github.com/{owner}/{repo}
/// Parse a GitHub URL into (owner, repo, branch_or_tag, subpath).
fn parse_github_url(url: &str) -> Option<(String, String, Option<String>, String)> {
    let url = url.trim().trim_end_matches('/');
    let url = url.strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))?;
    let parts: Vec<&str> = url.splitn(4, '/').collect();
    if parts.len() < 2 {
        return None;
    }
    let owner = parts[0].to_string();
    let repo = parts[1].to_string();
    // Extract branch and path from tree/{branch}/{path}
    if parts.len() >= 4 && parts[2] == "tree" {
        let rest = parts[3];
        if let Some(slash_pos) = rest.find('/') {
            let branch = rest[..slash_pos].to_string();
            let path = rest[slash_pos + 1..].to_string();
            Some((owner, repo, Some(branch), path))
        } else {
            Some((owner, repo, Some(rest.to_string()), String::new()))
        }
    } else {
        Some((owner, repo, None, String::new()))
    }
}

/// Fetch a single file from GitHub Contents API.
async fn fetch_github_file(client: &reqwest::Client, owner: &str, repo: &str, path: &str) -> Option<String> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/contents/{}",
        owner, repo, path
    );
    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github.raw")
        .header("User-Agent", "autopipe-desktop")
        .send()
        .await
        .ok()?;
    if resp.status().is_success() {
        resp.text().await.ok()
    } else {
        None
    }
}

// ── SSH helper methods ──────────────────────────────────────────────

impl AutoPipeServer {
    async fn ssh_run(&self, cmd: &str) -> Result<(String, i32), String> {
        let config = self.config.clone();
        let cmd = cmd.to_string();
        let (output, code) = tokio::task::spawn_blocking(move || ssh::ssh_exec(&config, &cmd))
            .await
            .map_err(|e| format!("Task error: {}", e))??;
        Ok((clean_content(&output), code))
    }

    async fn ssh_read_file(&self, path: &str) -> Result<String, String> {
        let cmd = format!("cat '{}'", shell_escape(path));
        match self.ssh_run(&cmd).await {
            Ok((output, 0)) => Ok(output),
            Ok((output, _)) => Err(format!("Failed to read: {}", output.trim())),
            Err(e) => Err(e),
        }
    }

    /// Download a remote file via SSH exec + base64 encoding (more reliable than SCP).
    /// Returns the file size in bytes on success.
    async fn ssh_download_base64(&self, remote_path: &str, local_path: &str) -> Result<usize, String> {
        use base64::Engine;
        let cmd = format!("base64 '{}'", shell_escape(remote_path));
        let (b64_output, code) = self.ssh_run(&cmd).await?;
        if code != 0 {
            return Err(format!("Remote base64 failed: {}", b64_output.trim()));
        }
        // Remove whitespace from base64 output (line breaks etc.)
        let clean: String = b64_output.chars().filter(|c| !c.is_whitespace()).collect();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&clean)
            .map_err(|e| format!("Base64 decode error: {}", e))?;
        let size = bytes.len();
        std::fs::write(local_path, &bytes)
            .map_err(|e| format!("Cannot write '{}': {}", local_path, e))?;
        Ok(size)
    }

    async fn ssh_write_file(&self, path: &str, content: &str) -> Result<(), String> {
        // Use base64 encoding to safely transfer arbitrary content without heredoc injection
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(content.as_bytes());
        let cmd = format!(
            "echo '{}' | base64 -d > '{}'",
            shell_escape(&encoded), shell_escape(path)
        );
        match self.ssh_run(&cmd).await {
            Ok((_, 0)) => Ok(()),
            Ok((output, _)) => Err(format!("Write failed: {}", output.trim())),
            Err(e) => Err(e),
        }
    }

    /// Resolve the output directory for a run. Always uses {configured_output_dir}/{run_name}.
    fn resolve_output_dir(&self, run_name: &str) -> String {
        format!(
            "{}/{}",
            self.config.full_output_dir().trim_end_matches('/'),
            run_name
        )
    }

    /// Find symlink targets inside a directory and return extra Docker -v mounts.
    async fn resolve_symlink_mounts(&self, dir: &str) -> String {
        let cmd = format!(
            "find '{}' -maxdepth 3 -type l -exec readlink -f '{{}}' \\; 2>/dev/null | xargs -I{{}} dirname '{{}}' | sort -u",
            shell_escape(dir)
        );
        let dirs = match self.ssh_run(&cmd).await {
            Ok((output, 0)) => clean_content(&output),
            _ => return String::new(),
        };

        let mut mounts = String::new();
        for target_dir in dirs.trim().lines() {
            let target_dir = target_dir.trim();
            if target_dir.is_empty() || target_dir == dir || !target_dir.starts_with('/') {
                continue;
            }
            mounts.push_str(&format!(" -v '{}:{}:ro'", shell_escape(target_dir), shell_escape(target_dir)));
        }
        mounts
    }




    async fn find_pipeline_dir(&self, image_name: &str) -> Option<String> {
        let pipeline_name = image_name.strip_prefix("autopipe-").unwrap_or(image_name);

        let pipelines_base = self.config.full_pipelines_dir();
        let candidate = format!(
            "{}/{}",
            pipelines_base.trim_end_matches('/'),
            pipeline_name
        );
        if let Ok((output, 0)) = self
            .ssh_run(&format!("test -d '{}' && echo 'exists'", shell_escape(&candidate)))
            .await
        {
            if output.trim().contains("exists") {
                return Some(candidate);
            }
        }

        let output_base = self.config.full_output_dir();
        let candidate = format!(
            "{}/{}/{}",
            output_base.trim_end_matches('/'),
            pipeline_name,
            pipeline_name
        );
        if let Ok((output, 0)) = self
            .ssh_run(&format!("test -d '{}' && echo 'exists'", shell_escape(&candidate)))
            .await
        {
            if output.trim().contains("exists") {
                return Some(candidate);
            }
        }

        None
    }
}

#[tool_router]
impl AutoPipeServer {
    pub fn new(config: AppConfig) -> Self {
        let registry = RegistryClient::new(&config.registry_url);
        Self {
            registry,
            config,
            tool_router: Self::tool_router(),
        }
    }

    // ── Workspace info ─────────────────────────────────────────

    #[tool(description = "Get the configured workspace paths on the remote SSH server. Call this first to understand where pipelines and outputs are stored.")]
    async fn get_workspace_info(&self) -> Result<CallToolResult, ErrorData> {
        let info = format!(
            "Workspace Configuration:\n\
             - Base path (repo_path): {}\n\
             - Pipelines directory: {}\n\
             - Output directory: {}\n\
             - SSH: {}@{}:{}\n\n\
             When creating pipelines, save files under the Pipelines directory.\n\
             When executing pipelines, outputs are automatically stored under the output directory.\n\
             To view result files, use list_files and read_file directly on the output path.\n\
             To link data, use create_symlink instead of copying files.",
            if self.config.repo_path.is_empty() {
                "(not set)"
            } else {
                &self.config.repo_path
            },
            self.config.full_pipelines_dir(),
            self.config.full_output_dir(),
            self.config.ssh_user,
            self.config.ssh_host,
            self.config.ssh_port,
        );
        Ok(CallToolResult::success(vec![Content::text(info)]))
    }

    // ── Pipeline registry tools ─────────────────────────────────

    #[tool(description = "Search pipelines by keyword in the registry")]
    async fn search_pipelines(
        &self,
        Parameters(params): Parameters<SearchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.registry.search(&params.query).await {
            Ok(results) => {
                let text = serde_json::to_string_pretty(&results).unwrap_or_default();
                Ok(CallToolResult::success(vec![Content::text(text)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Search failed: {}",
                e
            ))])),
        }
    }

    #[tool(description = "List all pipelines in the registry")]
    async fn list_pipelines(&self) -> Result<CallToolResult, ErrorData> {
        match self.registry.list().await {
            Ok(results) => {
                let text = serde_json::to_string_pretty(&results).unwrap_or_default();
                Ok(CallToolResult::success(vec![Content::text(text)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "List failed: {}",
                e
            ))])),
        }
    }

    #[tool(description = "Download a pipeline by ID from the registry and save it to the remote SSH server. Fetches pipeline files from its GitHub repository. If output_dir is omitted, saves to the configured pipelines directory.")]
    async fn download_pipeline(
        &self,
        Parameters(params): Parameters<DownloadParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // 1. Get pipeline metadata from registry (includes github_url)
        let pipeline = match self.registry.get_pipeline(params.pipeline_id).await {
            Ok(p) => p,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Failed to get pipeline: {}",
                    e
                ))]));
            }
        };

        // 2. Parse GitHub URL
        let (owner, repo, _branch, path) = match parse_github_url(&pipeline.github_url) {
            Some(parsed) => parsed,
            None => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid GitHub URL: {}",
                    pipeline.github_url
                ))]));
            }
        };

        // 3. Create directory on remote server
        let base_dir = params
            .output_dir
            .unwrap_or_else(|| self.config.full_pipelines_dir());
        let dir = format!(
            "{}/{}",
            base_dir.trim_end_matches('/'),
            pipeline.name
        );

        match self.ssh_run(&format!("mkdir -p '{}'", shell_escape(&dir))).await {
            Ok((_, 0)) => {}
            Ok((output, _)) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Cannot create directory '{}': {}",
                    dir,
                    output.trim()
                ))]));
            }
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "SSH error: {}",
                    e
                ))]));
            }
        }

        // 4. Fetch files from GitHub and write to SSH
        let client = reqwest::Client::new();
        let file_names = ["Snakefile", "Dockerfile", "config.yaml", "ro-crate-metadata.json", "README.md"];
        let mut written = Vec::new();

        for filename in &file_names {
            let file_path = if path.is_empty() {
                filename.to_string()
            } else {
                format!("{}/{}", path, filename)
            };

            if let Some(content) = fetch_github_file(&client, &owner, &repo, &file_path).await {
                let remote_path = format!("{}/{}", dir, filename);
                if let Err(e) = self.ssh_write_file(&remote_path, &content).await {
                    return Ok(CallToolResult::error(vec![Content::text(format!(
                        "Failed to write {}: {}",
                        filename, e
                    ))]));
                }
                written.push(*filename);
            }
        }

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Downloaded pipeline '{}' to {} (remote server)\nFiles: {}",
            pipeline.name, dir, written.join(", ")
        ))]))
    }

    #[tool(description = "Upload a pipeline to GitHub by committing files to the user's autopipe-pipelines repository. Requires GitHub login (configured in the GitHub tab). Returns the GitHub commit URL.")]
    async fn upload_workflow(
        &self,
        Parameters(params): Parameters<UploadWorkflowParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let token = match &self.config.github_token {
            Some(t) if !t.is_empty() => t.clone(),
            _ => {
                return Ok(CallToolResult::error(vec![Content::text(
                    "GitHub token not configured. Please login via the GitHub tab in the desktop app first.",
                )]));
            }
        };

        let dir = &params.pipeline_dir;

        // Read pipeline files from SSH
        let meta_raw = match self.ssh_read_file(&format!("{}/ro-crate-metadata.json", dir)).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Cannot read ro-crate-metadata.json: {}", e
                ))]));
            }
        };
        let cleaned_meta = clean_content(&meta_raw);
        let metadata: PipelineMetadata = match parse_ro_crate_metadata(&cleaned_meta) {
            Ok(m) => m,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid ro-crate-metadata.json: {}", e
                ))]));
            }
        };

        let snakefile = normalize_paths(&clean_content(
            &self.ssh_read_file(&format!("{}/Snakefile", dir)).await.unwrap_or_default(),
        ));
        let dockerfile = clean_content(
            &self.ssh_read_file(&format!("{}/Dockerfile", dir)).await.unwrap_or_default(),
        );
        let config_yaml = normalize_paths(&clean_content(
            &self.ssh_read_file(&format!("{}/config.yaml", dir)).await.unwrap_or_default(),
        ));
        let readme = clean_content(
            &self.ssh_read_file(&format!("{}/README.md", dir)).await.unwrap_or_default(),
        );

        // Update version in metadata if provided
        let mut meta_json: serde_json::Value = serde_json::from_str(&cleaned_meta).unwrap_or_default();
        if let Some(ref ver) = params.version {
            meta_json["version"] = serde_json::Value::String(ver.clone());
        }
        let metadata_json_str = serde_json::to_string_pretty(&meta_json).unwrap_or_default();

        let pipeline_name = &metadata.name;
        let repo_name = &self.config.github_repo;

        let client = reqwest::Client::new();

        // 1. Get GitHub username
        let user_resp = client
            .get("https://api.github.com/user")
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "autopipe-desktop")
            .send()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let user: serde_json::Value = user_resp.json().await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let owner = user["login"].as_str().unwrap_or_default().to_string();

        // 2. Ensure repo exists
        let repo_check = client
            .get(format!("https://api.github.com/repos/{}/{}", owner, repo_name))
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "autopipe-desktop")
            .send()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        if repo_check.status() == reqwest::StatusCode::NOT_FOUND {
            let create_resp = client
                .post("https://api.github.com/user/repos")
                .header("Authorization", format!("Bearer {}", token))
                .header("User-Agent", "autopipe-desktop")
                .json(&serde_json::json!({
                    "name": repo_name,
                    "description": "AutoPipe bioinformatics pipelines and plugins",
                    "auto_init": true
                }))
                .send()
                .await
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

            if !create_resp.status().is_success() {
                let err_text = create_resp.text().await.unwrap_or_default();
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Failed to create GitHub repo: {}", err_text
                ))]));
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }

        // 3. Get latest commit SHA on main branch
        let ref_resp = client
            .get(format!(
                "https://api.github.com/repos/{}/{}/git/ref/heads/main",
                owner, repo_name
            ))
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "autopipe-desktop")
            .send()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let ref_body: serde_json::Value = ref_resp.json().await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let latest_sha = ref_body["object"]["sha"].as_str().unwrap_or_default().to_string();

        if latest_sha.is_empty() {
            return Ok(CallToolResult::error(vec![Content::text(
                "Could not get latest commit SHA from the repository.",
            )]));
        }

        // 4. Get base tree SHA
        let commit_resp = client
            .get(format!(
                "https://api.github.com/repos/{}/{}/git/commits/{}",
                owner, repo_name, latest_sha
            ))
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "autopipe-desktop")
            .send()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let commit_body: serde_json::Value = commit_resp.json().await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let base_tree = commit_body["tree"]["sha"].as_str().unwrap_or_default().to_string();

        // 5. Create tree with pipeline files
        let files_to_commit: Vec<(&str, &str)> = vec![
            ("Snakefile", &snakefile),
            ("Dockerfile", &dockerfile),
            ("config.yaml", &config_yaml),
            ("ro-crate-metadata.json", &metadata_json_str),
            ("README.md", &readme),
        ];

        let tree_items: Vec<serde_json::Value> = files_to_commit
            .iter()
            .filter(|(_, content)| !content.is_empty())
            .map(|(name, content)| {
                serde_json::json!({
                    "path": format!("pipelines/{}/{}", pipeline_name, name),
                    "mode": "100644",
                    "type": "blob",
                    "content": content
                })
            })
            .collect();

        let tree_resp = client
            .post(format!(
                "https://api.github.com/repos/{}/{}/git/trees",
                owner, repo_name
            ))
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "autopipe-desktop")
            .json(&serde_json::json!({
                "base_tree": base_tree,
                "tree": tree_items
            }))
            .send()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let tree_body: serde_json::Value = tree_resp.json().await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let new_tree_sha = tree_body["sha"].as_str().unwrap_or_default().to_string();

        // 6. Create commit
        let commit_msg = params.commit_message.unwrap_or_else(|| {
            if let Some(ref ver) = params.version {
                format!("Upload {} v{}", pipeline_name, ver)
            } else {
                format!("Upload {}", pipeline_name)
            }
        });

        let new_commit_resp = client
            .post(format!(
                "https://api.github.com/repos/{}/{}/git/commits",
                owner, repo_name
            ))
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "autopipe-desktop")
            .json(&serde_json::json!({
                "message": commit_msg,
                "tree": new_tree_sha,
                "parents": [latest_sha]
            }))
            .send()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let new_commit_body: serde_json::Value = new_commit_resp.json().await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let new_commit_sha = new_commit_body["sha"].as_str().unwrap_or_default().to_string();

        // 7. Update ref to point to new commit
        let update_ref = client
            .patch(format!(
                "https://api.github.com/repos/{}/{}/git/refs/heads/main",
                owner, repo_name
            ))
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "autopipe-desktop")
            .json(&serde_json::json!({ "sha": new_commit_sha }))
            .send()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        if !update_ref.status().is_success() {
            let err = update_ref.text().await.unwrap_or_default();
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to update branch ref: {}", err
            ))]));
        }

        // 8. Create version tag if version is provided
        if let Some(ref ver) = params.version {
            let tag_name = format!("{}/v{}", pipeline_name, ver);
            let _ = client
                .post(format!(
                    "https://api.github.com/repos/{}/{}/git/refs",
                    owner, repo_name
                ))
                .header("Authorization", format!("Bearer {}", token))
                .header("User-Agent", "autopipe-desktop")
                .json(&serde_json::json!({
                    "ref": format!("refs/tags/{}", tag_name),
                    "sha": new_commit_sha
                }))
                .send()
                .await;
        }

        let github_url = format!(
            "https://github.com/{}/{}/tree/main/pipelines/{}",
            owner, repo_name, pipeline_name
        );
        let commit_url = format!(
            "https://github.com/{}/{}/commit/{}",
            owner, repo_name, new_commit_sha
        );

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Successfully uploaded '{}' to GitHub!\n\
             Pipeline URL: {}\n\
             Commit: {}\n\
             {}",
            pipeline_name,
            github_url,
            commit_url,
            if let Some(ref ver) = params.version {
                format!("Version tag: {}/v{}", pipeline_name, ver)
            } else {
                String::new()
            }
        ))]))
    }

    #[tool(description = "Publish a pipeline from GitHub to the AutoPipe registry web page. The pipeline must be uploaded to GitHub first (via upload_workflow). This performs security validation and makes the pipeline publicly visible on the registry website. IMPORTANT: Before publishing, ALWAYS search the registry first using search_pipelines with both the pipeline name and key tool names. Compare the content (tools, description, analysis type) of search results against the new pipeline. VERSION UPGRADE: If a similar pipeline exists AND the author matches yours, ask the user: '기존 파이프라인 [name] v[version]의 버전업으로 등록할까요?'. If yes, set forked_from to that pipeline_id (same name will be kept by the server). If no, omit forked_from. FORK (Based on): If a similar pipeline exists but by a DIFFERENT author, inform the user: '레지스트리에 [author]님의 [name] v[version] 파이프라인과 유사합니다. Based on으로 등록하겠습니다.' and set forked_from to that pipeline_id. The user can choose any name freely. NAME DEDUP: If forked_from is omitted and the name already exists, the server auto-appends a numeric suffix (e.g. 'name 2').")]
    async fn publish_workflow(
        &self,
        Parameters(params): Parameters<PublishWorkflowParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let token = match &self.config.github_token {
            Some(t) if !t.is_empty() => t.clone(),
            _ => {
                return Ok(CallToolResult::error(vec![Content::text(
                    "GitHub token not configured. Please login via the GitHub tab in the desktop app first.",
                )]));
            }
        };

        // Call registry publish endpoint with just github_url + token
        let base = self.config.registry_url.trim_end_matches('/');
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/api/publish", base))
            .json(&serde_json::json!({
                "github_url": params.github_url,
                "github_token": token,
                "forked_from": params.forked_from,
            }))
            .send()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let status = resp.status();
        let body: serde_json::Value = resp.json().await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        if status.is_success() {
            let pipeline_id = body["pipeline_id"].as_i64().unwrap_or(0);
            let name = body["name"].as_str().unwrap_or("unknown");
            let web_url = format!("{}/pipelines/{}", base, pipeline_id);
            Ok(CallToolResult::success(vec![Content::text(format!(
                "Successfully published '{}' to the registry!\n\
                 Web page: {}\n\
                 Pipeline ID: {}",
                name, web_url, pipeline_id
            ))]))
        } else if status.as_u16() == 422 {
            let issues = body["issues"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .map(|i| {
                            format!(
                                "- [{}] {} (line {}, {})",
                                i["severity"].as_str().unwrap_or("?"),
                                i["message"].as_str().unwrap_or(""),
                                i["line"].as_u64().unwrap_or(0),
                                i["file"].as_str().unwrap_or("")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_else(|| body["error"].as_str().unwrap_or("Unknown error").to_string());

            Ok(CallToolResult::error(vec![Content::text(format!(
                "Security validation failed. Please fix the following issues:\n{}",
                issues
            ))]))
        } else {
            let error_msg = body["error"].as_str().unwrap_or("Unknown error");
            Ok(CallToolResult::error(vec![Content::text(format!(
                "Publish failed: {}", error_msg
            ))]))
        }
    }

    #[tool(description = "Validate a pipeline directory structure on the remote SSH server")]
    async fn validate_pipeline(
        &self,
        Parameters(params): Parameters<PipelineDirParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dir = &params.pipeline_dir;
        let mut errors: Vec<String> = Vec::new();
        let required = [
            "Snakefile",
            "Dockerfile",
            "config.yaml",
            "ro-crate-metadata.json",
            "README.md",
        ];

        for f in &required {
            let path = format!("{}/{}", dir, f);
            match self.ssh_read_file(&path).await {
                Ok(raw) if raw.is_empty() => errors.push(format!("Empty: {}", f)),
                Err(_) => errors.push(format!("Missing: {}", f)),
                Ok(raw) => {
                    let content = clean_content(&raw);
                    if *f == "Snakefile" && !content.contains("rule all") {
                        errors.push("Snakefile: missing 'rule all'".into());
                    }
                    if *f == "ro-crate-metadata.json" {
                        match parse_ro_crate_metadata(&content) {
                            Ok(m) => {
                                if m.name.is_empty() {
                                    errors.push("ro-crate-metadata.json: 'name' is empty".into());
                                }
                                if m.tools.is_empty() {
                                    errors.push("ro-crate-metadata.json: 'tools' is empty".into());
                                }
                            }
                            Err(e) => {
                                errors.push(format!("ro-crate-metadata.json: invalid - {}", e))
                            }
                        }
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(CallToolResult::success(vec![Content::text(
                "Validation passed. All files present and valid.",
            )]))
        } else {
            Ok(CallToolResult::error(vec![Content::text(format!(
                "Validation errors:\n{}",
                errors.join("\n")
            ))]))
        }
    }

    // ── Execution tools (via SSH) ───────────────────────────────

    #[tool(description = "Build a Docker image for a pipeline on the remote server via SSH. The build runs in the background and returns immediately. After calling this, automatically call check_build_status every 10 seconds until the build completes. Do NOT ask the user to check — poll automatically. If the build fails, analyze the log, call cleanup_failed, fix the pipeline, and retry.")]
    async fn build_image(
        &self,
        Parameters(params): Parameters<BuildParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let log_path = format!("{}/build_{}.log", params.pipeline_dir, params.image_name);

        let cmd = format!(
            "cd '{}' && nohup docker build -t '{}' . > '{}' 2>&1 &\necho $!",
            shell_escape(&params.pipeline_dir), shell_escape(&params.image_name), shell_escape(&log_path)
        );

        match self.ssh_run(&cmd).await {
            Ok((output, 0)) => {
                let pid = output.trim();
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Docker build started in background (PID: {}).\nLog: {}\nNow call check_build_status with image_name='{}' every 10 seconds to monitor progress.",
                    pid, log_path, params.image_name
                ))]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
            _ => Ok(CallToolResult::error(vec![Content::text("Failed to start build")])),
        }
    }

    #[tool(description = "Check the status of a background Docker build started by build_image. Returns building/success/failed status with recent log output. Call this automatically every 10 seconds after build_image — do NOT wait for the user to ask.")]
    async fn check_build_status(
        &self,
        Parameters(params): Parameters<CheckBuildParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Check if docker build process is still running
        let check_cmd = format!(
            "ps aux | grep 'docker build.*{}' | grep -v grep | head -1",
            shell_escape(&params.image_name)
        );
        let is_running = match self.ssh_run(&check_cmd).await {
            Ok((output, _)) => !output.trim().is_empty(),
            Err(_) => false,
        };

        // Check if image exists (build succeeded)
        let image_check = format!("docker images -q '{}' 2>/dev/null", shell_escape(&params.image_name));
        let image_exists = match self.ssh_run(&image_check).await {
            Ok((output, 0)) => !output.trim().is_empty(),
            _ => false,
        };

        // Get recent log output
        let pipelines_dir = self.config.full_pipelines_dir();
        let log_path = format!("{}/build_{}.log", pipelines_dir, params.image_name);
        // Also check in pipeline subdirectories
        let find_log = format!(
            "find '{}' -name 'build_{}.log' 2>/dev/null | head -1",
            shell_escape(&pipelines_dir), shell_escape(&params.image_name)
        );
        let actual_log_path = match self.ssh_run(&find_log).await {
            Ok((output, 0)) if !output.trim().is_empty() => output.trim().to_string(),
            _ => log_path,
        };

        let tail_cmd = format!("tail -30 '{}' 2>/dev/null", shell_escape(&actual_log_path));
        let recent_log = match self.ssh_run(&tail_cmd).await {
            Ok((output, _)) => output,
            Err(_) => "Log not found".to_string(),
        };

        if is_running {
            Ok(CallToolResult::success(vec![Content::text(format!(
                "Build in progress...\n\nRecent log:\n{}",
                recent_log
            ))]))
        } else if image_exists {
            // Clean up build log
            let _ = self.ssh_run(&format!("rm -f '{}'", shell_escape(&actual_log_path))).await;
            Ok(CallToolResult::success(vec![Content::text(format!(
                "Build completed successfully! Image '{}' is ready.\n\nFinal log:\n{}",
                params.image_name, recent_log
            ))]))
        } else {
            Ok(CallToolResult::error(vec![Content::text(format!(
                "Build failed.\n\nBuild log:\n{}\n\nNext steps: Analyze the error above, call cleanup_failed to remove broken artifacts, fix the pipeline code, then retry build_image.",
                recent_log
            ))]))
        }
    }

    #[tool(description = "Dry-run a pipeline (snakemake -n -p) on the remote server via SSH. If output_dir is omitted, uses the configured output directory.")]
    async fn dry_run(
        &self,
        Parameters(params): Parameters<DryRunParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let cores = params.cores.unwrap_or(8);
        let output_dir = params
            .output_dir
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.config.full_output_dir());

        let pipeline_mount = match self.find_pipeline_dir(&params.image_name).await {
            Some(dir) => format!("-v '{}:/pipeline' -w /pipeline", shell_escape(&dir)),
            None => String::new(),
        };

        let symlink_mounts = self.resolve_symlink_mounts(&params.input_dir).await;

        let cmd = format!(
            "docker run --rm --entrypoint snakemake {} -v '{}:/input:ro'{} -v '{}:/output' '{}' --cores {} --snakefile /pipeline/Snakefile --configfile /pipeline/config.yaml -n -p",
            pipeline_mount, shell_escape(&params.input_dir), symlink_mounts, shell_escape(&output_dir), shell_escape(&params.image_name), cores
        );

        match self.ssh_run(&cmd).await {
            Ok((output, 0)) => Ok(CallToolResult::success(vec![Content::text(output)])),
            Ok((output, _)) => Ok(CallToolResult::error(vec![Content::text(output)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(description = "Execute a pipeline in the background on the remote server via SSH. Outputs are stored at {configured_output_dir}/{run_name}/. Logs are written to {output_dir}/{run_name}/pipeline.log. This tool monitors the first ~90 seconds for early failures before returning. If the pipeline fails (check via check_status or list_running_pipelines), analyze the log, call cleanup_failed to remove the failed output directory and Docker image, fix the pipeline code, rebuild, and retry. Tell the user they can check progress later with list_running_pipelines, even from a new conversation session.")]
    async fn execute_pipeline(
        &self,
        Parameters(params): Parameters<ExecuteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let cores = params.cores.unwrap_or(8);
        let output_dir = self.resolve_output_dir(&params.run_name);
        let container_name = format!("{}-run", params.run_name);
        let log_path = format!("{}/pipeline.log", output_dir.trim_end_matches('/'));

        let pipeline_mount = match self.find_pipeline_dir(&params.image_name).await {
            Some(dir) => format!("-v '{}:/pipeline' -w /pipeline", shell_escape(&dir)),
            None => String::new(),
        };

        let symlink_mounts = self.resolve_symlink_mounts(&params.input_dir).await;

        let _ = self.ssh_run(&format!("docker rm -f '{}' 2>/dev/null", shell_escape(&container_name))).await;
        let _ = self.ssh_run(&format!("mkdir -p '{}'", shell_escape(&output_dir))).await;

        // Write run metadata for list_running_pipelines (use base64 to avoid heredoc injection)
        let run_meta = serde_json::json!({
            "run_name": params.run_name,
            "image_name": params.image_name,
            "container_name": container_name,
            "input_dir": params.input_dir,
            "started_at": chrono::Utc::now().to_rfc3339()
        }).to_string();
        let meta_path = format!("{}/.autopipe-run.json", output_dir.trim_end_matches('/'));
        let _ = self.ssh_write_file(&meta_path, &run_meta).await;

        let cmd = format!(
            "nohup docker run --rm --entrypoint snakemake --name '{}' {} -v '{}:/input:ro'{} -v '{}:/output' '{}' --cores {} --snakefile /pipeline/Snakefile --configfile /pipeline/config.yaml > '{}' 2>&1 &\necho $!",
            shell_escape(&container_name), pipeline_mount, shell_escape(&params.input_dir), symlink_mounts, shell_escape(&output_dir), shell_escape(&params.image_name), cores, shell_escape(&log_path)
        );

        match self.ssh_run(&cmd).await {
            Ok((output, 0)) => {
                let pid = output.trim().lines().last().unwrap_or("unknown");

                // Monitor first ~90 seconds for early failures (check at 10s, 30s, 60s, 90s)
                let check_intervals = [10u64, 20, 30, 30];
                for wait_secs in check_intervals {
                    tokio::time::sleep(std::time::Duration::from_secs(wait_secs)).await;

                    let still_running = match self.ssh_run(&format!(
                        "docker inspect -f '{{{{.State.Running}}}}' '{}' 2>/dev/null", shell_escape(&container_name)
                    )).await {
                        Ok((out, 0)) => out.trim() == "true",
                        _ => false,
                    };

                    if !still_running {
                        // Container exited — check if it succeeded or failed
                        let log_tail = match self.ssh_run(&format!("tail -30 '{}' 2>/dev/null", shell_escape(&log_path))).await {
                            Ok((out, 0)) => out,
                            _ => "(no log available)".to_string(),
                        };

                        let has_error = log_tail.contains("Error") || log_tail.contains("error")
                            || log_tail.contains("FAILED") || log_tail.contains("failed")
                            || log_tail.contains("Exiting because a job execution failed");
                        let completed_ok = log_tail.contains("steps (100%) done")
                            || log_tail.contains("Nothing to be done");

                        if completed_ok {
                            return Ok(CallToolResult::success(vec![Content::text(format!(
                                "Pipeline completed successfully!\n\
                                 Output directory: {}\n\
                                 Log: {}\n\n{}",
                                output_dir, log_path, log_tail
                            ))]));
                        } else if has_error {
                            return Ok(CallToolResult::error(vec![Content::text(format!(
                                "Pipeline FAILED early (within 90s). Analyze the log and fix the issue.\n\
                                 Container: {}\n\
                                 Output directory: {}\n\
                                 Log: {}\n\n{}",
                                container_name, output_dir, log_path, log_tail
                            ))]));
                        }
                    }
                }

                // Still running after 90s — no early errors detected
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Pipeline is running (no errors in first 90s). PID: {}, container: '{}'.\n\
                     Output directory: {}\n\
                     Log file: {}\n\
                     The user can check progress anytime (even in a new session) with list_running_pipelines.",
                    pid, container_name, output_dir, log_path
                ))]))
            }
            Ok((output, _)) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to start pipeline:\n{}",
                output
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(description = "List all pipeline runs (running, completed, or failed). Scans the output directory for .autopipe-run.json metadata files and checks container status. No parameters needed — call this when the user asks about pipeline status, progress, or running jobs.")]
    async fn list_running_pipelines(&self) -> Result<CallToolResult, ErrorData> {
        let output_base = self.config.full_output_dir();

        // Find all run metadata files (use glob instead of find -exec to avoid {} escaping issues over SSH)
        let find_cmd = format!(
            "for f in '{}'/*/.autopipe-run.json; do [ -f \"$f\" ] && cat \"$f\"; done 2>/dev/null",
            shell_escape(&output_base)
        );
        let meta_output = match self.ssh_run(&find_cmd).await {
            Ok((output, _)) => output,
            Err(e) => return Ok(CallToolResult::error(vec![Content::text(format!("Cannot scan output directory: {}", e))])),
        };

        if meta_output.trim().is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No pipeline runs found in the output directory."
            )]));
        }

        // Get running containers
        let docker_cmd = "docker ps --filter 'name=-run' --format '{{.Names}} {{.Status}}' 2>/dev/null";
        let running_containers = match self.ssh_run(docker_cmd).await {
            Ok((output, _)) => output,
            _ => String::new(),
        };

        let mut results = Vec::new();
        for line in meta_output.lines() {
            let line = line.trim();
            if !line.starts_with('{') { continue; }

            let meta: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let run_name = meta["run_name"].as_str().unwrap_or("unknown");
            let image = meta["image_name"].as_str().unwrap_or("unknown");
            let container = meta["container_name"].as_str().unwrap_or("unknown");
            let started = meta["started_at"].as_str().unwrap_or("unknown");

            // Check if container is running
            let is_running = running_containers.lines().any(|l| l.starts_with(container));
            let docker_status = running_containers.lines()
                .find(|l| l.starts_with(container))
                .unwrap_or("");

            // Read last line of pipeline.log for progress
            let log_path = format!("{}/{}/pipeline.log", output_base.trim_end_matches('/'), run_name);
            let last_line = match self.ssh_run(&format!("tail -3 '{}' 2>/dev/null", shell_escape(&log_path))).await {
                Ok((out, 0)) => {
                    let trimmed = out.trim();
                    if trimmed.len() > 200 { trimmed[..200].to_string() } else { trimmed.to_string() }
                }
                _ => "(no log)".to_string(),
            };

            let status = if is_running {
                format!("RUNNING ({})", docker_status.split_whitespace().skip(1).collect::<Vec<_>>().join(" "))
            } else if last_line.contains("100%) done") || last_line.contains("Nothing to be done") {
                "COMPLETED".to_string()
            } else if last_line.contains("Error") || last_line.contains("failed") || last_line.contains("FAILED") {
                "FAILED".to_string()
            } else {
                "STOPPED".to_string()
            };

            results.push(format!(
                "- {} [{}]\n  Image: {}\n  Started: {}\n  Log: {}",
                run_name, status, image, started, last_line
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Pipeline runs ({}):\n\n{}",
            results.len(),
            results.join("\n\n")
        ))]))
    }

    #[tool(description = "Check pipeline execution status by reading the log file and checking if the process is still running. Uses {configured_output_dir}/{run_name}/.")]
    async fn check_status(
        &self,
        Parameters(params): Parameters<StatusParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let output_dir = self.resolve_output_dir(&params.run_name);
        let container_name = format!("{}-run", params.run_name);
        let log_path = format!("{}/pipeline.log", output_dir.trim_end_matches('/'));

        let running = match self.ssh_run(&format!("docker inspect -f '{{{{.State.Running}}}}' '{}' 2>/dev/null", shell_escape(&container_name))).await {
            Ok((output, 0)) => output.trim() == "true",
            _ => false,
        };

        let log_output = match self.ssh_run(&format!("tail -50 '{}' 2>/dev/null", shell_escape(&log_path))).await {
            Ok((output, 0)) => output,
            Ok((output, _)) => format!("(log not available: {})", output.trim()),
            Err(e) => format!("(cannot read log: {})", e),
        };

        let status_str = if running { "RUNNING" } else { "FINISHED" };

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Status: {}\nContainer: {}\nOutput: {}\nLog ({}):\n{}",
            status_str, container_name, output_dir, log_path, log_output
        ))]))
    }

    // ── Cleanup tools ────────────────────────────────────────────

    #[tool(description = "Clean up artifacts from a failed pipeline build or execution. Removes the failed output directory and the Docker image. Call this ONLY after a build_image failure or after check_status confirms a pipeline execution failed. Steps: (1) checks if the Docker image has running containers — refuses to remove if so, (2) removes the output directory, (3) removes the Docker image and dangling build layers. After cleanup, fix the pipeline code and retry.")]
    async fn cleanup_failed(
        &self,
        Parameters(params): Parameters<CleanupFailedParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let output_dir = self.resolve_output_dir(&params.run_name);
        let mut results = Vec::new();

        // 1. Check if image has running containers
        let running_check = format!(
            "docker ps -q --filter ancestor='{}' 2>/dev/null",
            shell_escape(&params.image_name)
        );
        if let Ok((output, 0)) = self.ssh_run(&running_check).await {
            if !output.trim().is_empty() {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Cannot clean up: image '{}' has running containers. Stop them first.",
                    params.image_name
                ))]));
            }
        }

        // 2. Remove failed output directory
        let check_dir = format!("test -d '{}'", shell_escape(&output_dir));
        if let Ok((_, 0)) = self.ssh_run(&check_dir).await {
            let rm_cmd = format!("rm -rf '{}'", shell_escape(&output_dir));
            match self.ssh_run(&rm_cmd).await {
                Ok((_, 0)) => results.push(format!("Removed output directory: {}", output_dir)),
                Ok((err, _)) => results.push(format!("Failed to remove output directory: {}", err.trim())),
                Err(e) => results.push(format!("Error removing output directory: {}", e)),
            }
        } else {
            results.push(format!("Output directory not found (already clean): {}", output_dir));
        }

        // 3. Remove Docker image
        let check_img = format!(
            "docker images -q '{}' 2>/dev/null",
            shell_escape(&params.image_name)
        );
        if let Ok((output, 0)) = self.ssh_run(&check_img).await {
            if !output.trim().is_empty() {
                let rmi_cmd = format!("docker rmi '{}' 2>/dev/null", shell_escape(&params.image_name));
                match self.ssh_run(&rmi_cmd).await {
                    Ok((_, 0)) => results.push(format!("Removed Docker image: {}", params.image_name)),
                    Ok((err, _)) => results.push(format!("Failed to remove image: {}", err.trim())),
                    Err(e) => results.push(format!("Error removing image: {}", e)),
                }
            } else {
                results.push(format!("Docker image not found (already clean): {}", params.image_name));
            }
        }

        // 4. Prune dangling images from failed builds
        let _ = self.ssh_run("docker image prune -f --filter dangling=true 2>/dev/null").await;
        results.push("Pruned dangling images from incomplete builds.".to_string());

        Ok(CallToolResult::success(vec![Content::text(
            results.join("\n"),
        )]))
    }

    // ── Remote file tools ───────────────────────────────────────

    #[tool(description = "Create a symbolic link on the remote SSH server. Use this to link input/output data instead of copying files. Prefer symlinks over cp for accessing result files and plots.")]
    async fn create_symlink(
        &self,
        Parameters(params): Parameters<CreateSymlinkParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.ssh_run(&format!("test -e '{}' && echo 'exists'", shell_escape(&params.source))).await {
            Ok((output, 0)) if output.trim().contains("exists") => {}
            _ => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Source path '{}' does not exist on remote server",
                    params.source
                ))]));
            }
        }

        if let Some(parent) = std::path::Path::new(&params.target).parent() {
            let _ = self
                .ssh_run(&format!("mkdir -p '{}'", shell_escape(&parent.to_string_lossy())))
                .await;
        }

        match self.ssh_run(&format!("ln -sf '{}' '{}'", shell_escape(&params.source), shell_escape(&params.target))).await {
            Ok((_, 0)) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Symlink created: {} -> {}",
                params.target, params.source
            ))])),
            Ok((output, _)) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to create symlink: {}",
                output.trim()
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(description = "Remove a symbolic link on the remote SSH server")]
    async fn remove_symlink(
        &self,
        Parameters(params): Parameters<RemoveSymlinkParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let cmd = format!(
            "test -L '{}' && rm '{}' && echo 'removed' || echo 'not_a_symlink'",
            shell_escape(&params.symlink_path), shell_escape(&params.symlink_path)
        );
        match self.ssh_run(&cmd).await {
            Ok((output, 0)) if output.trim().contains("removed") => {
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Symlink '{}' removed",
                    params.symlink_path
                ))]))
            }
            Ok((_, 0)) => Ok(CallToolResult::error(vec![Content::text(format!(
                "'{}' is not a symlink or does not exist",
                params.symlink_path
            ))])),
            Ok((output, _)) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed: {}",
                output.trim()
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(description = "List files and directories at a remote path on the SSH server. IMPORTANT: When the user asks to view, check, or see results/output files, follow this workflow: (1) Call list_files to see what files exist in the output directory. (2) Show the file list to the user and ask: 'Would you like to open the viewer, or see the contents in chat?' (3) If viewer: call show_results. If chat: call read_file for the requested file.")]
    async fn list_files(
        &self,
        Parameters(params): Parameters<ListFilesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.ssh_run(&format!("ls -la '{}'", shell_escape(&params.path))).await {
            Ok((output, 0)) => Ok(CallToolResult::success(vec![Content::text(output)])),
            Ok((output, _)) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Cannot list '{}': {}",
                params.path,
                output.trim()
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(description = "Read the contents of a file on the remote SSH server and display it in chat. Use this only when the user explicitly chooses to see file contents in chat (not in the viewer). Do NOT call this directly when the user asks to view results — first use list_files, ask the user whether they want the viewer or chat, then proceed accordingly. IMPORTANT: After showing file contents, ALWAYS ask if the user wants to save the file locally. If yes, use download_results.")]
    async fn read_file(
        &self,
        Parameters(params): Parameters<ReadFileParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.ssh_read_file(&params.path).await {
            Ok(content) => Ok(CallToolResult::success(vec![Content::text(content)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Cannot read '{}': {}",
                params.path, e
            ))])),
        }
    }

    #[tool(description = "Download file(s) from the remote SSH server to the user's local machine. Use this when the user wants to save result files locally. If local_dir is omitted, files are saved to the OS default Downloads folder. Tell the user the default path and ask if they want to change it. Supports single files and directories.")]
    async fn download_results(
        &self,
        Parameters(params): Parameters<DownloadResultsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let remote_path = params.remote_path.clone();

        // Resolve local directory: user-specified or OS default Downloads folder
        let local_dir = match &params.local_dir {
            Some(dir) if !dir.is_empty() => dir.clone(),
            _ => {
                match dirs::download_dir() {
                    Some(p) => p.to_string_lossy().to_string(),
                    None => {
                        // Fallback: ~/Downloads
                        match dirs::home_dir() {
                            Some(h) => format!("{}/Downloads", h.to_string_lossy()),
                            None => return Ok(CallToolResult::error(vec![Content::text(
                                "Cannot determine Downloads folder. Please specify local_dir explicitly."
                            )])),
                        }
                    }
                }
            }
        };

        // Ensure local directory exists
        if let Err(e) = std::fs::create_dir_all(&local_dir) {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Cannot create local directory '{}': {}",
                local_dir, e
            ))]));
        }

        // Check if remote path is a directory or file
        let is_dir = match self.ssh_run(&format!("test -d '{}' && echo DIR || echo FILE", shell_escape(&remote_path))).await {
            Ok((output, 0)) => output.trim() == "DIR",
            _ => false,
        };

        if is_dir {
            // List files in the remote directory
            let files = match self.ssh_run(&format!("find '{}' -maxdepth 1 -type f -printf '%f\\n'", shell_escape(&remote_path))).await {
                Ok((output, 0)) => output.trim().to_string(),
                Ok((output, _)) => return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Cannot list directory '{}': {}", remote_path, output.trim()
                ))])),
                Err(e) => return Ok(CallToolResult::error(vec![Content::text(e)])),
            };

            if files.is_empty() {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "No files found in '{}'", remote_path
                ))]));
            }

            let file_list: Vec<&str> = files.lines().collect();
            let mut downloaded = Vec::new();
            let mut errors = Vec::new();

            for file_name in &file_list {
                let remote_file = format!("{}/{}", remote_path.trim_end_matches('/'), file_name);
                let local_file = format!("{}/{}", local_dir.trim_end_matches('/'), file_name);

                match self.ssh_download_base64(&remote_file, &local_file).await {
                    Ok(size) => downloaded.push(format!("  {} ({} bytes)", file_name, size)),
                    Err(e) => errors.push(format!("  {}: {}", file_name, e)),
                }
            }

            let mut msg = format!("Downloaded to: {}\n\n", local_dir);
            if !downloaded.is_empty() {
                msg.push_str(&format!("✓ {} file(s) saved:\n{}\n", downloaded.len(), downloaded.join("\n")));
            }
            if !errors.is_empty() {
                msg.push_str(&format!("\n✗ {} error(s):\n{}", errors.len(), errors.join("\n")));
            }
            Ok(CallToolResult::success(vec![Content::text(msg)]))
        } else {
            // Single file download
            let file_name = std::path::Path::new(&remote_path)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "downloaded_file".to_string());
            let local_file = format!("{}/{}", local_dir.trim_end_matches('/'), file_name);

            match self.ssh_download_base64(&remote_path, &local_file).await {
                Ok(size) => Ok(CallToolResult::success(vec![Content::text(format!(
                    "Downloaded to: {}\n✓ {} ({} bytes)",
                    local_file, file_name, size
                ))])),
                Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                    "Download failed for '{}': {}", remote_path, e
                ))])),
            }
        }
    }

    #[tool(description = "Write content to a file on the remote SSH server. Creates parent directories if needed.")]
    async fn write_file(
        &self,
        Parameters(params): Parameters<WriteFileParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if let Some(parent) = std::path::Path::new(&params.path).parent() {
            let _ = self
                .ssh_run(&format!("mkdir -p '{}'", shell_escape(&parent.to_string_lossy())))
                .await;
        }

        match self.ssh_write_file(&params.path, &params.content).await {
            Ok(()) => Ok(CallToolResult::success(vec![Content::text(format!(
                "File written: {}",
                params.path
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Cannot write '{}': {}",
                params.path, e
            ))])),
        }
    }

    // ── Browser viewer ─────────────────────────────────────────

    #[tool(description = "Open the Results Viewer in a browser. Use this when the user chooses to view results in the viewer (after being asked via list_files workflow). Pass a DIRECTORY path to view all files in it, or a single FILE path to view only that file. The viewer handles ALL file types: images, PDF, text, genomics (BAM/VCF/BED/GFF), HDF5 (h5ad). When the user asks to view a specific file, pass the exact file path — do NOT pass the parent directory. IMPORTANT workflow for genomics files (BAM/VCF/BED/GFF/CRAM/BCF): (1) First call show_results WITHOUT the reference parameter. The viewer will NOT open yet — instead you will receive information about FASTA files in the directory. (2) Ask the user about the reference based on the response. (3) Then call show_results AGAIN: with reference=<fasta_filename> if the user confirmed, with reference=<user_provided_path> if they gave a different path, or with reference=\"none\" if the user has no reference. The viewer only opens on this second call. Without reference, only Data tabs are shown. With reference, both Data and IGV tabs appear. CRAM/BCF files cannot be displayed without a reference.")]
    async fn show_results(
        &self,
        Parameters(params): Parameters<ShowResultsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Check if path is a directory or file
        let is_dir = match self
            .ssh_run(&format!(
                "test -d '{}' && echo DIR || echo FILE",
                shell_escape(&params.path)
            ))
            .await
        {
            Ok((output, 0)) => clean_content(&output).trim() == "DIR",
            _ => false,
        };

        let file_paths: Vec<String> = if is_dir {
            // List files in directory (non-recursive, max 50)
            match self
                .ssh_run(&format!(
                    "find '{}' -maxdepth 1 -type f ! -name 'Dockerfile' ! -name 'Snakefile*' ! -name '*.py' ! -name '*.sh' | head -50",
                    shell_escape(&params.path)
                ))
                .await
            {
                Ok((output, 0)) => clean_content(&output)
                    .trim()
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(|l| l.to_string())
                    .collect(),
                Ok((output, _)) => {
                    return Ok(CallToolResult::error(vec![Content::text(format!(
                        "Cannot list directory '{}': {}",
                        params.path,
                        clean_content(&output).trim()
                    ))]));
                }
                Err(e) => return Ok(CallToolResult::error(vec![Content::text(e)])),
            }
        } else {
            // Single file: check it exists, then load only that file
            match self
                .ssh_run(&format!(
                    "test -f '{}' && echo OK || echo NOT_FOUND",
                    shell_escape(&params.path)
                ))
                .await
            {
                Ok((output, 0)) if clean_content(&output).trim() == "OK" => {
                    vec![params.path.clone()]
                }
                _ => {
                    return Ok(CallToolResult::error(vec![Content::text(format!(
                        "File not found: {}",
                        params.path
                    ))]));
                }
            }
        };

        // Apply file type filter if specified
        let file_paths: Vec<String> = if let Some(ref filter) = params.filter {
            let allowed_exts: Vec<&str> = match filter.to_lowercase().as_str() {
                "image" | "images" => vec!["png", "jpg", "jpeg", "gif", "svg", "webp", "bmp", "tiff", "tif"],
                "text" => vec!["txt", "log", "csv", "tsv", "json", "yaml", "yml", "xml", "md"],
                "genomics" => vec!["bam", "cram", "vcf", "bcf", "bed", "gff", "gtf", "gff3", "fasta", "fa", "fastq", "fq", "bigwig", "bw", "bigbed", "bb"],
                "pdf" => vec!["pdf"],
                "hdf5" => vec!["h5ad", "h5", "hdf5"],
                _ => vec![],
            };
            if allowed_exts.is_empty() {
                file_paths
            } else {
                file_paths
                    .into_iter()
                    .filter(|p| {
                        let ext = p.rsplit('.').next().map(|e| e.to_lowercase()).unwrap_or_default();
                        allowed_exts.contains(&ext.as_str())
                    })
                    .collect()
            }
        } else {
            file_paths
        };

        if file_paths.is_empty() {
            let filter_msg = params.filter.as_deref().unwrap_or("any");
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "No {} files found in '{}'",
                filter_msg, params.path
            ))]));
        }

        // Separate genomics files (remote, server-side pagination) from other files (local transfer)
        let genomics_remote_exts = ["bam", "vcf", "bed", "gff", "gtf", "gff3", "cram", "bcf",
                                     "bai", "crai", "tbi", "csi", "fai", "idx"];
        let mut files: Vec<(String, Vec<u8>, String)> = Vec::new();
        let mut remote_files: Vec<(String, String, u64, String)> = Vec::new(); // (filename, remote_path, size, mime)
        let mut errors: Vec<String> = Vec::new();

        for path in &file_paths {
            let ext = path
                .rsplit('.')
                .next()
                .map(|e| e.to_lowercase())
                .unwrap_or_default();
            let mime = match ext.as_str() {
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
            };

            let filename = std::path::Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string();

            // Genomics files: register as remote (server-side pagination + Range proxy)
            if genomics_remote_exts.contains(&ext.as_str()) {
                let size_cmd = format!("stat -c%s '{}' 2>/dev/null || stat -f%z '{}' 2>/dev/null", shell_escape(path), shell_escape(path));
                let size: u64 = if let Ok((size_str, 0)) = self.ssh_run(&size_cmd).await {
                    clean_content(&size_str).trim().parse().unwrap_or(0)
                } else {
                    0
                };
                remote_files.push((filename, path.clone(), size, mime.to_string()));
                continue;
            }

            // h5ad/h5/hdf5: check file size, skip if > 1GB (download only)
            if matches!(ext.as_str(), "h5ad" | "h5" | "hdf5") {
                let size_cmd = format!("stat -c%s '{}' 2>/dev/null || stat -f%z '{}' 2>/dev/null", shell_escape(path), shell_escape(path));
                if let Ok((size_str, 0)) = self.ssh_run(&size_cmd).await {
                    let size: u64 = clean_content(&size_str).trim().parse().unwrap_or(0);
                    if size > 1_073_741_824 {
                        errors.push(format!("{}: file too large ({:.1} GB) — download only", filename, size as f64 / 1_073_741_824.0));
                        continue;
                    }
                }
            }

            match self
                .ssh_run(&format!("base64 -w 0 '{}'", shell_escape(path)))
                .await
            {
                Ok((b64, 0)) => {
                    let trimmed = clean_content(&b64).trim().to_string();
                    match base64::Engine::decode(
                        &base64::engine::general_purpose::STANDARD,
                        &trimmed,
                    ) {
                        Ok(data) => files.push((filename, data, mime.to_string())),
                        Err(e) => errors.push(format!("{}: decode error: {}", filename, e)),
                    }
                }
                Ok((output, _)) => {
                    errors.push(format!("{}: {}", filename, clean_content(&output).trim()));
                }
                Err(e) => errors.push(format!("{}: {}", filename, e)),
            }
        }

        if files.is_empty() && remote_files.is_empty() {
            let msg = if errors.is_empty() {
                format!("No viewable files found in '{}'", params.path)
            } else {
                format!("Failed to load files:\n{}", errors.join("\n"))
            };
            return Ok(CallToolResult::error(vec![Content::text(msg)]));
        }

        // "none" means user explicitly said no reference → open viewer without IGV
        let user_declined_ref = matches!(
            params.reference.as_deref(),
            Some("none") | Some("None") | Some("no")
        );
        let reference = if user_declined_ref {
            None
        } else {
            params.reference.clone()
        };

        // Check if any file requires a plugin that is not installed
        let plugins_dir_path = self.config.full_plugins_dir();
        let installed_plugin_exts: Vec<String> = {
            let mut exts = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&plugins_dir_path) {
                for entry in entries.flatten() {
                    let manifest_path = entry.path().join("manifest.json");
                    if let Ok(text) = std::fs::read_to_string(&manifest_path) {
                        if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&text) {
                            if let Some(arr) = manifest["extensions"].as_array() {
                                for e in arr {
                                    if let Some(s) = e.as_str() {
                                        exts.push(s.to_lowercase());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            exts
        };
        // Filter out files without a matching viewer plugin (e.g. .bai, .tbi index files)
        let has_viewer = |name: &str| -> bool {
            let ext = name.rsplit('.').next().map(|e| e.to_lowercase()).unwrap_or_default();
            ext.is_empty() || installed_plugin_exts.contains(&ext)
        };
        {
            let mut skipped: Vec<String> = file_paths.iter().filter_map(|p| {
                if has_viewer(p) { None } else {
                    Some(p.rsplit('.').next().unwrap_or("").to_lowercase())
                }
            }).collect();
            skipped.sort();
            skipped.dedup();
            if !skipped.is_empty() {
                let ext_list = skipped.iter().map(|e| format!(".{}", e)).collect::<Vec<_>>().join(", ");
                errors.push(format!("Skipped files with no viewer plugin: {}", ext_list));
            }
        }
        files.retain(|(name, _, _)| has_viewer(name));
        remote_files.retain(|(name, _, _, _)| has_viewer(name));
        if files.is_empty() && remote_files.is_empty() {
            let msg = if errors.is_empty() {
                format!("No viewable files found in '{}'", params.path)
            } else {
                format!("No viewable files found:\n{}", errors.join("\n"))
            };
            return Ok(CallToolResult::error(vec![Content::text(msg)]));
        }

        // Detect genomics files
        let genomics_exts = ["bam", "vcf", "bed", "gff", "gtf", "gff3", "cram", "bcf"];
        let has_genomics = file_paths.iter().any(|p| {
            let ext = p.rsplit('.').next().map(|e| e.to_lowercase()).unwrap_or_default();
            genomics_exts.contains(&ext.as_str())
        });

        // --- Genomics files exist but no reference decision yet → ask first, don't open viewer ---
        if has_genomics && reference.is_none() && !user_declined_ref {
            let fasta_files: Vec<&String> = file_paths.iter().filter(|p| {
                let ext = p.rsplit('.').next().map(|e| e.to_lowercase()).unwrap_or_default();
                matches!(ext.as_str(), "fasta" | "fa" | "fna")
            }).collect();

            let igv_only_files: Vec<String> = file_paths.iter().filter(|p| {
                let ext = p.rsplit('.').next().map(|e| e.to_lowercase()).unwrap_or_default();
                matches!(ext.as_str(), "cram" | "bcf")
            }).map(|p| {
                std::path::Path::new(p.as_str())
                    .file_name().and_then(|n| n.to_str()).unwrap_or("unknown").to_string()
            }).collect();

            let mut msg = String::from("[Reference check] Genomics files detected. The viewer is NOT opened yet.\n");

            if !fasta_files.is_empty() {
                let fasta_names: Vec<String> = fasta_files.iter().map(|p| {
                    std::path::Path::new(p.as_str())
                        .file_name().and_then(|n| n.to_str()).unwrap_or("unknown").to_string()
                }).collect();

                msg.push_str(&format!(
                    "\nFASTA file(s) found in the directory: {}\n\
                     Ask the user: 'Is [{}] the correct reference for your data? If not, please provide the correct reference file path.'\n\
                     - If yes: call show_results again with reference=\"{}\"\n\
                     - If user provides a different path: call show_results again with reference=<that_path>\n\
                     - If user has no reference: call show_results again with reference=\"none\"",
                    fasta_names.join(", "),
                    fasta_names.join(", "),
                    fasta_names[0],
                ));
            } else {
                msg.push_str(
                    "\nNo FASTA reference file found in the directory.\n\
                     Ask the user: 'Do you have a reference FASTA file for this data? If so, please provide the file path.'\n\
                     - If user provides a path: call show_results again with reference=<that_path>\n\
                     - If user has no reference: call show_results again with reference=\"none\""
                );
            }

            if !igv_only_files.is_empty() {
                msg.push_str(&format!(
                    "\n\nNote: {} require a reference and cannot be viewed without one.",
                    igv_only_files.join(", ")
                ));
            }

            return Ok(CallToolResult::success(vec![Content::text(msg)]));
        }

        // --- Reference confirmed / declined / no genomics → open viewer ---
        let total_files = files.len() + remote_files.len();
        match viewer::show_files(
            files.clone(),
            remote_files.clone(),
            self.config.full_plugins_dir(),
            reference,
            Some(self.config.clone()),
        ).await {
            Ok(url) => {
                let mut msg = format!(
                    "Opened results in browser: {}\n\nDisplaying {} file(s):",
                    url,
                    total_files
                );
                for (name, data, _) in &files {
                    msg.push_str(&format!("\n  {} ({} bytes)", name, data.len()));
                }
                for (name, _, size, _) in &remote_files {
                    msg.push_str(&format!("\n  {} ({} bytes, server-side)", name, size));
                }
                if !errors.is_empty() {
                    msg.push_str(&format!(
                        "\n\nSkipped {} file(s):\n{}",
                        errors.len(),
                        errors.join("\n")
                    ));
                }
                Ok(CallToolResult::success(vec![Content::text(msg)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to open browser: {}",
                e
            ))])),
        }
    }

    // ── Template tools ──────────────────────────────────────────

    #[tool(description = "Get pipeline file templates for creating new pipelines")]
    async fn get_templates(&self) -> Result<CallToolResult, ErrorData> {
        let text = format!(
            "=== Snakefile Template ===\n{}\n\n\
             === Dockerfile Template ===\n{}\n\n\
             === config.yaml Template ===\n{}\n\n\
             === ro-crate-metadata.json Template ===\n{}",
            templates::SNAKEFILE_TEMPLATE,
            templates::DOCKERFILE_TEMPLATE,
            templates::CONFIG_YAML_TEMPLATE,
            templates::RO_CRATE_METADATA_TEMPLATE,
        );
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(description = "Get the pipeline generation guide with rules for Snakefiles and Dockerfiles")]
    async fn get_generation_guide(&self) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::success(vec![Content::text(
            templates::GENERATION_GUIDE,
        )]))
    }

    // ── Plugin management tools ─────────────────────────────────

    #[tool(description = "List all locally installed viewer plugins. Shows each plugin's name, version, description, and supported file extensions.")]
    async fn list_installed_plugins(&self) -> Result<CallToolResult, ErrorData> {
        let plugins_dir = self.config.full_plugins_dir();
        let dir = std::path::Path::new(&plugins_dir);

        if !dir.is_dir() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "No plugins installed.\nPlugins directory: {}",
                plugins_dir
            ))]));
        }

        let mut plugins: Vec<serde_json::Value> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let manifest_path = path.join("manifest.json");
                if let Ok(content) = std::fs::read_to_string(&manifest_path) {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
                        plugins.push(v);
                    }
                }
            }
        }

        if plugins.is_empty() {
            Ok(CallToolResult::success(vec![Content::text(format!(
                "No plugins installed.\nPlugins directory: {}",
                plugins_dir
            ))]))
        } else {
            let mut msg = format!("Installed plugins ({}):\n", plugins.len());
            for p in &plugins {
                let name = p["name"].as_str().unwrap_or("unknown");
                let ver = p["version"].as_str().unwrap_or("?");
                let desc = p["description"].as_str().unwrap_or("");
                let exts: Vec<&str> = p["extensions"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
                    .unwrap_or_default();
                msg.push_str(&format!(
                    "\n  {} v{}\n    {}\n    Extensions: {}\n",
                    name, ver, desc, exts.join(", ")
                ));
            }
            msg.push_str(&format!("\nPlugins directory: {}", plugins_dir));
            Ok(CallToolResult::success(vec![Content::text(msg)]))
        }
    }

    #[tool(description = "Open the local plugins directory in the OS file explorer. Also shows a link to the plugin creation guide.")]
    async fn open_plugin_dir(&self) -> Result<CallToolResult, ErrorData> {
        let plugins_dir = self.config.full_plugins_dir();

        // Create directory if it doesn't exist
        if let Err(e) = std::fs::create_dir_all(&plugins_dir) {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to create plugins directory '{}': {}",
                plugins_dir, e
            ))]));
        }

        match open::that(&plugins_dir) {
            Ok(_) => {
                let base = self.config.registry_url.trim_end_matches('/');
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Opened plugins directory: {}\n\n\
                     To create a new plugin, see the guide:\n\
                     {}/plugins/guide",
                    plugins_dir, base
                ))]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to open directory '{}': {}",
                plugins_dir, e
            ))])),
        }
    }

    #[tool(description = "Install a viewer plugin from the AutoPipe registry. Downloads the plugin files from GitHub and saves them to the local plugins directory. WARNING: Plugins run JavaScript in the browser — only install plugins from authors you trust.")]
    async fn install_plugin(
        &self,
        Parameters(params): Parameters<InstallPluginParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let base = self.config.registry_url.trim_end_matches('/');
        let client = reqwest::Client::new();

        // 1. Search registry for the plugin
        let encoded_name: String = params.plugin_name.chars().map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c.to_string()
            } else {
                format!("%{:02X}", c as u32)
            }
        }).collect();
        let search_resp = client
            .get(format!(
                "{}/api/plugins/search?q={}",
                base, encoded_name
            ))
            .send()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let plugins: Vec<serde_json::Value> = search_resp
            .json()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Find exact name match
        let plugin = plugins.iter().find(|p| {
            p["name"].as_str().map(|n| n == params.plugin_name).unwrap_or(false)
        });

        let plugin = match plugin {
            Some(p) => p.clone(),
            None => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Plugin '{}' not found in the registry.",
                    params.plugin_name
                ))]));
            }
        };

        let github_url = match plugin["github_url"].as_str() {
            Some(u) => u.to_string(),
            None => {
                return Ok(CallToolResult::error(vec![Content::text(
                    "Plugin has no GitHub URL in the registry.",
                )]));
            }
        };

        let author = plugin["author"].as_str().unwrap_or("unknown");

        // 2. Parse GitHub URL to get owner/repo/path
        let (gh_owner, gh_repo, gh_branch, gh_path) = match parse_github_url(&github_url) {
            Some(parsed) => parsed,
            None => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid GitHub URL: {}", github_url
                ))]));
            }
        };

        // 3. Download manifest.json from GitHub
        let branch = gh_branch.as_deref().unwrap_or("main");
        let manifest_url = if gh_path.is_empty() {
            format!(
                "https://raw.githubusercontent.com/{}/{}/{}/manifest.json",
                gh_owner, gh_repo, branch
            )
        } else {
            format!(
                "https://raw.githubusercontent.com/{}/{}/{}/{}/manifest.json",
                gh_owner, gh_repo, branch, gh_path
            )
        };

        let manifest_resp = client
            .get(&manifest_url)
            .send()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        if !manifest_resp.status().is_success() {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to download manifest.json from GitHub: HTTP {}",
                manifest_resp.status()
            ))]));
        }

        let manifest_text = manifest_resp
            .text()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let manifest: serde_json::Value = match serde_json::from_str(&manifest_text) {
            Ok(v) => v,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid manifest.json: {}", e
                ))]));
            }
        };

        let plugin_name = manifest["name"]
            .as_str()
            .unwrap_or(&params.plugin_name)
            .to_string();
        let entry = manifest["entry"].as_str().unwrap_or("index.js");
        let style = manifest["style"].as_str();

        // 4. Create local plugin directory
        let plugins_dir = self.config.full_plugins_dir();
        let plugin_dir = std::path::Path::new(&plugins_dir).join(&plugin_name);
        if let Err(e) = std::fs::create_dir_all(&plugin_dir) {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to create plugin directory: {}", e
            ))]));
        }

        // 5. Save manifest.json
        if let Err(e) = std::fs::write(plugin_dir.join("manifest.json"), &manifest_text) {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to write manifest.json: {}", e
            ))]));
        }

        // 6. Download and save entry file (index.js)
        let entry_url = if gh_path.is_empty() {
            format!(
                "https://raw.githubusercontent.com/{}/{}/{}/{}",
                gh_owner, gh_repo, branch, entry
            )
        } else {
            format!(
                "https://raw.githubusercontent.com/{}/{}/{}/{}/{}",
                gh_owner, gh_repo, branch, gh_path, entry
            )
        };

        let entry_resp = client
            .get(&entry_url)
            .send()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        if entry_resp.status().is_success() {
            let entry_data = entry_resp
                .bytes()
                .await
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
            if let Err(e) = std::fs::write(plugin_dir.join(entry), &entry_data) {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Failed to write {}: {}", entry, e
                ))]));
            }
        } else {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to download {} from GitHub: HTTP {}",
                entry, entry_resp.status()
            ))]));
        }

        // 7. Download style file if specified
        if let Some(style_file) = style {
            let style_url = if gh_path.is_empty() {
                format!(
                    "https://raw.githubusercontent.com/{}/{}/{}/{}",
                    gh_owner, gh_repo, branch, style_file
                )
            } else {
                format!(
                    "https://raw.githubusercontent.com/{}/{}/{}/{}/{}",
                    gh_owner, gh_repo, branch, gh_path, style_file
                )
            };

            if let Ok(resp) = client.get(&style_url).send().await {
                if resp.status().is_success() {
                    if let Ok(data) = resp.bytes().await {
                        let _ = std::fs::write(plugin_dir.join(style_file), &data);
                    }
                }
            }
        }

        let version = manifest["version"].as_str().unwrap_or("?");
        let extensions: Vec<&str> = manifest["extensions"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
            .unwrap_or_default();

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Successfully installed plugin '{}' v{} by {}\n\
             Extensions: {}\n\
             Location: {}\n\n\
             The plugin will be active next time you use show_results.",
            plugin_name,
            version,
            author,
            extensions.join(", "),
            plugin_dir.display()
        ))]))
    }
}

#[tool_handler]
impl ServerHandler for AutoPipeServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            instructions: Some(
                "AutoPipe MCP Server — Bioinformatics pipeline management.\n\
                 All file operations are on the remote SSH server.\n\
                 Use get_workspace_info first to see configured paths.\n\
                 Use create_symlink instead of cp for data files.\n\
                 Pipeline outputs are stored under the configured output directory.\n\
                 Use list_files and read_file to view results from the output path.\n\n\
                 VERSION & FORK TRACKING (PUBLISH):\n\
                 Each publish creates a new version entry in the registry.\n\
                 Same name → automatically linked as a new version.\n\
                 Similar to existing → set forked_from to that pipeline ID.\n\
                 Brand new → omit forked_from.\n\
                 Before publishing, ALWAYS search the registry first."
                    .into(),
            ),
            ..Default::default()
        }
    }
}

/// Run the MCP server in stdio mode.
pub async fn run_mcp_server() -> Result<(), Box<dyn std::error::Error>> {
    let config = AppConfig::load();
    let server = AutoPipeServer::new(config);
    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
