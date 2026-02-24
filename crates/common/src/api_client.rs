use crate::models::{Pipeline, PipelineSummary};
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

    /// Download a pipeline by ID (includes file contents).
    pub async fn download(&self, id: i32) -> Result<Pipeline, ApiError> {
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

    /// Upload a new pipeline. Returns the assigned pipeline_id.
    pub async fn upload(&self, pipeline: &Pipeline) -> Result<i32, ApiError> {
        let resp = self
            .client
            .post(format!("{}/api/pipelines", self.base_url))
            .json(pipeline)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ApiError::Server {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }

        #[derive(serde::Deserialize)]
        struct Created {
            pipeline_id: i32,
        }
        let created: Created = resp.json().await?;
        Ok(created.pipeline_id)
    }

    /// Update an existing pipeline.
    pub async fn update(&self, id: i32, pipeline: &Pipeline) -> Result<(), ApiError> {
        let resp = self
            .client
            .put(format!("{}/api/pipelines/{}", self.base_url, id))
            .json(pipeline)
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
}
