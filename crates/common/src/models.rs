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

// ── Helpers ─────────────────────────────────────────────────────────

/// Strip any prepended `{"success":true}` or `{"success": true}` from content.
/// This artifact can appear when files are written via SSH and read back.
pub fn clean_content(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() {
        return s.to_string();
    }
    // JSON files: {"success": true}{"name": ...} → split on first }{
    if s.starts_with('{') {
        if let Some(pos) = s.find("}{") {
            let after = &s[pos + 1..];
            // Only split if the second part looks like a real JSON object
            if after.starts_with('{') {
                return after.to_string();
            }
        }
    }
    // Non-JSON files: strip known prefixes
    for prefix in &[
        r#"{"success":true}"#,
        r#"{"success": true}"#,
        r#"{"success" : true}"#,
    ] {
        if s.starts_with(prefix) {
            return s[prefix.len()..].trim_start().to_string();
        }
    }
    s.to_string()
}

impl Pipeline {
    /// Remove `{"success":true}` prefix from all file content fields.
    pub fn clean_file_contents(&mut self) {
        self.snakefile = clean_content(&self.snakefile);
        self.dockerfile = clean_content(&self.dockerfile);
        self.config_yaml = clean_content(&self.config_yaml);
        self.readme = clean_content(&self.readme);
    }
}

/// Search query parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
}
