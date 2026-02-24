use serde::{Deserialize, Serialize};

/// A bioinformatics pipeline stored in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipeline {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline_id: Option<i32>,
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
    pub input_formats: Vec<String>,
    pub output_formats: Vec<String>,
    pub tags: Vec<String>,
    pub snakefile: String,
    pub dockerfile: String,
    pub config_yaml: String,
    pub metadata_json: serde_json::Value,
    pub readme: String,
    pub author: String,
    pub version: String,
    pub verified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Summary returned from list/search (without full file contents).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineSummary {
    pub pipeline_id: i32,
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
    pub input_formats: Vec<String>,
    pub output_formats: Vec<String>,
    pub tags: Vec<String>,
    pub author: String,
    pub version: String,
    pub verified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

/// Metadata parsed from metadata.json inside a pipeline directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineMetadata {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub input_formats: Vec<String>,
    #[serde(default)]
    pub output_formats: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub verified: bool,
}

/// Search query parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
}
