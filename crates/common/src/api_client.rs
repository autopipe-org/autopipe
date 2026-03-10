use crate::models::{Pipeline, PipelineSummary, Plugin, PluginSummary};
use reqwest::Client;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Server error: {status} - {message}")]
    Server { status: u16, message: String },
}

/// HTTP client for the AutoPipe registry REST API.
#[derive(Debug, Clone)]
pub struct RegistryClient {
    base_url: String,
    client: Client,
}

impl RegistryClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: Client::new(),
        }
    }

    // ── Pipeline methods ────────────────────────────────────────

    /// Search pipelines by keyword.
    pub async fn search(&self, query: &str) -> Result<Vec<PipelineSummary>, ApiError> {
        let resp = self
            .client
            .get(format!("{}/api/pipelines", self.base_url))
            .query(&[("q", query)])
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ApiError::Server {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }
        Ok(resp.json().await?)
    }

    /// List all pipelines.
    pub async fn list(&self) -> Result<Vec<PipelineSummary>, ApiError> {
        let resp = self
            .client
            .get(format!("{}/api/pipelines", self.base_url))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ApiError::Server {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }
        Ok(resp.json().await?)
    }

    /// Get a pipeline by ID (metadata only, code is on GitHub).
    pub async fn get_pipeline(&self, id: i32) -> Result<Pipeline, ApiError> {
        let resp = self
            .client
            .get(format!("{}/api/pipelines/{}", self.base_url, id))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ApiError::Server {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }
        Ok(resp.json().await?)
    }

    /// Delete a pipeline by ID.
    pub async fn delete(&self, id: i32) -> Result<(), ApiError> {
        let resp = self
            .client
            .delete(format!("{}/api/pipelines/{}", self.base_url, id))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ApiError::Server {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }
        Ok(())
    }

    // ── Plugin methods ──────────────────────────────────────────

    /// Search plugins by keyword.
    pub async fn search_plugins(&self, query: &str) -> Result<Vec<PluginSummary>, ApiError> {
        let resp = self
            .client
            .get(format!("{}/api/plugins", self.base_url))
            .query(&[("q", query)])
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ApiError::Server {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }
        Ok(resp.json().await?)
    }

    /// List all plugins.
    pub async fn list_plugins(&self) -> Result<Vec<PluginSummary>, ApiError> {
        let resp = self
            .client
            .get(format!("{}/api/plugins", self.base_url))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ApiError::Server {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }
        Ok(resp.json().await?)
    }

    /// Get a plugin by exact name.
    pub async fn get_plugin_by_name(&self, name: &str) -> Result<Option<Plugin>, ApiError> {
        let resp = self
            .client
            .get(format!("{}/api/plugins", self.base_url))
            .query(&[("name", name)])
            .send()
            .await?;

        if resp.status().as_u16() == 404 {
            return Ok(None);
        }
        if !resp.status().is_success() {
            return Err(ApiError::Server {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }
        Ok(Some(resp.json().await?))
    }
}

// ── WorkflowHub integration ─────────────────────────────────────────

/// Detect registry type from URL.
pub fn detect_registry_type(url: &str) -> RegistryType {
    if url.contains("workflowhub.eu") {
        RegistryType::WorkflowHub
    } else {
        RegistryType::AutoPipe
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RegistryType {
    AutoPipe,
    WorkflowHub,
}

/// HTTP client for WorkflowHub GA4GH TRS API.
#[derive(Debug, Clone)]
pub struct WorkflowHubClient {
    base_url: String,
    client: Client,
}

impl WorkflowHubClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: Client::new(),
        }
    }

    /// Search workflows on WorkflowHub by keyword.
    pub async fn search(&self, query: &str) -> Result<Vec<PipelineSummary>, ApiError> {
        let resp = self
            .client
            .get(format!("{}/workflows.json", self.base_url))
            .query(&[("filter[query]", query)])
            .header("Accept", "application/json")
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ApiError::Server {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }

        let body: serde_json::Value = resp.json().await?;
        Ok(self.convert_workflows(&body))
    }

    /// List workflows on WorkflowHub.
    pub async fn list(&self) -> Result<Vec<PipelineSummary>, ApiError> {
        let resp = self
            .client
            .get(format!("{}/workflows.json", self.base_url))
            .header("Accept", "application/json")
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ApiError::Server {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }

        let body: serde_json::Value = resp.json().await?;
        Ok(self.convert_workflows(&body))
    }

    /// Get detailed workflow info (description, tags, creator) from WorkflowHub JSON API.
    pub async fn get_workflow_detail(&self, id: i32) -> Result<serde_json::Value, ApiError> {
        let resp = self
            .client
            .get(format!("{}/workflows/{}.json", self.base_url, id))
            .header("Accept", "application/json")
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ApiError::Server {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }
        Ok(resp.json().await?)
    }

    /// Download workflow files (Snakefile) from WorkflowHub via GA4GH TRS API.
    /// Returns a list of (filename, content) tuples.
    pub async fn get_workflow_files(&self, id: i32) -> Result<Vec<(String, String)>, ApiError> {
        // First, get the latest version ID from the workflow detail
        let detail = self.get_workflow_detail(id).await?;
        let version_id = detail
            .get("data")
            .and_then(|d| d.get("attributes"))
            .and_then(|a| a.get("latest_version"))
            .and_then(|v| v.as_i64())
            .unwrap_or(1);

        // Use GA4GH TRS API to get Snakemake files
        let url = format!(
            "{}/ga4gh/trs/v2/tools/{}/versions/{}/PLAIN_SMK/files",
            self.base_url, id, version_id
        );
        let resp = self
            .client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ApiError::Server {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }

        let files: serde_json::Value = resp.json().await?;
        let mut result = Vec::new();

        if let Some(arr) = files.as_array() {
            for file_entry in arr {
                let path = file_entry
                    .get("path")
                    .and_then(|p| p.as_str())
                    .unwrap_or("Snakefile");

                // Download the actual file content
                let file_url = file_entry
                    .get("file_wrapper_url")
                    .or_else(|| file_entry.get("url"))
                    .and_then(|u| u.as_str());

                if let Some(url) = file_url {
                    // file_wrapper_url returns JSON with "content" field
                    let content_resp = self
                        .client
                        .get(url)
                        .header("Accept", "application/json")
                        .send()
                        .await;

                    if let Ok(resp) = content_resp {
                        if resp.status().is_success() {
                            let body: serde_json::Value =
                                resp.json().await.unwrap_or_default();
                            if let Some(content) = body.get("content").and_then(|c| c.as_str()) {
                                result.push((path.to_string(), content.to_string()));
                            }
                        }
                    }
                }
            }
        }

        Ok(result)
    }

    /// Convert WorkflowHub JSON response to Vec<PipelineSummary>.
    fn convert_workflows(&self, body: &serde_json::Value) -> Vec<PipelineSummary> {
        let data = match body.get("data").and_then(|d| d.as_array()) {
            Some(arr) => arr,
            None => return vec![],
        };

        data.iter()
            .filter_map(|item| {
                let id = item.get("id")?.as_str()?.parse::<i32>().ok()?;
                let attrs = item.get("attributes")?;
                let name = attrs.get("title")?.as_str()?.to_string();
                let description = attrs
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string();
                let tags = attrs
                    .get("tags")
                    .and_then(|t| t.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();

                Some(PipelineSummary {
                    pipeline_id: id,
                    name,
                    description,
                    tools: vec![],
                    input_formats: vec![],
                    output_formats: vec![],
                    tags,
                    github_url: format!("{}/workflows/{}", self.base_url, id),
                    author: String::new(),
                    version: String::from("1.0.0"),
                    verified: false,
                    forked_from: None,
                    created_at: None,
                    source: Some("workflowhub".to_string()),
                })
            })
            .collect()
    }
}
