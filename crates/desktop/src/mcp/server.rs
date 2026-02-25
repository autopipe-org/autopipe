use common::api_client::RegistryClient;
use common::models::{Pipeline, PipelineMetadata};
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
    /// Remote directory path on the SSH server where the pipeline will be saved
    output_dir: String,
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
    /// Remote output directory
    output_dir: String,
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
    /// Remote output directory
    output_dir: String,
    /// Number of CPU cores (default: 8)
    cores: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct StatusParams {
    /// Run name (matches the run_name used in execute_pipeline)
    run_name: String,
    /// Remote output directory (where pipeline.log is located)
    output_dir: String,
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

// ── Server ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AutoPipeServer {
    registry: RegistryClient,
    config: AppConfig,
    tool_router: ToolRouter<Self>,
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Strip any prepended JSON fragment like `{"success": true}` from file content.
/// Works for all file types: JSON files get `}{` split, others get prefix removed.
fn clean_content(raw: &str) -> String {
    let s = raw.trim();
    // For JSON-like content with concatenated objects: {"success": true}{"name": ...}
    if let Some(pos) = s.find("}{") {
        return s[pos + 1..].to_string();
    }
    // For non-JSON files where {"success": true} is prepended as a line
    let prefix = r#"{"success": true}"#;
    if s.starts_with(prefix) {
        return s[prefix.len()..].trim_start().to_string();
    }
    // Also handle without spaces: {"success":true}
    let prefix_no_space = r#"{"success":true}"#;
    if s.starts_with(prefix_no_space) {
        return s[prefix_no_space.len()..].trim_start().to_string();
    }
    s.to_string()
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

    /// Find the pipeline directory for a given Docker image name.
    /// Searches the configured output_dir for a matching pipeline directory.
    async fn find_pipeline_dir(&self, image_name: &str) -> Option<String> {
        // The image name follows the pattern "autopipe-<pipeline-name>"
        // The pipeline directory is at <output_dir>/<pipeline-name>/<pipeline-name>/
        let pipeline_name = image_name.strip_prefix("autopipe-").unwrap_or(image_name);
        let candidate = format!(
            "{}/{}/{}",
            self.config.output_dir.trim_end_matches('/'),
            pipeline_name,
            pipeline_name
        );
        // Check if the directory exists
        if let Ok((output, 0)) = self.ssh_run(&format!("test -d '{}' && echo 'exists'", candidate)).await {
            if output.trim().contains("exists") {
                return Some(candidate);
            }
        }
        // Also try under the user's home projects path
        let home_candidate = format!(
            "/home/{}/projects/autopipe/pipelines_output/{}/{}",
            self.config.ssh_user, pipeline_name, pipeline_name
        );
        if let Ok((output, 0)) = self.ssh_run(&format!("test -d '{}' && echo 'exists'", home_candidate)).await {
            if output.trim().contains("exists") {
                return Some(home_candidate);
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

    #[tool(description = "Download a pipeline by ID and save it to a directory on the remote SSH server")]
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

        let dir = format!(
            "{}/{}",
            params.output_dir.trim_end_matches('/'),
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

    #[tool(description = "Dry-run a pipeline (snakemake -n -p) on the remote server via SSH")]
    async fn dry_run(
        &self,
        Parameters(params): Parameters<DryRunParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let cores = params.cores.unwrap_or(8);

        // Find the pipeline directory to mount
        let pipeline_mount = match self.find_pipeline_dir(&params.image_name).await {
            Some(dir) => format!("-v '{}:/pipeline' -w /pipeline", dir),
            None => String::new(),
        };

        let cmd = format!(
            "docker run --rm --entrypoint snakemake {} -v '{}:/input:ro' -v '{}:/output' '{}' --cores {} --snakefile /pipeline/Snakefile --configfile /pipeline/config.yaml -n -p",
            pipeline_mount, params.input_dir, params.output_dir, params.image_name, cores
        );

        match self.ssh_run(&cmd).await {
            Ok((output, 0)) => Ok(CallToolResult::success(vec![Content::text(output)])),
            Ok((output, _)) => Ok(CallToolResult::error(vec![Content::text(output)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(description = "Execute a pipeline in the background on the remote server via SSH. Logs are written to {output_dir}/pipeline.log.")]
    async fn execute_pipeline(
        &self,
        Parameters(params): Parameters<ExecuteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let cores = params.cores.unwrap_or(8);
        let container_name = format!("{}-run", params.run_name);
        let log_path = format!("{}/pipeline.log", params.output_dir.trim_end_matches('/'));

        // Find the pipeline directory to mount
        let pipeline_mount = match self.find_pipeline_dir(&params.image_name).await {
            Some(dir) => format!("-v '{}:/pipeline' -w /pipeline", dir),
            None => String::new(),
        };

        // Remove old container with same name if exists
        let _ = self.ssh_run(&format!("docker rm -f '{}' 2>/dev/null", container_name)).await;

        // Create output directory
        let _ = self.ssh_run(&format!("mkdir -p '{}'", params.output_dir)).await;

        // Run with nohup in background, redirect all output to log file
        let cmd = format!(
            "nohup docker run --rm --entrypoint snakemake --name '{}' {} -v '{}:/input:ro' -v '{}:/output' '{}' --cores {} --snakefile /pipeline/Snakefile --configfile /pipeline/config.yaml > '{}' 2>&1 &\necho $!",
            container_name, pipeline_mount, params.input_dir, params.output_dir, params.image_name, cores, log_path
        );

        match self.ssh_run(&cmd).await {
            Ok((output, 0)) => {
                let pid = output.trim().lines().last().unwrap_or("unknown");
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Pipeline started in background (PID: {}, container: '{}').\nLog file: {}\nUse check_status with run_name='{}' and output_dir='{}' to monitor progress.",
                    pid, container_name, log_path, params.run_name, params.output_dir
                ))]))
            }
            Ok((output, _)) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to start pipeline:\n{}",
                output
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(description = "Check pipeline execution status by reading the log file and checking if the process is still running")]
    async fn check_status(
        &self,
        Parameters(params): Parameters<StatusParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let container_name = format!("{}-run", params.run_name);
        let log_path = format!("{}/pipeline.log", params.output_dir.trim_end_matches('/'));

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
            "Status: {}\nContainer: {}\nLog ({}):\n{}",
            status_str, container_name, log_path, log_output
        ))]))
    }

    // ── Remote file tools ───────────────────────────────────────

    #[tool(description = "Create a symbolic link on the remote SSH server. Use this to link input/output data into a pipeline working directory. The source path must exist on the server.")]
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

    #[tool(description = "Read the contents of a file on the remote SSH server")]
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
                 Use read_file/write_file/list_files for remote file access.\n\
                 Use create_symlink to link input/output data into pipeline directories.\n\
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

