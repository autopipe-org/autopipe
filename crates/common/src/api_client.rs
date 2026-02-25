use crate::models::{Pipeline, PipelineSummary, PluginSummary};
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
}
