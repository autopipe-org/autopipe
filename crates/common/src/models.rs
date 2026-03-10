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
    pub github_url: String,
    pub metadata_json: serde_json::Value,
    pub author: String,
    pub version: String,
    pub verified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_from: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Summary returned from list/search (without full details).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineSummary {
    pub pipeline_id: i32,
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
    pub input_formats: Vec<String>,
    pub output_formats: Vec<String>,
    pub tags: Vec<String>,
    pub github_url: String,
    pub author: String,
    pub version: String,
    pub verified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_from: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// Source registry: "autopipehub" or "workflowhub". Absent means autopipehub.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// Metadata parsed from ro-crate-metadata.json inside a pipeline directory.
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
    /// URL of the original workflow this pipeline is based on (e.g., WorkflowHub page).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub based_on_url: Option<String>,
}

/// Parse pipeline metadata from RO-Crate format (ro-crate-metadata.json).
/// Extracts fields from the @graph Dataset node and converts to PipelineMetadata.
pub fn parse_ro_crate_metadata(json_str: &str) -> Result<PipelineMetadata, String> {
    let v: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| format!("Invalid JSON: {}", e))?;

    // Find the Dataset node in @graph
    let graph = v.get("@graph")
        .and_then(|g| g.as_array())
        .ok_or("Missing @graph array")?;

    let dataset = graph.iter()
        .find(|node| {
            node.get("@id").and_then(|id| id.as_str()) == Some("./")
        })
        .ok_or("Missing Dataset node (@id: \"./\") in @graph")?;

    let name = dataset.get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let description = dataset.get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let version = dataset.get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("1.0.0")
        .to_string();

    // Extract creator name from referenced Person node
    let author = dataset.get("creator")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|creator_ref| {
            let creator_id = creator_ref.get("@id").and_then(|v| v.as_str())?;
            // Look up the Person node in @graph
            graph.iter()
                .find(|node| node.get("@id").and_then(|id| id.as_str()) == Some(creator_id))
                .and_then(|node| node.get("name").and_then(|v| v.as_str()))
                .map(|s| s.to_string())
        })
        .unwrap_or_default();

    // Extract tool names from softwareRequirements references
    let tools = dataset.get("softwareRequirements")
        .and_then(|s| s.as_array())
        .map(|arr| {
            arr.iter().filter_map(|tool_ref| {
                let tool_id = tool_ref.get("@id").and_then(|v| v.as_str())?;
                graph.iter()
                    .find(|node| node.get("@id").and_then(|id| id.as_str()) == Some(tool_id))
                    .and_then(|node| node.get("name").and_then(|v| v.as_str()))
                    .map(|s| s.to_string())
            }).collect()
        })
        .unwrap_or_default();

    // Extract input format names from FormalParameter references
    let input_formats = dataset.get("input")
        .and_then(|s| s.as_array())
        .map(|arr| {
            arr.iter().filter_map(|param_ref| {
                let param_id = param_ref.get("@id").and_then(|v| v.as_str())?;
                graph.iter()
                    .find(|node| node.get("@id").and_then(|id| id.as_str()) == Some(param_id))
                    .and_then(|node| node.get("name").and_then(|v| v.as_str()))
                    .map(|s| s.to_string())
            }).collect()
        })
        .unwrap_or_default();

    // Extract output format names from FormalParameter references
    let output_formats = dataset.get("output")
        .and_then(|s| s.as_array())
        .map(|arr| {
            arr.iter().filter_map(|param_ref| {
                let param_id = param_ref.get("@id").and_then(|v| v.as_str())?;
                graph.iter()
                    .find(|node| node.get("@id").and_then(|id| id.as_str()) == Some(param_id))
                    .and_then(|node| node.get("name").and_then(|v| v.as_str()))
                    .map(|s| s.to_string())
            }).collect()
        })
        .unwrap_or_default();

    let tags = dataset.get("keywords")
        .and_then(|k| k.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let verified = false; // RO-Crate doesn't have this; default to false

    // Extract isBasedOn URL (e.g., WorkflowHub source)
    let based_on_url = dataset.get("isBasedOn")
        .and_then(|b| {
            // Can be {"@id": "url"} or just a string
            b.get("@id").and_then(|id| id.as_str())
                .or_else(|| b.as_str())
        })
        .map(|s| s.to_string());

    Ok(PipelineMetadata {
        name,
        description,
        version,
        author,
        tools,
        input_formats,
        output_formats,
        tags,
        verified,
        based_on_url,
    })
}

/// A plugin stored in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plugin {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<i32>,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub category: String,
    pub tags: Vec<String>,
    pub github_url: String,
    pub metadata_json: serde_json::Value,
    pub author: String,
    pub version: String,
    pub verified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_from: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Summary returned from list/search for plugins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSummary {
    pub plugin_id: i32,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub category: String,
    pub tags: Vec<String>,
    pub github_url: String,
    pub author: String,
    pub version: String,
    pub verified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_from: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
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
}
