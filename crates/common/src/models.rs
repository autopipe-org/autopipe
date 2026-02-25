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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_content_strips_success_true_no_space() {
        let input = r#"{"success":true}rule all:
    input: "output.txt""#;
        let result = clean_content(input);
        assert!(result.starts_with("rule all:"), "Got: {}", result);
    }

    #[test]
    fn clean_content_strips_success_true_with_space() {
        let input = r#"{"success": true}FROM ubuntu:22.04"#;
        let result = clean_content(input);
        assert!(result.starts_with("FROM"), "Got: {}", result);
    }

    #[test]
    fn clean_content_splits_json_concatenation() {
        // metadata.json: {"success":true}{"name":"my-pipeline",...}
        let input = r#"{"success":true}{"name":"my-pipeline","description":"test"}"#;
        let result = clean_content(input);
        assert!(result.starts_with(r#"{"name":"my-pipeline""#), "Got: {}", result);
    }

    #[test]
    fn clean_content_leaves_normal_content_unchanged() {
        let input = "rule all:\n    input: \"output.txt\"";
        let result = clean_content(input);
        assert_eq!(result, input);
    }

    #[test]
    fn clean_content_handles_empty() {
        assert_eq!(clean_content(""), "");
        assert_eq!(clean_content("   "), "");
    }

    #[test]
    fn clean_content_leaves_normal_json_unchanged() {
        let input = r#"{"name":"pipeline","tools":["bwa"]}"#;
        let result = clean_content(input);
        assert_eq!(result, input);
    }

    #[test]
    fn clean_file_contents_cleans_all_fields() {
        let mut pipeline = Pipeline {
            pipeline_id: None,
            name: "test".into(),
            description: "test".into(),
            tools: vec![],
            input_formats: vec![],
            output_formats: vec![],
            tags: vec![],
            snakefile: r#"{"success":true}rule all:"#.into(),
            dockerfile: r#"{"success": true}FROM ubuntu"#.into(),
            config_yaml: r#"{"success":true}samples: test"#.into(),
            readme: r#"{"success":true}# README"#.into(),
            metadata_json: serde_json::json!({}),
            author: "test".into(),
            version: "1.0".into(),
            verified: false,
            created_at: None,
            updated_at: None,
        };

        pipeline.clean_file_contents();

        assert!(pipeline.snakefile.starts_with("rule all"), "snakefile: {}", pipeline.snakefile);
        assert!(pipeline.dockerfile.starts_with("FROM"), "dockerfile: {}", pipeline.dockerfile);
        assert!(pipeline.config_yaml.starts_with("samples"), "config: {}", pipeline.config_yaml);
        assert!(pipeline.readme.starts_with("# README"), "readme: {}", pipeline.readme);
    }
}
