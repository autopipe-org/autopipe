use common::api_client::RegistryClient;
use common::models::{clean_content, Pipeline, PipelineMetadata};
use common::templates;
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::{tool, tool_handler, tool_router, ErrorData, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::config::AppConfig;
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
    /// Remote output directory (optional, defaults to configured output directory under run_name)
    output_dir: Option<String>,
    /// Number of CPU cores (default: 8)
    cores: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct StatusParams {
    /// Run name (matches the run_name used in execute_pipeline)
    run_name: String,
    /// Remote output directory (optional, defaults to configured output directory under run_name)
    output_dir: Option<String>,
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
struct ViewImageParams {
    /// Remote path to an image file (PNG, JPG, SVG, PDF, etc.)
    path: String,
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
    /// Remote path to the pipeline directory on the SSH server
    pipeline_dir: String,
    /// GitHub URL of the uploaded workflow (from upload_workflow result)
    github_url: String,
}

// ── Server ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AutoPipeServer {
    registry: RegistryClient,
    config: AppConfig,
    tool_router: ToolRouter<Self>,
}

// ── Helpers ─────────────────────────────────────────────────────────

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

// ── SSH helper methods ──────────────────────────────────────────────

impl AutoPipeServer {
    async fn ssh_run(&self, cmd: &str) -> Result<(String, i32), String> {
        let config = self.config.clone();
        let cmd = cmd.to_string();
        tokio::task::spawn_blocking(move || ssh::ssh_exec(&config, &cmd))
            .await
            .map_err(|e| format!("Task error: {}", e))?
    }

    async fn ssh_read_file(&self, path: &str) -> Result<String, String> {
        let cmd = format!("cat '{}'", path);
        match self.ssh_run(&cmd).await {
            Ok((output, 0)) => Ok(output),
            Ok((output, _)) => Err(format!("Failed to read: {}", output.trim())),
            Err(e) => Err(e),
        }
    }

    async fn ssh_write_file(&self, path: &str, content: &str) -> Result<(), String> {
        let cmd = format!(
            "cat << 'AUTOPIPE_EOF' > '{}'\n{}\nAUTOPIPE_EOF",
            path, content
        );
        match self.ssh_run(&cmd).await {
            Ok((_, 0)) => Ok(()),
            Ok((output, _)) => Err(format!("Write failed: {}", output.trim())),
            Err(e) => Err(e),
        }
    }

    /// Resolve the output directory for a run.
    /// Uses the provided path if given, otherwise defaults to {full_output_dir}/{run_name}.
    fn resolve_output_dir(&self, explicit: &Option<String>, run_name: &str) -> String {
        match explicit {
            Some(dir) if !dir.is_empty() => dir.clone(),
            _ => format!(
                "{}/{}",
                self.config.full_output_dir().trim_end_matches('/'),
                run_name
            ),
        }
    }

    /// Find symlink targets inside a directory and return extra Docker -v mounts.
    /// This resolves symlinks on the SSH server so Docker can see the real files.
    async fn resolve_symlink_mounts(&self, dir: &str) -> String {
        // Find all symlinks, resolve them, get unique parent directories
        let cmd = format!(
            "find '{}' -maxdepth 3 -type l -exec readlink -f '{{}}' \\; 2>/dev/null | xargs -I{{}} dirname '{{}}' | sort -u",
            dir
        );
        let dirs = match self.ssh_run(&cmd).await {
            Ok((output, 0)) => output,
            _ => return String::new(),
        };

        let mut mounts = String::new();
        for target_dir in dirs.trim().lines() {
            let target_dir = target_dir.trim();
            if target_dir.is_empty() || target_dir == dir {
                continue;
            }
            mounts.push_str(&format!(" -v '{}:{}:ro'", target_dir, target_dir));
        }
        mounts
    }

    /// Find the pipeline directory for a given Docker image name.
    /// Searches the configured pipelines and output directories.
    async fn find_pipeline_dir(&self, image_name: &str) -> Option<String> {
        let pipeline_name = image_name.strip_prefix("autopipe-").unwrap_or(image_name);

        // Check under configured pipelines directory: {pipelines_dir}/{name}
        let pipelines_base = self.config.full_pipelines_dir();
        let candidate = format!(
            "{}/{}",
            pipelines_base.trim_end_matches('/'),
            pipeline_name
        );
        if let Ok((output, 0)) = self
            .ssh_run(&format!("test -d '{}' && echo 'exists'", candidate))
            .await
        {
            if output.trim().contains("exists") {
                return Some(candidate);
            }
        }

        // Check under configured output directory: {output_dir}/{name}/{name}
        let output_base = self.config.full_output_dir();
        let candidate = format!(
            "{}/{}/{}",
            output_base.trim_end_matches('/'),
            pipeline_name,
            pipeline_name
        );
        if let Ok((output, 0)) = self
            .ssh_run(&format!("test -d '{}' && echo 'exists'", candidate))
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

    // ── Registry tools ──────────────────────────────────────────

    #[tool(description = "Search pipelines by keyword in name, description, tools, or tags")]
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

    #[tool(description = "Download a pipeline by ID and save it to the remote SSH server. If output_dir is omitted, saves to the configured pipelines directory.")]
    async fn download_pipeline(
        &self,
        Parameters(params): Parameters<DownloadParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let pipeline = match self.registry.download(params.pipeline_id).await {
            Ok(p) => p,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Download failed: {}",
                    e
                ))]));
            }
        };

        let base_dir = params
            .output_dir
            .unwrap_or_else(|| self.config.full_pipelines_dir());
        let dir = format!(
            "{}/{}",
            base_dir.trim_end_matches('/'),
            pipeline.name
        );
        let meta_str =
            serde_json::to_string_pretty(&pipeline.metadata_json).unwrap_or_default();

        // Create directory on remote server
        match self.ssh_run(&format!("mkdir -p '{}'", dir)).await {
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

        // Write each file via SSH
        let files: Vec<(&str, &str)> = vec![
            ("Snakefile", &pipeline.snakefile),
            ("Dockerfile", &pipeline.dockerfile),
            ("config.yaml", &pipeline.config_yaml),
            ("metadata.json", &meta_str),
            ("README.md", &pipeline.readme),
        ];

        for (name, content) in &files {
            let path = format!("{}/{}", dir, name);
            if let Err(e) = self.ssh_write_file(&path, content).await {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Failed to write {}: {}",
                    name, e
                ))]));
            }
        }

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Downloaded pipeline '{}' to {} (remote server)",
            pipeline.name, dir
        ))]))
    }

    #[tool(description = "Upload a pipeline from a directory on the remote SSH server to the registry")]
    async fn upload_pipeline(
        &self,
        Parameters(params): Parameters<PipelineDirParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dir = &params.pipeline_dir;

        let meta_content = match self.ssh_read_file(&format!("{}/metadata.json", dir)).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Cannot read metadata.json: {}",
                    e
                ))]));
            }
        };

        // Read and clean all files (strip {"success": true} prefix if present)
        let snakefile = clean_content(
            &self.ssh_read_file(&format!("{}/Snakefile", dir)).await.unwrap_or_default(),
        );
        let dockerfile = clean_content(
            &self.ssh_read_file(&format!("{}/Dockerfile", dir)).await.unwrap_or_default(),
        );
        let config_yaml = clean_content(
            &self.ssh_read_file(&format!("{}/config.yaml", dir)).await.unwrap_or_default(),
        );
        let readme = clean_content(
            &self.ssh_read_file(&format!("{}/README.md", dir)).await.unwrap_or_default(),
        );

        // Normalize paths in config.yaml and Snakefile to use /input, /output
        let snakefile = normalize_paths(&snakefile);
        let config_yaml = normalize_paths(&config_yaml);

        let cleaned_meta = clean_content(&meta_content);
        let metadata: PipelineMetadata = match serde_json::from_str(&cleaned_meta) {
            Ok(m) => m,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid metadata.json: {}",
                    e
                ))]));
            }
        };

        let pipeline = Pipeline {
            pipeline_id: None,
            name: metadata.name.clone(),
            description: metadata.description,
            tools: metadata.tools,
            input_formats: metadata.input_formats,
            output_formats: metadata.output_formats,
            tags: metadata.tags,
            snakefile,
            dockerfile,
            config_yaml,
            metadata_json: serde_json::from_str(&cleaned_meta).unwrap_or_default(),
            readme,
            author: metadata.author,
            version: metadata.version,
            verified: metadata.verified,
            created_at: None,
            updated_at: None,
        };

        match self.registry.upload(&pipeline).await {
            Ok(id) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Uploaded pipeline '{}' with id={}",
                metadata.name, id
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Upload failed: {}",
                e
            ))])),
        }
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
        let meta_raw = match self.ssh_read_file(&format!("{}/metadata.json", dir)).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Cannot read metadata.json: {}", e
                ))]));
            }
        };
        let cleaned_meta = clean_content(&meta_raw);
        let metadata: PipelineMetadata = match serde_json::from_str(&cleaned_meta) {
            Ok(m) => m,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid metadata.json: {}", e
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
            // Create repo
            let create_resp = client
                .post("https://api.github.com/user/repos")
                .header("Authorization", format!("Bearer {}", token))
                .header("User-Agent", "autopipe-desktop")
                .json(&serde_json::json!({
                    "name": repo_name,
                    "description": "AutoPipe bioinformatics pipelines",
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
            // Wait briefly for repo initialization
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
            ("metadata.json", &metadata_json_str),
            ("README.md", &readme),
        ];

        let tree_items: Vec<serde_json::Value> = files_to_commit
            .iter()
            .filter(|(_, content)| !content.is_empty())
            .map(|(name, content)| {
                serde_json::json!({
                    "path": format!("{}/{}", pipeline_name, name),
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
            "https://github.com/{}/{}/tree/main/{}",
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

    #[tool(description = "Publish a pipeline from GitHub to the AutoPipe registry web page. The pipeline must be uploaded to GitHub first (via upload_workflow). This performs security validation and makes the pipeline publicly visible on the registry website.")]
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

        let dir = &params.pipeline_dir;

        // Read pipeline files from SSH
        let meta_raw = match self.ssh_read_file(&format!("{}/metadata.json", dir)).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Cannot read metadata.json: {}", e
                ))]));
            }
        };
        let cleaned_meta = clean_content(&meta_raw);
        let metadata: PipelineMetadata = match serde_json::from_str(&cleaned_meta) {
            Ok(m) => m,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid metadata.json: {}", e
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

        let pipeline = Pipeline {
            pipeline_id: None,
            name: metadata.name.clone(),
            description: metadata.description,
            tools: metadata.tools,
            input_formats: metadata.input_formats,
            output_formats: metadata.output_formats,
            tags: metadata.tags,
            snakefile,
            dockerfile,
            config_yaml,
            metadata_json: serde_json::from_str(&cleaned_meta).unwrap_or_default(),
            readme,
            author: metadata.author,
            version: metadata.version,
            verified: metadata.verified,
            created_at: None,
            updated_at: None,
        };

        // Call registry publish endpoint
        let base = self.config.registry_url.trim_end_matches('/');
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/api/publish", base))
            .json(&serde_json::json!({
                "pipeline": pipeline,
                "github_url": params.github_url,
                "github_token": token,
            }))
            .send()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let status = resp.status();
        let body: serde_json::Value = resp.json().await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        if status.is_success() {
            let pipeline_id = body["pipeline_id"].as_i64().unwrap_or(0);
            let web_url = format!("{}/pipelines/{}", base, pipeline_id);
            Ok(CallToolResult::success(vec![Content::text(format!(
                "Successfully published '{}' to the registry!\n\
                 Web page: {}\n\
                 Pipeline ID: {}",
                metadata.name, web_url, pipeline_id
            ))]))
        } else if status.as_u16() == 422 {
            // Security validation failed
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
            "metadata.json",
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
                    if *f == "metadata.json" {
                        match serde_json::from_str::<PipelineMetadata>(&content) {
                            Ok(m) => {
                                if m.name.is_empty() {
                                    errors.push("metadata.json: 'name' is empty".into());
                                }
                                if m.tools.is_empty() {
                                    errors.push("metadata.json: 'tools' is empty".into());
                                }
                            }
                            Err(e) => {
                                errors.push(format!("metadata.json: invalid - {}", e))
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

    #[tool(description = "Build a Docker image for a pipeline on the remote server via SSH")]
    async fn build_image(
        &self,
        Parameters(params): Parameters<BuildParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let cmd = format!(
            "cd '{}' && docker build -t '{}' .",
            params.pipeline_dir, params.image_name
        );

        match self.ssh_run(&cmd).await {
            Ok((output, 0)) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Docker image '{}' built successfully.\n{}",
                params.image_name, output
            ))])),
            Ok((output, _)) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Docker build failed:\n{}",
                output
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
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

        // Find the pipeline directory to mount
        let pipeline_mount = match self.find_pipeline_dir(&params.image_name).await {
            Some(dir) => format!("-v '{}:/pipeline' -w /pipeline", dir),
            None => String::new(),
        };

        // Resolve symlinks in input_dir for extra Docker mounts
        let symlink_mounts = self.resolve_symlink_mounts(&params.input_dir).await;

        let cmd = format!(
            "docker run --rm --entrypoint snakemake {} -v '{}:/input:ro'{} -v '{}:/output' '{}' --cores {} --snakefile /pipeline/Snakefile --configfile /pipeline/config.yaml -n -p",
            pipeline_mount, params.input_dir, symlink_mounts, output_dir, params.image_name, cores
        );

        match self.ssh_run(&cmd).await {
            Ok((output, 0)) => Ok(CallToolResult::success(vec![Content::text(output)])),
            Ok((output, _)) => Ok(CallToolResult::error(vec![Content::text(output)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(description = "Execute a pipeline in the background on the remote server via SSH. If output_dir is omitted, outputs are stored at {configured_output_dir}/{run_name}/. Logs are written to {output_dir}/pipeline.log.")]
    async fn execute_pipeline(
        &self,
        Parameters(params): Parameters<ExecuteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let cores = params.cores.unwrap_or(8);
        let output_dir = self.resolve_output_dir(&params.output_dir, &params.run_name);
        let container_name = format!("{}-run", params.run_name);
        let log_path = format!("{}/pipeline.log", output_dir.trim_end_matches('/'));

        // Find the pipeline directory to mount
        let pipeline_mount = match self.find_pipeline_dir(&params.image_name).await {
            Some(dir) => format!("-v '{}:/pipeline' -w /pipeline", dir),
            None => String::new(),
        };

        // Resolve symlinks in input_dir for extra Docker mounts
        let symlink_mounts = self.resolve_symlink_mounts(&params.input_dir).await;

        // Remove old container with same name if exists
        let _ = self.ssh_run(&format!("docker rm -f '{}' 2>/dev/null", container_name)).await;

        // Create output directory
        let _ = self.ssh_run(&format!("mkdir -p '{}'", output_dir)).await;

        // Run with nohup in background, redirect all output to log file
        let cmd = format!(
            "nohup docker run --rm --entrypoint snakemake --name '{}' {} -v '{}:/input:ro'{} -v '{}:/output' '{}' --cores {} --snakefile /pipeline/Snakefile --configfile /pipeline/config.yaml > '{}' 2>&1 &\necho $!",
            container_name, pipeline_mount, params.input_dir, symlink_mounts, output_dir, params.image_name, cores, log_path
        );

        match self.ssh_run(&cmd).await {
            Ok((output, 0)) => {
                let pid = output.trim().lines().last().unwrap_or("unknown");
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Pipeline started in background (PID: {}, container: '{}').\n\
                     Output directory: {}\n\
                     Log file: {}\n\
                     Use check_status with run_name='{}' to monitor progress.\n\
                     Use list_files on the output directory to browse results.",
                    pid, container_name, output_dir, log_path, params.run_name
                ))]))
            }
            Ok((output, _)) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to start pipeline:\n{}",
                output
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(description = "Check pipeline execution status by reading the log file and checking if the process is still running. If output_dir is omitted, uses {configured_output_dir}/{run_name}/.")]
    async fn check_status(
        &self,
        Parameters(params): Parameters<StatusParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let output_dir = self.resolve_output_dir(&params.output_dir, &params.run_name);
        let container_name = format!("{}-run", params.run_name);
        let log_path = format!("{}/pipeline.log", output_dir.trim_end_matches('/'));

        // Check if container is still running
        let running = match self.ssh_run(&format!("docker inspect -f '{{{{.State.Running}}}}' '{}' 2>/dev/null", container_name)).await {
            Ok((output, 0)) => output.trim() == "true",
            _ => false,
        };

        // Read last 50 lines of log
        let log_output = match self.ssh_run(&format!("tail -50 '{}' 2>/dev/null", log_path)).await {
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

    // ── Remote file tools ───────────────────────────────────────

    #[tool(description = "Create a symbolic link on the remote SSH server. Use this to link input/output data instead of copying files. Prefer symlinks over cp for accessing result files and plots.")]
    async fn create_symlink(
        &self,
        Parameters(params): Parameters<CreateSymlinkParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Verify source exists
        match self.ssh_run(&format!("test -e '{}' && echo 'exists'", params.source)).await {
            Ok((output, 0)) if output.trim().contains("exists") => {}
            _ => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Source path '{}' does not exist on remote server",
                    params.source
                ))]));
            }
        }

        // Create parent directory of target if needed
        if let Some(parent) = std::path::Path::new(&params.target).parent() {
            let _ = self
                .ssh_run(&format!("mkdir -p '{}'", parent.to_string_lossy()))
                .await;
        }

        // Create symlink
        match self.ssh_run(&format!("ln -sf '{}' '{}'", params.source, params.target)).await {
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
            params.symlink_path, params.symlink_path
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

    #[tool(description = "List files and directories at a remote path on the SSH server")]
    async fn list_files(
        &self,
        Parameters(params): Parameters<ListFilesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.ssh_run(&format!("ls -la '{}'", params.path)).await {
            Ok((output, 0)) => Ok(CallToolResult::success(vec![Content::text(output)])),
            Ok((output, _)) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Cannot list '{}': {}",
                params.path,
                output.trim()
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(description = "Read the contents of a file on the remote SSH server. Use this to view result files directly from the output directory.")]
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

    #[tool(description = "Write content to a file on the remote SSH server. Creates parent directories if needed.")]
    async fn write_file(
        &self,
        Parameters(params): Parameters<WriteFileParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Create parent directory if needed
        if let Some(parent) = std::path::Path::new(&params.path).parent() {
            let _ = self
                .ssh_run(&format!("mkdir -p '{}'", parent.to_string_lossy()))
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

    #[tool(description = "View an image file (plot, figure, chart) from the remote SSH server. Returns the image inline so it can be displayed directly. Large images are automatically resized. Supports PNG, JPG, SVG, GIF, and PDF.")]
    async fn view_image(
        &self,
        Parameters(params): Parameters<ViewImageParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Detect MIME type from extension
        let ext = params.path.rsplit('.').next().map(|e| e.to_lowercase()).unwrap_or_default();
        let mime_type = match ext.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "svg" => "image/svg+xml",
            "pdf" => "application/pdf",
            "webp" => "image/webp",
            _ => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Unsupported file type: {}. Supported: png, jpg, gif, svg, pdf, webp",
                    params.path
                ))]));
            }
        };

        // Check file exists and get size
        let size_check = format!("stat -c%s '{}' 2>/dev/null || echo 'NOT_FOUND'", params.path);
        let file_size: u64 = match self.ssh_run(&size_check).await {
            Ok((output, _)) => {
                let cleaned = clean_content(&output);
                let val = cleaned.trim();
                if val == "NOT_FOUND" || val.is_empty() {
                    return Ok(CallToolResult::error(vec![Content::text(format!(
                        "File not found: {}",
                        params.path
                    ))]));
                }
                val.parse().unwrap_or(0)
            }
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Cannot check file: {}", e
                ))]));
            }
        };

        // For raster images > 500KB, resize on server using Python
        let needs_resize = file_size > 500 * 1024
            && matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp");

        let base64_result = if needs_resize {
            // Ensure autopipe-resizer Docker image exists, build if not
            let ensure_image = r#"docker image inspect autopipe-resizer >/dev/null 2>&1 || docker build -t autopipe-resizer - << 'DOCKERFILE'
FROM python:3.11-slim
RUN pip install --no-cache-dir Pillow
DOCKERFILE"#;
            let _ = self.ssh_run(ensure_image).await;

            // Resize using Docker container with Pillow
            let parent_dir = params.path.rsplitn(2, '/').nth(1).unwrap_or("/");
            let filename = params.path.rsplitn(2, '/').next().unwrap_or(&params.path);
            let resize_cmd = format!(
                r#"docker run --rm -i -v '{parent_dir}:/img:ro' autopipe-resizer python3 << 'PYEOF'
import sys, base64, io
from PIL import Image
img = Image.open('/img/{filename}')
img.thumbnail((1200, 1200), Image.LANCZOS)
buf = io.BytesIO()
fmt = 'PNG' if '{ext}' in ('png', 'gif', 'webp') else 'JPEG'
img.save(buf, format=fmt, quality=85)
buf.seek(0)
sys.stdout.write(base64.b64encode(buf.read()).decode())
PYEOF"#,
                parent_dir = parent_dir,
                filename = filename,
                ext = ext,
            );
            self.ssh_run(&resize_cmd).await
        } else {
            // Small file or non-raster (SVG/PDF): send as-is
            let cmd = format!("base64 -w 0 '{}'", params.path);
            self.ssh_run(&cmd).await
        };

        match base64_result {
            Ok((base64_data, 0)) => {
                // Strip {"success": true} prefix if present from shell init
                let trimmed = clean_content(&base64_data).trim().to_string();
                if trimmed.is_empty() {
                    return Ok(CallToolResult::error(vec![Content::text(
                        "Failed to encode image: empty output",
                    )]));
                }
                // Use PNG mime type if we resized (output is always PNG/JPEG)
                let final_mime = if needs_resize && matches!(ext.as_str(), "png" | "gif" | "webp") {
                    "image/png"
                } else if needs_resize {
                    "image/jpeg"
                } else {
                    mime_type
                };
                Ok(CallToolResult::success(vec![Content::image(
                    trimmed, final_mime,
                )]))
            }
            Ok((output, _)) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to read image: {}",
                clean_content(&output).trim()
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    // ── Template tools ──────────────────────────────────────────

    #[tool(description = "Get pipeline file templates for creating new pipelines")]
    async fn get_templates(&self) -> Result<CallToolResult, ErrorData> {
        let text = format!(
            "=== Snakefile Template ===\n{}\n\n\
             === Dockerfile Template ===\n{}\n\n\
             === config.yaml Template ===\n{}\n\n\
             === metadata.json Template ===\n{}",
            templates::SNAKEFILE_TEMPLATE,
            templates::DOCKERFILE_TEMPLATE,
            templates::CONFIG_YAML_TEMPLATE,
            templates::METADATA_JSON_TEMPLATE,
        );
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(description = "Get the pipeline generation guide with rules for Snakefiles and Dockerfiles")]
    async fn get_generation_guide(&self) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::success(vec![Content::text(
            templates::GENERATION_GUIDE,
        )]))
    }
}

#[tool_handler]
impl ServerHandler for AutoPipeServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            instructions: Some(
                "AutoPipe MCP Server - Manage bioinformatics Snakemake pipelines.\n\
                 All file operations are performed on the remote SSH server.\n\
                 Use get_workspace_info first to see configured paths.\n\
                 Pipeline outputs are automatically stored under the configured output directory.\n\
                 Use list_files and read_file to view results directly from the output path.\n\
                 IMPORTANT: Use create_symlink instead of cp when linking data or accessing results.\n\
                 All paths are remote server paths."
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
