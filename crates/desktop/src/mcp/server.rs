use common::api_client::RegistryClient;
use common::models::{Pipeline, PipelineMetadata};
use common::templates;
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::{tool, tool_handler, tool_router, ErrorData, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::Deserialize;
use std::path::Path;
use std::process::Command;

use crate::config::AppConfig;

// Parameter structs for MCP tools
#[derive(Debug, Deserialize, JsonSchema)]
struct SearchParams {
    /// Search keyword
    query: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DownloadParams {
    /// Pipeline ID to download
    pipeline_id: i32,
    /// Output directory path
    output_dir: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PipelineDirParams {
    /// Path to the pipeline directory
    pipeline_dir: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct BuildParams {
    /// Path to the pipeline directory
    pipeline_dir: String,
    /// Docker image name/tag
    image_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DryRunParams {
    /// Docker image name
    image_name: String,
    /// Input data directory (mounted as read-only)
    input_dir: String,
    /// Output directory
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
    /// Input data directory (mounted as read-only)
    input_dir: String,
    /// Output directory
    output_dir: String,
    /// Number of CPU cores (default: 8)
    cores: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct StatusParams {
    /// tmux session name or container name prefix
    session_name: String,
}

#[derive(Clone)]
pub struct AutoPipeServer {
    registry: RegistryClient,
    #[allow(dead_code)]
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

    #[tool(description = "Download a pipeline by ID to a local directory")]
    async fn download_pipeline(
        &self,
        Parameters(params): Parameters<DownloadParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.registry.download(params.pipeline_id).await {
            Ok(pipeline) => {
                let dir = Path::new(&params.output_dir).join(&pipeline.name);
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    return Ok(CallToolResult::error(vec![Content::text(format!(
                        "Failed to create directory: {}",
                        e
                    ))]));
                }

                let meta_str =
                    serde_json::to_string_pretty(&pipeline.metadata_json).unwrap_or_default();
                let files: [(&str, &str); 5] = [
                    ("Snakefile", &pipeline.snakefile),
                    ("Dockerfile", &pipeline.dockerfile),
                    ("config.yaml", &pipeline.config_yaml),
                    ("metadata.json", &meta_str),
                    ("README.md", &pipeline.readme),
                ];

                for (name, content) in &files {
                    if let Err(e) = std::fs::write(dir.join(name), content) {
                        return Ok(CallToolResult::error(vec![Content::text(format!(
                            "Failed to write {}: {}",
                            name, e
                        ))]));
                    }
                }

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Downloaded pipeline '{}' to {}",
                    pipeline.name,
                    dir.display()
                ))]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Download failed: {}",
                e
            ))])),
        }
    }

    #[tool(description = "Upload a pipeline directory to the registry")]
    async fn upload_pipeline(
        &self,
        Parameters(params): Parameters<PipelineDirParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dir = Path::new(&params.pipeline_dir);
        let meta_path = dir.join("metadata.json");
        let meta_content = match std::fs::read_to_string(&meta_path) {
            Ok(c) => c,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Cannot read metadata.json: {}",
                    e
                ))]))
            }
        };
        let metadata: PipelineMetadata = match serde_json::from_str(&meta_content) {
            Ok(m) => m,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid metadata.json: {}",
                    e
                ))]))
            }
        };

        let read_file = |name: &str| -> String {
            std::fs::read_to_string(dir.join(name)).unwrap_or_default()
        };

        let pipeline = Pipeline {
            pipeline_id: None,
            name: metadata.name.clone(),
            description: metadata.description,
            tools: metadata.tools,
            input_formats: metadata.input_formats,
            output_formats: metadata.output_formats,
            tags: metadata.tags,
            snakefile: read_file("Snakefile"),
            dockerfile: read_file("Dockerfile"),
            config_yaml: read_file("config.yaml"),
            metadata_json: serde_json::from_str(&meta_content).unwrap_or_default(),
            readme: read_file("README.md"),
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

    #[tool(description = "Validate a pipeline directory structure and files")]
    async fn validate_pipeline(
        &self,
        Parameters(params): Parameters<PipelineDirParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dir = Path::new(&params.pipeline_dir);
        let mut errors: Vec<String> = Vec::new();

        let required = [
            "Snakefile",
            "Dockerfile",
            "config.yaml",
            "metadata.json",
            "README.md",
        ];
        for f in &required {
            let path = dir.join(f);
            if !path.exists() {
                errors.push(format!("Missing: {}", f));
            } else if std::fs::metadata(&path)
                .map(|m| m.len() == 0)
                .unwrap_or(true)
            {
                errors.push(format!("Empty: {}", f));
            }
        }

        if let Ok(content) = std::fs::read_to_string(dir.join("Snakefile")) {
            if !content.contains("rule all") {
                errors.push("Snakefile: missing 'rule all'".into());
            }
        }

        if let Ok(content) = std::fs::read_to_string(dir.join("metadata.json")) {
            match serde_json::from_str::<PipelineMetadata>(&content) {
                Ok(m) => {
                    if m.name.is_empty() {
                        errors.push("metadata.json: 'name' is empty".into());
                    }
                    if m.tools.is_empty() {
                        errors.push("metadata.json: 'tools' is empty".into());
                    }
                }
                Err(e) => errors.push(format!("metadata.json: invalid JSON - {}", e)),
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

    #[tool(description = "Build a Docker image for a pipeline directory")]
    async fn build_image(
        &self,
        Parameters(params): Parameters<BuildParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let output = Command::new("docker")
            .args(["build", "-t", &params.image_name, &params.pipeline_dir])
            .output();

        match output {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                let stderr = String::from_utf8_lossy(&o.stderr);
                if o.status.success() {
                    Ok(CallToolResult::success(vec![Content::text(format!(
                        "Docker image '{}' built successfully.\n{}",
                        params.image_name, stdout
                    ))]))
                } else {
                    Ok(CallToolResult::error(vec![Content::text(format!(
                        "Docker build failed:\n{}\n{}",
                        stdout, stderr
                    ))]))
                }
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to run docker: {}",
                e
            ))])),
        }
    }

    #[tool(description = "Dry-run a pipeline (snakemake -n -p) without executing")]
    async fn dry_run(
        &self,
        Parameters(params): Parameters<DryRunParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let cores_str = params.cores.unwrap_or(8).to_string();
        let output = Command::new("docker")
            .args([
                "run",
                "--rm",
                "-v",
                &format!("{}:/input:ro", params.input_dir),
                "-v",
                &format!("{}:/output", params.output_dir),
                &params.image_name,
                "snakemake",
                "--cores",
                &cores_str,
                "-n",
                "-p",
            ])
            .output();

        match output {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                let stderr = String::from_utf8_lossy(&o.stderr);
                let text = format!("{}\n{}", stdout, stderr);
                if o.status.success() {
                    Ok(CallToolResult::success(vec![Content::text(text)]))
                } else {
                    Ok(CallToolResult::error(vec![Content::text(text)]))
                }
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to run docker: {}",
                e
            ))])),
        }
    }

    #[tool(description = "Execute a pipeline in a tmux session for real-time monitoring")]
    async fn execute_pipeline(
        &self,
        Parameters(params): Parameters<ExecuteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let cores_str = params.cores.unwrap_or(8).to_string();
        let docker_cmd = format!(
            "docker run --rm --name {}-run -v {}:/input:ro -v {}:/output {} snakemake --cores {}",
            params.session_name, params.input_dir, params.output_dir, params.image_name, cores_str,
        );

        let output = Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s",
                &params.session_name,
                &docker_cmd,
            ])
            .output();

        match output {
            Ok(o) if o.status.success() => Ok(CallToolResult::success(vec![Content::text(
                format!(
                    "Pipeline started in tmux session '{}'.\nMonitor: tmux attach -t {}\nDetach: Ctrl+B then D",
                    params.session_name, params.session_name
                ),
            )])),
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                Ok(CallToolResult::error(vec![Content::text(format!(
                    "Failed to start tmux session: {}",
                    stderr
                ))]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to run tmux: {}",
                e
            ))])),
        }
    }

    #[tool(description = "Check pipeline execution status via tmux or docker logs")]
    async fn check_status(
        &self,
        Parameters(params): Parameters<StatusParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let tmux_output = Command::new("tmux")
            .args([
                "capture-pane",
                "-t",
                &params.session_name,
                "-p",
                "-l",
                "30",
            ])
            .output();

        if let Ok(o) = tmux_output {
            if o.status.success() {
                let text = String::from_utf8_lossy(&o.stdout);
                return Ok(CallToolResult::success(vec![Content::text(format!(
                    "tmux session '{}' output:\n{}",
                    params.session_name, text
                ))]));
            }
        }

        let container_name = format!("{}-run", params.session_name);
        let docker_output = Command::new("docker")
            .args(["logs", "--tail", "30", &container_name])
            .output();

        match docker_output {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                let stderr = String::from_utf8_lossy(&o.stderr);
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Docker logs for '{}':\n{}\n{}",
                    container_name, stdout, stderr
                ))]))
            }
            Err(_) => Ok(CallToolResult::error(vec![Content::text(format!(
                "No active session or container found for '{}'",
                params.session_name
            ))])),
        }
    }

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
                "AutoPipe MCP Server - Manage bioinformatics Snakemake pipelines. \
                 Search, download, upload, validate, build, and execute pipelines."
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
