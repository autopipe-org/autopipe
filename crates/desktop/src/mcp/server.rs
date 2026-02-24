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
    /// Local directory path where the pipeline will be saved (SSHFS mount path)
    output_dir: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PipelineDirParams {
    /// Local path to the pipeline directory (SSHFS mount path)
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
    /// tmux session name
    session_name: String,
    /// Remote input data directory (mounted as read-only in Docker)
    input_dir: String,
    /// Remote output directory
    output_dir: String,
    /// Number of CPU cores (default: 8)
    cores: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct StatusParams {
    /// tmux session name or container name prefix
    session_name: String,
}

// ── Server ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AutoPipeServer {
    registry: RegistryClient,
    config: AppConfig,
    tool_router: ToolRouter<Self>,
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

    #[tool(description = "Download a pipeline by ID and save it to a local directory (SSHFS mounted)")]
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

        let files: Vec<(&str, &str)> = vec![
            ("Snakefile", &pipeline.snakefile),
            ("Dockerfile", &pipeline.dockerfile),
            ("config.yaml", &pipeline.config_yaml),
            ("metadata.json", &meta_str),
            ("README.md", &pipeline.readme),
        ];

        if let Err(e) = std::fs::create_dir_all(&dir) {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Cannot create directory '{}': {}",
                dir, e
            ))]));
        }

        for (name, content) in &files {
            let path = format!("{}/{}", dir, name);
            if let Err(e) = std::fs::write(&path, content) {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Failed to write {}: {}",
                    name, e
                ))]));
            }
        }

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Downloaded pipeline '{}' to {}",
            pipeline.name, dir
        ))]))
    }

    #[tool(description = "Upload a pipeline from a local directory (SSHFS mounted) to the registry")]
    async fn upload_pipeline(
        &self,
        Parameters(params): Parameters<PipelineDirParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dir = &params.pipeline_dir;

        let meta_content = match std::fs::read_to_string(format!("{}/metadata.json", dir)) {
            Ok(c) => c,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Cannot read metadata.json: {}",
                    e
                ))]));
            }
        };

        let snakefile =
            std::fs::read_to_string(format!("{}/Snakefile", dir)).unwrap_or_default();
        let dockerfile =
            std::fs::read_to_string(format!("{}/Dockerfile", dir)).unwrap_or_default();
        let config_yaml =
            std::fs::read_to_string(format!("{}/config.yaml", dir)).unwrap_or_default();
        let readme =
            std::fs::read_to_string(format!("{}/README.md", dir)).unwrap_or_default();

        let metadata: PipelineMetadata = match serde_json::from_str(&meta_content) {
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
            metadata_json: serde_json::from_str(&meta_content).unwrap_or_default(),
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

    #[tool(description = "Validate a pipeline directory structure (local SSHFS mounted path)")]
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
            match std::fs::read_to_string(&path) {
                Ok(content) if content.is_empty() => errors.push(format!("Empty: {}", f)),
                Err(_) => errors.push(format!("Missing: {}", f)),
                Ok(content) => {
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

    #[tool(description = "Build a Docker image for a pipeline on the remote server via SSH. Use remote path.")]
    async fn build_image(
        &self,
        Parameters(params): Parameters<BuildParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let config = self.config.clone();
        let cmd = format!(
            "cd '{}' && docker build -t '{}' .",
            params.pipeline_dir, params.image_name
        );

        let result = tokio::task::spawn_blocking(move || ssh::ssh_exec(&config, &cmd))
            .await
            .map_err(|e| format!("Task error: {}", e));

        match result {
            Ok(Ok((output, status))) if status == 0 => {
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Docker image '{}' built successfully.\n{}",
                    params.image_name, output
                ))]))
            }
            Ok(Ok((output, _))) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Docker build failed:\n{}",
                output
            ))])),
            Ok(Err(e)) => Ok(CallToolResult::error(vec![Content::text(e)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(description = "Dry-run a pipeline (snakemake -n -p) on the remote server via SSH. Use remote paths.")]
    async fn dry_run(
        &self,
        Parameters(params): Parameters<DryRunParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let cores = params.cores.unwrap_or(8);
        let config = self.config.clone();
        let cmd = format!(
            "docker run --rm -v '{}:/input:ro' -v '{}:/output' '{}' snakemake --cores {} -n -p",
            params.input_dir, params.output_dir, params.image_name, cores
        );

        let result = tokio::task::spawn_blocking(move || ssh::ssh_exec(&config, &cmd))
            .await
            .map_err(|e| format!("Task error: {}", e));

        match result {
            Ok(Ok((output, status))) if status == 0 => {
                Ok(CallToolResult::success(vec![Content::text(output)]))
            }
            Ok(Ok((output, _))) => Ok(CallToolResult::error(vec![Content::text(output)])),
            Ok(Err(e)) => Ok(CallToolResult::error(vec![Content::text(e)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(description = "Execute a pipeline in a tmux session on the remote server via SSH. Use remote paths.")]
    async fn execute_pipeline(
        &self,
        Parameters(params): Parameters<ExecuteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let cores = params.cores.unwrap_or(8);
        let config = self.config.clone();
        let cmd = format!(
            "tmux new-session -d -s '{}' \"docker run --rm --name '{}-run' -v '{}:/input:ro' -v '{}:/output' '{}' snakemake --cores {}\"",
            params.session_name, params.session_name, params.input_dir, params.output_dir, params.image_name, cores
        );

        let result = tokio::task::spawn_blocking(move || ssh::ssh_exec(&config, &cmd))
            .await
            .map_err(|e| format!("Task error: {}", e));

        match result {
            Ok(Ok((_, status))) if status == 0 => {
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Pipeline started in tmux session '{}' on remote server.\nMonitor: ssh then `tmux attach -t {}`",
                    params.session_name, params.session_name
                ))]))
            }
            Ok(Ok((output, _))) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to start tmux session:\n{}",
                output
            ))])),
            Ok(Err(e)) => Ok(CallToolResult::error(vec![Content::text(e)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(description = "Check pipeline execution status on the remote server via SSH")]
    async fn check_status(
        &self,
        Parameters(params): Parameters<StatusParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let config = self.config.clone();
        let session_name = params.session_name.clone();

        let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
            let tmux_cmd = format!(
                "tmux capture-pane -t '{}' -p -l 30 2>/dev/null",
                session_name
            );
            if let Ok((output, 0)) = ssh::ssh_exec(&config, &tmux_cmd) {
                if !output.trim().is_empty() {
                    return Ok(format!(
                        "tmux session '{}' output:\n{}",
                        session_name, output
                    ));
                }
            }

            let container_name = format!("{}-run", session_name);
            let docker_cmd = format!("docker logs --tail 30 '{}' 2>&1", container_name);
            if let Ok((output, _)) = ssh::ssh_exec(&config, &docker_cmd) {
                return Ok(format!("Docker logs for '{}':\n{}", container_name, output));
            }

            Ok(format!(
                "No active session or container found for '{}'",
                session_name
            ))
        })
        .await
        .map_err(|e| format!("Task error: {}", e));

        match result {
            Ok(Ok(text)) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Ok(Err(e)) => Ok(CallToolResult::error(vec![Content::text(e)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    // ── Mount info ──────────────────────────────────────────────

    #[tool(description = "Get SSHFS mount information. Shows local mount path, remote root, and path mapping. Use local paths for file access and remote paths for Docker execution.")]
    async fn get_mount_info(&self) -> Result<CallToolResult, ErrorData> {
        let mounted = ssh::is_mounted(&self.config);
        let available = ssh::check_sshfs_available();

        let text = format!(
            "SSHFS Mount Info:\n\
             - Local mount path: {}\n\
             - Remote mount root: {}\n\
             - SSHFS installed: {}\n\
             - Currently mounted: {}\n\n\
             Path mapping:\n\
             - Local: {}/some/path → Remote: {}/some/path\n\
             - Use LOCAL paths for file read/write (Claude can access directly)\n\
             - Use REMOTE paths for Docker execution tools (build_image, dry_run, execute_pipeline)",
            self.config.local_mount_path,
            self.config.remote_mount_root,
            available,
            mounted,
            self.config.local_mount_path.trim_end_matches('/'),
            self.config.remote_mount_root.trim_end_matches('/'),
        );

        Ok(CallToolResult::success(vec![Content::text(text)]))
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
                 Remote files are accessible locally via SSHFS mount.\n\
                 Use get_mount_info to see the local↔remote path mapping.\n\
                 - File access: use LOCAL paths (Claude can read/write directly)\n\
                 - Docker execution: use REMOTE paths (build_image, dry_run, execute_pipeline)"
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
