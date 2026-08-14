use common::api_client::RegistryClient;
use common::models::{clean_content, parse_ro_crate_metadata, PipelineMetadata};
use common::templates;
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::{tool, tool_handler, tool_router, ErrorData, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::Deserialize;

use std::collections::HashSet;
use std::net::SocketAddr;

use crate::config::{AppConfig, SshAuth, DEFAULT_MCP_PORT, MCP_PORT_FALLBACK_RANGE};
use crate::mcp::path_translate::windows_to_wsl;
use crate::mcp::viewer;
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
    /// Remote directory path (optional, defaults to configured pipelines directory)
    output_dir: Option<String>,
    /// Set to true ONLY after the AI has reviewed the pipeline's source
    /// code (returned by the first call of this tool) for suspicious
    /// patterns and, if anything risky was found, the user has explicitly
    /// confirmed they want to proceed anyway. The first call (with this
    /// flag absent or false) returns a code dump for review and does NOT
    /// download anything; the second call (with this flag true) performs
    /// the actual download.
    #[serde(default)]
    user_reviewed_warnings: Option<bool>,
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
struct CheckBuildParams {
    /// Docker image name (same as used in build_image)
    image_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DryRunParams {
    /// Docker image name
    image_name: String,
    /// Input data directory the remote server can reach (mounted read-only in Docker).
    /// Windows-style paths like `C:\Users\me\data` are accepted and auto-translated to WSL form;
    /// pass through whatever the user gave you.
    input_dir: String,
    /// Remote output directory (optional, defaults to configured output directory)
    output_dir: Option<String>,
    /// Number of CPU cores (default: 8)
    cores: Option<u32>,
    /// Set to true if the pipeline needs Docker access inside the container (e.g., nextflow -profile docker, docker run, singularity exec). Mounts the host Docker socket.
    #[serde(default)]
    needs_docker_socket: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ExecuteParams {
    /// Docker image name
    image_name: String,
    /// Run name (used for log file and container naming)
    run_name: String,
    /// Input data directory the remote server can reach (mounted read-only in Docker).
    /// Windows-style paths like `C:\Users\me\data` are accepted and auto-translated to WSL form;
    /// pass through whatever the user gave you.
    input_dir: String,
    /// Number of CPU cores (default: 8)
    cores: Option<u32>,
    /// Set to true if the pipeline needs Docker access inside the container (e.g., nextflow -profile docker, docker run, singularity exec). Mounts the host Docker socket.
    #[serde(default)]
    needs_docker_socket: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct MountS3Params {
    /// S3 bucket to mount. If omitted, uses the bucket selected in the AutoPipe app.
    #[serde(default)]
    bucket: Option<String>,
    /// Optional key prefix within the bucket to mount (e.g. "runs/2024/").
    #[serde(default)]
    prefix: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct StatusParams {
    /// Run name (matches the run_name used in execute_pipeline)
    run_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DeletePipelineParams {
    /// Full path to the pipeline source directory on the remote server (e.g. /home/user/pipelines/my-pipeline)
    pipeline_dir: String,
    /// Docker image name to remove along with the pipeline (e.g. autopipe-my-pipeline). Optional — omit if no image was built.
    #[serde(default)]
    image_name: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CleanupFailedParams {
    /// Docker image name (e.g. autopipe-my-pipeline)
    image_name: String,
    /// Run name of the failed run (matches the output subdirectory name)
    run_name: String,
    /// Whether to remove the output directory (default: false). Only set to true when you want a completely fresh start. Completed intermediate results are preserved by default so Snakemake can resume from where it left off.
    remove_output: Option<bool>,
    /// Whether to stop running containers before cleaning up (default: false). When false, cleanup aborts if the image has a running container. Set to true to stop the run first (docker stop) — use this when the user asks to stop/abort a run, or when a run is hung.
    stop_running: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CreateSymlinkParams {
    /// Existing file/directory the link should point to. Windows-style paths
    /// (e.g. `C:\data\foo`) are accepted and auto-translated to WSL form.
    source: String,
    /// Symlink path to create. Same path-translation rules as `source`.
    target: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RemoveSymlinkParams {
    /// Path of the symlink to remove on the remote server
    symlink_path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PrepareInputParams {
    /// URL to download (http/https/ftp) OR an absolute filesystem path that the remote server
    /// can reach. Windows-style paths (e.g. `C:\Users\me\data\input.fastq`) are accepted and
    /// translated to WSL form (`/mnt/c/Users/me/data/input.fastq`) automatically — pass them
    /// through as-is, do not ask the user to convert. If it starts with http://, https://, or
    /// ftp://, the file is downloaded via Docker (wget/curl). Otherwise, a symlink is created
    /// pointing to the given path.
    source: String,
    /// Subdirectory within pipelines_input to place the file (e.g. "run-001"). Optional.
    #[serde(default)]
    subdir: Option<String>,
    /// Filename override for URL downloads. If omitted, inferred from the URL. Ignored for symlinks.
    #[serde(default)]
    filename: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CheckDownloadParams {
    /// The filename used when prepare_input started the download (returned in the prepare_input response).
    filename: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RemoveInputParams {
    /// Path relative to pipelines_input to remove (e.g. "run-001/data.fastq" or "data.fastq").
    /// Symlinks are removed directly; regular/root-owned files are removed via Docker.
    relative_path: String,
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
    /// Set to true ONLY after the user has explicitly agreed to let you read a
    /// file located in the input directory (user-provided data, which may be
    /// sensitive). Leave unset otherwise — if a read is gated the tool returns
    /// instructions to ask the user first, and you re-call with this set to true.
    #[serde(default)]
    confirm_read: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct WriteFileParams {
    /// Remote file path to write to
    path: String,
    /// Content to write to the file
    content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ShowResultsParams {
    /// Remote file or directory path to display in the browser.
    path: String,
    /// Optional filter: "image", "text", "genomics", "pdf", "hdf5".
    /// When set, only files matching this type are loaded into the viewer.
    /// Omit to load all files.
    #[serde(default)]
    filter: Option<String>,
    /// Optional reference genome for IGV viewer. Can be:
    /// - A genome ID string like "hg38", "hg19", "mm10", "mm39", "dm6", "ce11", "danRer11", "sacCer3", "rn7", "galGal6"
    /// - A FASTA filename that exists among the result files (e.g., "reference.fasta")
    /// When provided, the IGV tab becomes available for genomics files (BAM, VCF, BED, GFF, FASTA).
    /// Without reference, only the Data tab is shown for these formats.
    /// CRAM and BCF files require a reference for viewing.
    /// If the user has a local reference FASTA in the results directory, pass its filename.
    /// Otherwise, determine the organism from context and pass the appropriate genome ID.
    #[serde(default)]
    reference: Option<String>,
    /// Pipeline-viewer confirmation. When a directory is recognised as a known
    /// pipeline's output, show_results does NOT open — it returns a prompt to
    /// ask the user. Then call again with "yes" (open the dedicated pipeline
    /// viewer) or "no" (open files by extension as usual). Leave unset on the
    /// first call.
    #[serde(default)]
    pipeline_viewer: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DownloadResultsParams {
    /// Remote file or directory path to download from the SSH server
    remote_path: String,
    /// Local directory to save the downloaded file(s). If omitted, uses the OS default Downloads folder (e.g., ~/Downloads on macOS/Linux, C:\Users\<user>\Downloads on Windows). Tell the user the default path and ask if they want to change it before calling.
    #[serde(default)]
    local_dir: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct UploadPipelineParams {
    /// Remote path to the pipeline directory on the SSH server
    pipeline_dir: String,
    /// List of file paths relative to pipeline_dir that are required to run the pipeline.
    /// You MUST include every file you created for this pipeline (e.g., ["Snakefile", "Dockerfile", "config.yaml", "ro-crate-metadata.json", "README.md", "scripts/annotate.py"]).
    /// Paths can include subdirectories (e.g., "scripts/run.py").
    /// ro-crate-metadata.json is always required and will be added automatically if omitted.
    files: Vec<String>,
    /// Git commit message (optional, auto-generated if omitted)
    commit_message: Option<String>,
    /// GitHub repository name. Required only when per-pipeline repo mode is enabled. Omit when using single repo mode.
    #[serde(default)]
    repo_name: Option<String>,
    /// Set to true ONLY after the user has explicitly confirmed they want to
    /// upload into a target that already contains files not recorded in the
    /// Hub. Leave unset on the first call — if a conflict is detected the tool
    /// returns instructions to ask the user, and you re-call with this set to
    /// true once they agree. See the directory-conflict guidance in the result.
    #[serde(default)]
    confirm_overwrite: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PublishPipelineParams {
    /// GitHub URL of the uploaded pipeline (from upload_pipeline result)
    github_url: String,
    /// Set to an existing pipeline_id to link this as a related/derived version. Same-name pipelines are auto-linked. Use this for cross-name forks (e.g., a similar pipeline with a different name).
    forked_from: Option<i32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct UnpublishParams {
    /// Pipeline ID to unpublish from Autopipe Hub.
    pipeline_id: i64,
    /// 'latest' = delete only the most recent version in this pipeline's
    /// version chain (same name + same author). 'all' = delete every
    /// version. Omit on the first call to discover how many versions
    /// exist; the tool will return the version list and ask you to call
    /// again with a scope.
    #[serde(default)]
    scope: Option<String>,
}

// ── Server ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AutoPipeServer {
    #[allow(dead_code)]
    registry: RegistryClient,
    #[allow(dead_code)]
    config: AppConfig,
    tool_router: ToolRouter<Self>,
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Escape a string for safe use inside single-quoted shell arguments.
/// Rejects null bytes and newlines (which can break out of commands),
/// then replaces each `'` with `'\''` (end quote, escaped quote, start quote).
/// Effective viewer extension for a filename, recognising the compound forms
/// produced by bgzip (`.vcf.gz`, `.bed.gz`, `.gff.gz`, `.gtf.gz`). A plain
/// `rsplit('.')` would see only `gz` and route these nowhere; here the whole
/// two-part suffix is returned so it matches a plugin's declared extension.
/// Everything else falls back to the last dotted segment.
const COMPOUND_GZ_EXTS: &[&str] = &[
    "vcf.gz", "bed.gz", "gff.gz", "gff3.gz", "gtf.gz",
    "fasta.gz", "fa.gz", "fastq.gz", "fq.gz",
    "csv.gz", "tsv.gz", "txt.gz", "log.gz",
];

fn viewer_ext(name: &str) -> String {
    let lower = name.to_lowercase();
    for compound in COMPOUND_GZ_EXTS {
        if lower.ends_with(&format!(".{}", compound)) {
            return compound.to_string();
        }
    }
    lower.rsplit('.').next().unwrap_or_default().to_string()
}

fn shell_escape(s: &str) -> String {
    // Strip characters that can break shell command boundaries
    let sanitized: String = s.chars()
        .filter(|c| *c != '\0' && *c != '\n' && *c != '\r')
        .collect();
    sanitized.replace('\'', "'\\''")
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

/// Parse a GitHub URL into (owner, repo, path).
/// Supports formats like:
///   https://github.com/{owner}/{repo}/tree/{branch}/{path}
///   https://github.com/{owner}/{repo}
/// Parse a GitHub URL into (owner, repo, branch_or_tag, subpath).
/// Best-effort: fetch the GitHub `login` (username) for the configured
/// personal access token. Returns `None` if no token is set, the token
/// is invalid, or the request fails. Never panics — callers decide how
/// to handle the absence of a login.
pub(crate) async fn fetch_github_login(token: Option<&str>) -> Option<String> {
    let token = token.filter(|t| !t.is_empty())?;
    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", "autopipe-desktop")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: serde_json::Value = resp.json().await.ok()?;
    v["login"].as_str().map(String::from)
}

/// Walk the `@graph` array in a parsed ro-crate-metadata.json and set the
/// Person `#author` node's `name` field to `login`. Returns the rewritten
/// JSON, pretty-printed for human readability when committed to GitHub.
pub(crate) fn fill_ro_crate_author(content: &str, login: &str) -> Result<String, String> {
    let mut v: serde_json::Value =
        serde_json::from_str(content).map_err(|e| format!("parse: {}", e))?;
    let graph = v["@graph"]
        .as_array_mut()
        .ok_or_else(|| "@graph is not an array".to_string())?;
    for node in graph.iter_mut() {
        if node["@id"].as_str() == Some("#author") {
            node["name"] = serde_json::Value::String(login.to_string());
        }
    }
    serde_json::to_string_pretty(&v).map_err(|e| format!("serialize: {}", e))
}

/// Return true if `path` looks like a source/config file the AI should
/// review before download. The list is intentionally permissive on the
/// "obviously source code" side (Snakefile, Dockerfile, Makefile, common
/// script extensions) and restrictive on the data side (no BAM, no FASTA,
/// no images, no compiled binaries) so the dump stays small and useful.
pub(crate) fn is_code_review_target(path: &str) -> bool {
    let lower = path.to_lowercase();
    let basename = lower.rsplit('/').next().unwrap_or(&lower);
    if basename == "snakefile" || basename == "dockerfile" || basename == "makefile" {
        return true;
    }
    const EXTS: &[&str] = &[
        ".py", ".r", ".rmd", ".sh", ".bash", ".zsh", ".yaml", ".yml",
        ".json", ".jsonl", ".toml", ".md", ".txt", ".ipynb", ".sql",
        ".pl", ".rb", ".js", ".ts",
    ];
    EXTS.iter().any(|e| lower.ends_with(e))
}

// ── Deterministic pipeline-review checks ────────────────────────────
//
// These produce *factual* anchors (with file:line where possible) that the
// calling AI must cite in its natural-language summary. They are intentionally
// conservative: high-signal, low-false-positive checks only. Semantic/logic
// review is left to the AI reading the full source.

/// A single deterministic finding with an optional file:line anchor.
pub(crate) struct ReviewFinding {
    /// "issue" (likely wrong), "to-fill" (blank value the user must set),
    /// or "note" (advisory).
    pub severity: &'static str,
    pub file: String,
    pub line: Option<usize>,
    pub message: String,
}

/// Parse a simple `key: value` YAML line. Returns (indent, key, value) or None
/// for blank lines, comments, and list items. Naive on purpose — enough to
/// enumerate top-level/nested scalar keys and spot blank values.
pub(crate) fn parse_yaml_line(line: &str) -> Option<(usize, String, String)> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('-') {
        return None;
    }
    let indent = line.len() - trimmed.len();
    let colon = trimmed.find(':')?;
    let key = trimmed[..colon].trim();
    if key.is_empty()
        || !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return None;
    }
    let mut val = trimmed[colon + 1..].trim().to_string();
    // Strip a trailing " # comment" (naive; good enough for scalars).
    if let Some(pos) = val.find(" #") {
        val = val[..pos].trim().to_string();
    }
    Some((indent, key.to_string(), val))
}

/// Extract config keys referenced as `config["X"]` / `config.get("X"` on a
/// single line (top-level key only — nested `["a"]["b"]` yields "a").
pub(crate) fn scan_config_refs(line: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for pat in ["config[", "config.get("] {
        let mut start = 0usize;
        while let Some(idx) = line[start..].find(pat) {
            let after = start + idx + pat.len();
            let rest = &line[after..];
            if let Some(qpos) = rest.find(['"', '\'']) {
                let q = rest.as_bytes()[qpos] as char;
                let key_start = qpos + 1;
                if let Some(qend) = rest[key_start..].find(q) {
                    let key = &rest[key_start..key_start + qend];
                    if !key.is_empty() {
                        keys.push(key.to_string());
                    }
                }
            }
            start = after;
        }
    }
    keys
}

/// Detect a hardcoded absolute host path prefix at a real path boundary.
/// Only flags user/home roots that should never appear in a containerized
/// pipeline (mount points /input, /output, /pipeline are the correct forms).
pub(crate) fn hardcoded_host_prefix(line: &str) -> Option<&'static str> {
    const PREFIXES: &[&str] = &["/home/", "/Users/", "/root/"];
    for p in PREFIXES {
        if let Some(pos) = line.find(p) {
            let ok_before = pos == 0
                || line[..pos]
                    .chars()
                    .last()
                    .map(|c| !(c.is_ascii_alphanumeric() || c == '/'))
                    .unwrap_or(true);
            if ok_before {
                return Some(p);
            }
        }
    }
    None
}

/// Run all deterministic checks over the pipeline's (relpath, content) files.
pub(crate) fn review_checks(files: &[(String, String, bool)]) -> Vec<ReviewFinding> {
    let mut findings = Vec::new();
    let get = |name: &str| {
        files
            .iter()
            .find(|(r, _, _)| r.eq_ignore_ascii_case(name))
            .map(|(_, c, _)| c.as_str())
    };

    // 1. Required files present.
    for req in [
        "Snakefile",
        "Dockerfile",
        "config.yaml",
        "ro-crate-metadata.json",
        "README.md",
    ] {
        if !files.iter().any(|(r, _, _)| r.eq_ignore_ascii_case(req)) {
            findings.push(ReviewFinding {
                severity: "issue",
                file: req.to_string(),
                line: None,
                message: format!("Required file '{}' is missing.", req),
            });
        }
    }

    // 2. Snakefile has a target rule.
    if let Some(snake) = get("Snakefile") {
        if !snake.contains("rule all") {
            findings.push(ReviewFinding {
                severity: "issue",
                file: "Snakefile".into(),
                line: None,
                message: "No `rule all` found — Snakemake needs a target rule defining final outputs.".into(),
            });
        }
    }

    // 3. config.yaml: collect defined keys + flag blank scalar values.
    let mut defined: HashSet<String> = HashSet::new();
    if let Some(cfg) = get("config.yaml") {
        let lines: Vec<&str> = cfg.lines().collect();
        for (i, l) in lines.iter().enumerate() {
            if let Some((indent, key, val)) = parse_yaml_line(l) {
                defined.insert(key.clone());
                let is_blank = matches!(val.as_str(), "" | "\"\"" | "''" | "null" | "~");
                if is_blank {
                    // A blank value with more-indented children is a parent
                    // mapping, not a missing scalar — skip those.
                    let mut is_parent = false;
                    for nxt in lines.iter().skip(i + 1) {
                        let t = nxt.trim();
                        if t.is_empty() || t.starts_with('#') {
                            continue;
                        }
                        let ni = nxt.len() - nxt.trim_start().len();
                        if ni > indent || t.starts_with('-') {
                            is_parent = true;
                        }
                        break;
                    }
                    if !is_parent {
                        findings.push(ReviewFinding {
                            severity: "to-fill",
                            file: "config.yaml".into(),
                            line: Some(i + 1),
                            message: format!(
                                "`{}` has no value — the user must fill this before running.",
                                key
                            ),
                        });
                    }
                }
            }
        }
    }

    // 4. Snakefile references a config key that config.yaml never defines.
    if let Some(snake) = get("Snakefile") {
        for (i, line) in snake.lines().enumerate() {
            for key in scan_config_refs(line) {
                if !defined.contains(&key) {
                    findings.push(ReviewFinding {
                        severity: "issue",
                        file: "Snakefile".into(),
                        line: Some(i + 1),
                        message: format!(
                            "References config[\"{}\"] but config.yaml has no `{}` key.",
                            key, key
                        ),
                    });
                }
            }
        }
    }

    // 5. Hardcoded absolute host paths in code files (advisory).
    for (rel, content, _) in files {
        let low = rel.to_lowercase();
        let is_code = low.ends_with("snakefile")
            || low.ends_with("dockerfile")
            || low.ends_with(".py")
            || low.ends_with(".sh")
            || low.ends_with(".r");
        if !is_code {
            continue;
        }
        for (i, line) in content.lines().enumerate() {
            if line.trim_start().starts_with('#') {
                continue;
            }
            if let Some(p) = hardcoded_host_prefix(line) {
                findings.push(ReviewFinding {
                    severity: "note",
                    file: rel.clone(),
                    line: Some(i + 1),
                    message: format!(
                        "Hardcoded host path starting `{}` — pipelines should use /input, /output, /pipeline mount points.",
                        p
                    ),
                });
            }
        }
    }

    findings
}

/// Instruction footer appended to `review_pipeline` output. Tells the calling
/// AI exactly how to turn the code dump + checks into a user-facing summary.
pub(crate) const REVIEW_INSTRUCTIONS: &str = "\n=== INSTRUCTIONS FOR AI ===\n\
Produce a natural-language PIPELINE SUMMARY REPORT for the user, in THEIR chat language, using exactly this skeleton:\n\
1. One-line summary — what the pipeline does.\n\
2. Inputs — data/files it needs (from config.yaml and rule inputs).\n\
3. Steps (in order) — for each rule: what tool runs, input -> output.\n\
4. Outputs — final products (rule all targets).\n\
5. Tools & versions — from the Dockerfile.\n\
6. Config values — each key and its current value; mark blank ones the user must fill.\n\
7. Checks — list EVERY deterministic finding above, plus any logic problems you find yourself in the code. For each item you MUST cite file:line and quote the offending snippet.\n\
Rules: describe ONLY what is present in the code above — do not invent rules, tools, or files. If something is unclear, or a file was truncated or omitted, say so explicitly instead of guessing. Every finding in section 7 must point to a real file:line.\n\
After presenting the report, ask the user to confirm or request changes BEFORE building or executing (e.g. 'Proceed to build, or should I fix anything?'). For a freshly GENERATED pipeline, always show this report and get confirmation before build_image. Re-run this review after any edit — never rely on a remembered earlier approval.";

/// Fetch every reviewable source/config file (see `is_code_review_target`)
/// from the pipeline's GitHub directory and assemble a single textual dump
/// for the calling AI to silently review. The dump ends with an
/// `INSTRUCTIONS FOR AI` block that tells the agent what to do next (call
/// again with `user_reviewed_warnings=true`, or surface findings to the
/// user first). The function does not write anything to disk and does
/// not perform the actual download.
pub(crate) async fn fetch_pipeline_code_dump(
    owner: &str,
    repo: &str,
    branch: &str,
    subpath: &str,
    token: Option<&str>,
) -> Result<String, String> {
    let client = reqwest::Client::new();

    // 1. List every blob in the repo at the given branch in a single call.
    let tree_url = format!(
        "https://api.github.com/repos/{}/{}/git/trees/{}?recursive=1",
        owner, repo, branch
    );
    let mut tree_req = client.get(&tree_url).header("User-Agent", "autopipe-desktop");
    if let Some(t) = token {
        tree_req = tree_req.header("Authorization", format!("Bearer {}", t));
    }
    let tree_resp = tree_req.send().await.map_err(|e| e.to_string())?;
    if !tree_resp.status().is_success() {
        return Err(format!("GitHub tree fetch failed: HTTP {}", tree_resp.status()));
    }
    let tree_body: serde_json::Value = tree_resp.json().await.map_err(|e| e.to_string())?;
    let tree = tree_body["tree"]
        .as_array()
        .ok_or_else(|| "GitHub tree response missing 'tree' array".to_string())?;

    // 2. Keep blobs that live under the pipeline's subpath and look like code.
    let normalized = subpath.trim_matches('/');
    let prefix = if normalized.is_empty() {
        String::new()
    } else {
        format!("{}/", normalized)
    };

    let mut targets: Vec<String> = Vec::new();
    for entry in tree {
        if entry["type"].as_str() != Some("blob") {
            continue;
        }
        let path = match entry["path"].as_str() {
            Some(p) => p.to_string(),
            None => continue,
        };
        if !prefix.is_empty() && !path.starts_with(&prefix) {
            continue;
        }
        if !is_code_review_target(&path) {
            continue;
        }
        targets.push(path);
    }
    targets.sort();

    if targets.is_empty() {
        return Err("No reviewable source files found in the pipeline directory.".to_string());
    }

    // 3. Fetch each blob's contents from raw.githubusercontent.com.
    let mut out = String::new();
    out.push_str(&format!(
        "Reviewing pipeline source ({} file{}) before download.\n\n",
        targets.len(),
        if targets.len() == 1 { "" } else { "s" }
    ));
    for path in &targets {
        let raw_url = format!(
            "https://raw.githubusercontent.com/{}/{}/{}/{}",
            owner, repo, branch, path
        );
        let mut req = client.get(&raw_url).header("User-Agent", "autopipe-desktop");
        if let Some(t) = token {
            req = req.header("Authorization", format!("Bearer {}", t));
        }
        let content = match req.send().await {
            Ok(r) if r.status().is_success() => r.text().await.unwrap_or_default(),
            Ok(r) => format!("<failed to fetch: HTTP {}>", r.status()),
            Err(e) => format!("<failed to fetch: {}>", e),
        };
        let display_path = path.strip_prefix(&prefix).unwrap_or(path);
        out.push_str(&format!("=== {} ===\n{}\n\n", display_path, content));
    }

    // 4. Instruction footer for the calling AI.
    out.push_str(
        "INSTRUCTIONS FOR AI: Silently review every file above for suspicious patterns — \
         overly permissive permissions (chmod 777, SUID/SGID), credentials or tokens \
         hard-coded in source, unexpected outbound network calls, obfuscated or base64-decoded \
         execution, mounting host paths beyond the workspace, --network host or --privileged \
         docker flags, executing remotely-fetched scripts without verification, deleting files \
         outside the pipeline workspace, etc. Then either:\n\
         - No concerns found → call download_pipeline again with user_reviewed_warnings=true \
         to perform the actual download.\n\
         - Concerns found → present each finding to the user IN THEIR LANGUAGE with (1) file \
         path, (2) line number, (3) the offending code snippet, (4) a plain-English explanation \
         of the risk. Then ask: 'Mark all of this code as safe and download anyway?' (or the \
         equivalent in the user's language). Only call download_pipeline again with \
         user_reviewed_warnings=true after the user explicitly confirms.\n\
         NEVER skip this review just because you remember a previous approval of the same \
         pipeline — repeat the review every time.",
    );

    Ok(out)
}

pub(crate) fn parse_github_url(url: &str) -> Option<(String, String, Option<String>, String)> {
    let url = url.trim().trim_end_matches('/');
    let url = url.strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))?;
    let parts: Vec<&str> = url.splitn(4, '/').collect();
    if parts.len() < 2 {
        return None;
    }
    let owner = parts[0].to_string();
    let repo = parts[1].to_string();
    // Extract branch and path from tree/{branch}/{path}
    if parts.len() >= 4 && parts[2] == "tree" {
        let rest = parts[3];
        if let Some(slash_pos) = rest.find('/') {
            let branch = rest[..slash_pos].to_string();
            let path = rest[slash_pos + 1..].to_string();
            Some((owner, repo, Some(branch), path))
        } else {
            Some((owner, repo, Some(rest.to_string()), String::new()))
        }
    } else {
        Some((owner, repo, None, String::new()))
    }
}

/// Fetch a single file from GitHub Contents API.
async fn fetch_github_file(client: &reqwest::Client, owner: &str, repo: &str, path: &str, token: Option<&str>) -> Option<String> {
    let encoded_path = path.split('/').map(|seg| seg.replace(" ", "%20")).collect::<Vec<_>>().join("/");
    let url = format!(
        "https://api.github.com/repos/{}/{}/contents/{}",
        owner, repo, encoded_path
    );
    let mut req = client
        .get(&url)
        .header("Accept", "application/vnd.github.raw")
        .header("User-Agent", "autopipe-desktop");
    if let Some(token) = token {
        req = req.header("Authorization", format!("Bearer {}", token));
    }
    let resp = req.send().await.ok()?;
    if resp.status().is_success() {
        resp.text().await.ok()
    } else {
        None
    }
}

// ── SSH helper methods ──────────────────────────────────────────────

impl AutoPipeServer {
    /// Reload config from disk on every call so that changes made in the
    /// desktop app are picked up immediately without restarting the MCP server.
    fn config(&self) -> AppConfig {
        let mut c = AppConfig::load();
        // Tab-based routing. Only AutoPipe-managed AWS provisioning uses separate
        // VM fields; on that path route SSH to the VM and leave the user's manual
        // SSH server config untouched on disk. The SSH tab — and manual GCP/Azure
        // cloud entry, where the user pastes host/user/key into the normal ssh
        // fields — keep using the manual config exactly as before.
        if c.connection_type == "cloud" && c.cloud_provider == "aws" {
            if c.aws_vm_host.trim().is_empty() {
                // AWS cloud mode but no VM provisioned yet: blank the host so any
                // SSH operation fails clearly rather than silently falling back to
                // a stale manual SSH host (the previous confusing behavior).
                c.ssh_host = String::new();
            } else {
                c.ssh_host = c.aws_vm_host.clone();
                c.ssh_port = 22;
                c.ssh_user = "ubuntu".to_string();
                c.ssh_auth = SshAuth::Key {
                    key_path: c.aws_vm_key_path.clone(),
                };
                c.repo_path = "/home/ubuntu/autopipe".to_string();
            }
        }
        c
    }


    async fn ssh_run(&self, cmd: &str) -> Result<(String, i32), String> {
        let config = self.config();
        let cmd = cmd.to_string();
        let (output, code) = tokio::task::spawn_blocking(move || ssh::ssh_exec(&config, &cmd))
            .await
            .map_err(|e| format!("Task error: {}", e))??;
        Ok((clean_content(&output), code))
    }

    async fn ssh_run_timed(&self, cmd: &str, timeout_secs: u64) -> Result<(String, i32), String> {
        let config = self.config();
        let cmd = cmd.to_string();
        tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            tokio::task::spawn_blocking(move || ssh::ssh_exec(&config, &cmd)),
        )
        .await
        .map_err(|_| format!("SSH command timed out after {}s", timeout_secs))?
        .map_err(|e| format!("Task error: {}", e))?
        .map(|(output, code)| (clean_content(&output), code))
    }

    async fn ssh_read_file(&self, path: &str) -> Result<String, String> {
        let cmd = format!("cat '{}'", shell_escape(path));
        match self.ssh_run(&cmd).await {
            Ok((output, 0)) => Ok(output),
            Ok((output, _)) => Err(format!("Failed to read: {}", output.trim())),
            Err(e) => Err(e),
        }
    }

    /// Download a remote file via SSH exec + base64 encoding (more reliable than SCP).
    /// Returns the file size in bytes on success.
    async fn ssh_download_base64(&self, remote_path: &str, local_path: &str) -> Result<usize, String> {
        use base64::Engine;
        let cmd = format!("base64 '{}'", shell_escape(remote_path));
        let (b64_output, code) = self.ssh_run(&cmd).await?;
        if code != 0 {
            return Err(format!("Remote base64 failed: {}", b64_output.trim()));
        }
        // Remove whitespace from base64 output (line breaks etc.)
        let clean: String = b64_output.chars().filter(|c| !c.is_whitespace()).collect();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&clean)
            .map_err(|e| format!("Base64 decode error: {}", e))?;
        let size = bytes.len();
        std::fs::write(local_path, &bytes)
            .map_err(|e| format!("Cannot write '{}': {}", local_path, e))?;
        Ok(size)
    }

    async fn ssh_write_file(&self, path: &str, content: &str) -> Result<(), String> {
        // Base64 so arbitrary content transfers without heredoc/shell injection.
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(content.as_bytes());
        let bytes = encoded.as_bytes();
        const CHUNK: usize = 16_384; // multiple of 4; leaves headroom for the wrapper

        // Write to a temp file, then atomically `mv` it into place. If any chunk
        // transfer fails we restart the WHOLE write (re-truncating the temp), so
        // a partial or failed transfer never leaves a corrupt / half-written
        // file — the real path only ever changes via the final atomic rename.
        // Retries are bounded with backoff so a genuinely-down connection fails
        // cleanly (with the original file untouched) instead of looping forever.
        let tmp = format!("{}.autopipe-tmp", path);
        const MAX_ATTEMPTS: usize = 3;
        let mut last_err = String::new();
        for attempt in 0..MAX_ATTEMPTS {
            match self.ssh_write_chunks(&tmp, bytes, CHUNK).await {
                Ok(()) => {
                    let mv = format!("mv -f '{}' '{}'", shell_escape(&tmp), shell_escape(path));
                    match self.ssh_run(&mv).await {
                        Ok((_, 0)) => return Ok(()),
                        Ok((out, _)) => last_err = format!("atomic move failed: {}", out.trim()),
                        Err(e) => last_err = e,
                    }
                }
                Err(e) => last_err = e,
            }
            if attempt + 1 < MAX_ATTEMPTS {
                tokio::time::sleep(std::time::Duration::from_millis(300u64 << attempt)).await;
            }
        }
        // Best-effort cleanup so a failed write leaves no stray temp file behind.
        let _ = self.ssh_run(&format!("rm -f '{}'", shell_escape(&tmp))).await;
        Err(format!("Write failed after {} attempts: {}", MAX_ATTEMPTS, last_err))
    }

    /// Stream already-base64 `bytes` to `path` in `chunk`-sized pieces: the
    /// first piece truncates the file, the rest append. base64 output length is
    /// always a multiple of 4 and we cut on a multiple-of-4 boundary, so each
    /// piece is a valid standalone base64 unit that `base64 -d` decodes to whole
    /// bytes — the concatenation reconstructs the content exactly. The whole call
    /// is safe to re-run: it always re-truncates on the first piece.
    async fn ssh_write_chunks(&self, path: &str, bytes: &[u8], chunk: usize) -> Result<(), String> {
        let mut i = 0usize;
        let mut first = true;
        loop {
            let end = std::cmp::min(i + chunk, bytes.len());
            let piece = std::str::from_utf8(&bytes[i..end])
                .map_err(|e| format!("internal base64 encoding error: {}", e))?;
            let redirect = if first { ">" } else { ">>" };
            let cmd = format!(
                "printf '%s' '{}' | base64 -d {} '{}'",
                shell_escape(piece), redirect, shell_escape(path)
            );
            match self.ssh_run(&cmd).await {
                Ok((_, 0)) => {}
                Ok((output, _)) => return Err(format!("chunk write failed: {}", output.trim())),
                Err(e) => return Err(e),
            }
            first = false;
            i = end;
            if i >= bytes.len() {
                break; // empty content still performs one truncating write
            }
        }
        Ok(())
    }

    /// Resolve the output directory for a run. Always uses {configured_output_dir}/{run_name}.
    fn resolve_output_dir(&self, run_name: &str) -> String {
        format!(
            "{}/{}",
            self.config().full_output_dir().trim_end_matches('/'),
            run_name
        )
    }

    /// Resolve Docker socket mount path (rootless → /run/user/$UID/docker.sock, fallback → /var/run/docker.sock).
    async fn resolve_docker_socket_mount(&self) -> String {
        let check_cmd = "test -S /run/user/$(id -u)/docker.sock && echo rootless || echo root";
        let socket_path = match self.ssh_run(check_cmd).await {
            Ok((output, 0)) if clean_content(&output).trim() == "rootless" => {
                match self.ssh_run("echo /run/user/$(id -u)/docker.sock").await {
                    Ok((path, 0)) => clean_content(&path).trim().to_string(),
                    _ => "/var/run/docker.sock".to_string(),
                }
            }
            _ => "/var/run/docker.sock".to_string(),
        };
        format!(" -v '{}:/var/run/docker.sock' -v /usr/bin/docker:/usr/bin/docker", socket_path)
    }

    /// Find symlink targets inside a directory and return the resolved target
    /// file paths.
    ///
    /// The directory itself may be a symlink (e.g. `inputs/aptaselect` ->
    /// `0328_test/.../aptaselect`). `find -type l` would match only that
    /// directory link and never descend into it, so we canonicalize `dir` with
    /// `readlink -f` first and run `find` on the real path.
    ///
    /// We return the individual target files (not their parent directories) so
    /// callers can bind-mount exactly the referenced files into the container
    /// and avoid exposing unrelated sibling files in the target directory.
    async fn resolve_symlink_targets(&self, dir: &str) -> Vec<String> {
        let cmd = format!(
            "real=$(readlink -f '{}' 2>/dev/null); find \"$real\" -maxdepth 3 -type l -exec readlink -f '{{}}' \\; 2>/dev/null | sort -u",
            shell_escape(dir)
        );
        match self.ssh_run(&cmd).await {
            Ok((output, 0)) => clean_content(&output)
                .trim()
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty() && l != dir && l.starts_with('/'))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Find symlink targets inside a directory and return extra Docker -v mounts.
    async fn resolve_symlink_mounts(&self, dir: &str) -> String {
        let targets = self.resolve_symlink_targets(dir).await;
        let mut mounts = String::new();
        for target_dir in &targets {
            mounts.push_str(&format!(" -v '{}:{}:ro'", shell_escape(target_dir), shell_escape(target_dir)));
        }
        mounts
    }




    async fn find_pipeline_dir(&self, image_name: &str) -> Option<String> {
        let pipeline_name = image_name.strip_prefix("autopipe-").unwrap_or(image_name);

        let pipelines_base = self.config().full_pipelines_dir();
        let candidate = format!(
            "{}/{}",
            pipelines_base.trim_end_matches('/'),
            pipeline_name
        );
        if let Ok((output, 0)) = self
            .ssh_run(&format!("test -d '{}' && echo 'exists'", shell_escape(&candidate)))
            .await
        {
            if output.trim().contains("exists") {
                return Some(candidate);
            }
        }

        let output_base = self.config().full_output_dir();
        let candidate = format!(
            "{}/{}/{}",
            output_base.trim_end_matches('/'),
            pipeline_name,
            pipeline_name
        );
        if let Ok((output, 0)) = self
            .ssh_run(&format!("test -d '{}' && echo 'exists'", shell_escape(&candidate)))
            .await
        {
            if output.trim().contains("exists") {
                return Some(candidate);
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

    // ── Workspace info ─────────────────────────────────────────

    #[tool(description = "Get the configured workspace paths on the remote SSH server. Call this first to understand where pipelines and outputs are stored.")]
    async fn get_workspace_info(&self) -> Result<CallToolResult, ErrorData> {
        let config = self.config();
        let upload_mode = if config.per_pipeline_repo {
            "per-pipeline repo (ask user for repo name when uploading)".to_string()
        } else {
            format!("single repo ({})", config.github_repo)
        };

        // Look up GitHub login so the AI can fill ro-crate `#author.name`
        // without asking the user. Best-effort: when no token is set or
        // GitHub is unreachable we report "(not connected)" — the AI then
        // proceeds with an empty author and validate_pipeline gates the
        // publish step until the user connects GitHub.
        let github_login = fetch_github_login(config.github_token.as_deref())
            .await
            .unwrap_or_else(|| "(not connected)".to_string());

        let info = format!(
            "Workspace Configuration:\n\
             - Base path (repo_path): {}\n\
             - Pipelines directory: {}\n\
             - Input directory: {}\n\
             - Output directory: {}\n\
             - SSH: {}@{}:{}\n\
             - GitHub: {}\n\
             - Upload mode: {}\n\n\
             When creating pipelines, save files under the Pipelines directory.\n\
             When preparing input data, use prepare_input to download from a URL or symlink a server path into the Input directory. Pass the returned path as input_dir to dry_run or execute_pipeline.\n\
             When executing pipelines, outputs are automatically stored under the Output directory.\n\
             To view result files, use list_files and read_file directly on the output path.\n\
             To remove input files, use remove_input.\n\
             When generating ro-crate-metadata.json, use the GitHub login above as the default `#author.name`. If GitHub is (not connected), leave the author empty for now — validate_pipeline will auto-fill it once the user connects GitHub from the AutoPipe app.",
            if config.repo_path.is_empty() {
                "(not set)"
            } else {
                &config.repo_path
            },
            config.full_pipelines_dir(),
            config.full_input_dir(),
            config.full_output_dir(),
            config.ssh_user,
            config.ssh_host,
            config.ssh_port,
            github_login,
            upload_mode,
        );
        Ok(CallToolResult::success(vec![Content::text(info)]))
    }

    // ── Pipeline registry tools ─────────────────────────────────

    #[tool(description = "Search pipelines by keyword in the registry")]
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

    #[tool(description = "Download a pipeline by ID from the registry to the remote SSH server. Fetches pipeline files from its GitHub repository. If output_dir is omitted, saves to the configured pipelines directory.\n\n\
                          MANDATORY — SECURITY REVIEW BEFORE DOWNLOAD (two-call protocol):\n\
                          First call (without user_reviewed_warnings=true): this tool fetches every code/config file from the pipeline's GitHub repository (Snakefile, Dockerfile, *.py, *.R, *.sh, *.yaml, *.json, *.md, scripts/*, etc.) and returns them inline as a code dump. It does NOT save anything to the remote server. You MUST silently review every file in the dump for suspicious patterns — overly permissive permissions (chmod 777, SUID/SGID), credentials or tokens hard-coded in source, unexpected outbound network calls, obfuscated or base64-decoded execution, mounting host paths beyond the workspace, --network host or --privileged docker flags, executing remotely fetched scripts without verification, deleting files outside the pipeline workspace, etc. Use your judgement — the goal is to find anything that could harm the user if they run this pipeline.\n\
                          Second call (with user_reviewed_warnings=true): performs the actual download to the SSH server. Only call with this flag set when EITHER (a) you found no concerns in the review, OR (b) the user has explicitly confirmed they want to download despite the findings you presented. NEVER skip the review by calling with user_reviewed_warnings=true straight away.\n\
                          If you find concerns, present them to the user IN THEIR LANGUAGE: for each finding show (1) the file path and line number, (2) the offending code snippet, (3) a plain-English explanation of the risk (translated to the user's chat language). Then ask: 'Mark all of this code as safe and download anyway?' (or the equivalent in the user's language). Wait for an explicit yes/no per pipeline.\n\
                          This review is REQUIRED EVERY TIME, even for the same pipeline downloaded again later — never skip it just because you remember a previous approval.\n\n\
                          FALLBACK ON CLONE FAILURE:\n\
                          If the second call returns a clone-failure error while the SSH connection is still healthy, you may recover by writing each file individually with write_file, using the code dump returned by the first call. This avoids requiring the user to install or authenticate any additional tooling on the server. Use this fallback before asking the user to log in to GitHub, unless the error message explicitly indicates a private repository.\n\n\
                          AFTER a successful download: read the pipeline's config.yaml and briefly list each variable in one short line (its current value plus a few words on what it does — no long explanations; group related ones so it is easy to scan). Flag the values the user must set for their own data (anything blank, an input or reference path, or an example placeholder) and warn that running as-is would fail or use the example data. Then wait: if the user asks to change values, edit config.yaml exactly as they say; otherwise leave it as-is.")]
    async fn download_pipeline(
        &self,
        Parameters(params): Parameters<DownloadParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // 1. Get pipeline metadata from registry.
        let pipeline = match self.registry.get_pipeline(params.pipeline_id).await {
            Ok(p) => p,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Failed to get pipeline: {}",
                    e
                ))]));
            }
        };

        // 2. Parse GitHub URL once — used by both the review dump path and
        //    the actual download path below.
        let (owner, repo, branch_opt, path) = match parse_github_url(&pipeline.github_url) {
            Some(parsed) => parsed,
            None => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid GitHub URL: {}",
                    pipeline.github_url
                ))]));
            }
        };
        let branch = branch_opt.as_deref().unwrap_or("main");

        // 3. First call (user_reviewed_warnings != true): fetch source code
        //    for the AI to review and return it inline. Do NOT download.
        //    Prefer pipeline.git_tag (publish-time anchor) over the parsed
        //    URL branch so the review sees the same commit that the actual
        //    download will fetch.
        if !params.user_reviewed_warnings.unwrap_or(false) {
            let token = self.config().github_token.clone();
            let review_ref = pipeline.git_tag.as_deref().unwrap_or(branch);
            return match fetch_pipeline_code_dump(
                &owner,
                &repo,
                review_ref,
                &path,
                token.as_deref(),
            )
            .await
            {
                Ok(dump) => Ok(CallToolResult::success(vec![Content::text(dump)])),
                Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                    "Failed to fetch pipeline source for review: {}",
                    e
                ))])),
            };
        }

        // 3. Create directory on remote server
        // Convert any Windows-style path the user may have typed (e.g.
        // C:\Users\me\pipelines) into the WSL form (/mnt/c/Users/me/pipelines).
        let base_dir = params
            .output_dir
            .map(|p| windows_to_wsl(&p))
            .unwrap_or_else(|| self.config().full_pipelines_dir());
        let dir = format!(
            "{}/{}",
            base_dir.trim_end_matches('/'),
            pipeline.name
        );

        match self.ssh_run(&format!("mkdir -p '{}'", shell_escape(&dir))).await {
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

        // 4. Clone repo: try anonymous `git clone` first (covers all public
        //    repos without any GitHub authentication on the SSH server),
        //    then fall back to `gh repo clone`. With our token: authenticated
        //    (covers private repos). Without our token: anonymous gh, which
        //    is left as a last-resort fallback for servers where the user
        //    has independently configured `gh` (via `gh auth login` on the
        //    server itself or via a GH_TOKEN env var in their shell).
        let config = self.config();
        let github_token: Option<String> = config.github_token
            .as_ref()
            .filter(|t| !t.is_empty())
            .cloned();

        let tmp_dir = format!("/tmp/autopipe-clone-{}", std::process::id());

        // Anchor the clone to the publish-time tag when the registry has one
        // recorded; this preserves the exact published source even after the
        // default branch has been updated. Legacy rows (git_tag = None) fall
        // back to the default-branch HEAD, matching the previous behavior.
        let branch_flag = pipeline.git_tag.as_deref()
            .map(|t| format!("--branch '{}' ", t.replace('\'', "'\\''")))
            .unwrap_or_default();

        let git_clone = self.ssh_run(&format!(
            "git clone --depth 1 {}https://github.com/{}/{}.git '{}'",
            branch_flag, owner, repo, tmp_dir
        )).await;

        let (clone_output, clone_code) = match git_clone {
            Ok((out, 0)) => (out, 0),
            Ok((out, code)) => {
                // Clean partial clone before retrying.
                let _ = self.ssh_run(&format!("rm -rf '{}'", shell_escape(&tmp_dir))).await;

                // gh repo clone supports `--` pass-through for git args; we
                // include the same --branch flag so the fallback path also
                // pins to the publish-time tag when available.
                let pin_flag = pipeline.git_tag.as_deref()
                    .map(|t| format!(" -- --branch '{}'", t.replace('\'', "'\\''")))
                    .unwrap_or_default();
                let gh_cmd = match &github_token {
                    Some(token) => format!(
                        "GH_TOKEN='{}' gh repo clone {}/{} '{}'{}",
                        token, owner, repo, tmp_dir, pin_flag
                    ),
                    None => format!(
                        "gh repo clone {}/{} '{}'{}",
                        owner, repo, tmp_dir, pin_flag
                    ),
                };

                match self.ssh_run(&gh_cmd).await {
                    Ok((out2, 0)) => (out2, 0),
                    Ok((out2, code2)) => {
                        let _ = self.ssh_run(&format!("rm -rf '{}'", shell_escape(&tmp_dir))).await;
                        // Prefer the original git-clone output when gh also
                        // fails — git's error messages are usually more
                        // diagnostic than gh's "please authenticate" stub.
                        let combined = if out2.trim().is_empty() { out } else { out2 };
                        (combined, code2.max(code))
                    }
                    Err(e) => {
                        return Ok(CallToolResult::error(vec![Content::text(format!(
                            "SSH error during gh clone fallback: {}", e
                        ))]));
                    }
                }
            }
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "SSH error during git clone: {}", e
                ))]));
            }
        };

        if clone_code != 0 {
            let msg = if clone_output.contains("could not read Username")
                      || clone_output.contains("Authentication failed")
                      || clone_output.contains("terminal prompts disabled") {
                "Repository may be private. Connect GitHub in the AutoPipe app's GitHub panel and retry."
            } else if clone_output.contains("Repository not found")
                      || clone_output.contains("not found")
                      || clone_output.contains("does not exist") {
                "Repository not found on GitHub. Verify the pipeline ID."
            } else if clone_output.contains("Could not resolve host") {
                "Cannot reach github.com from the SSH server. Check network connectivity."
            } else if clone_output.contains("command not found") {
                "git is not installed on the SSH server. Install git and retry."
            } else {
                clone_output.as_str()
            };
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to clone repository: {}", msg
            ))]));
        }

        // 5. Extract subpath if needed, then move to target dir
        let source = if path.is_empty() {
            tmp_dir.clone()
        } else {
            format!("{}/{}", tmp_dir, path)
        };

        // Move files to target directory
        let move_result = self.ssh_run(&format!(
            "cp -r '{}'/* '{}' 2>/dev/null; cp -r '{}'/.* '{}' 2>/dev/null; rm -rf '{}'",
            shell_escape(&source), shell_escape(&dir),
            shell_escape(&source), shell_escape(&dir),
            shell_escape(&tmp_dir)
        )).await;

        if let Err(e) = move_result {
            let _ = self.ssh_run(&format!("rm -rf '{}'", shell_escape(&tmp_dir))).await;
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to move files: {}", e
            ))]));
        }

        // 6. List downloaded files
        let file_list = match self.ssh_run(&format!(
            "find '{}' -type f -not -path '*/.git/*' | sed 's|{}/||'",
            shell_escape(&dir), shell_escape(&dir)
        )).await {
            Ok((output, 0)) => output.trim().to_string(),
            _ => "(unable to list files)".to_string(),
        };

        // Clean up .git directory
        let _ = self.ssh_run(&format!("rm -rf '{}/.git'", shell_escape(&dir))).await;

        // Inject isBasedOn into ro-crate-metadata.json so that a later
        // publish_pipeline can automatically set forked_from = source pipeline_id.
        // This records the lineage to the source Hub pipeline. Users who want
        // to break the lineage can manually remove the isBasedOn field from
        // ro-crate-metadata.json before publishing.
        //
        // Implemented in Rust + serde_json so we don't depend on python3, jq,
        // or any other tool being installed on the user's host (e.g. macOS
        // ships without python3 by default).
        let meta_path = format!("{}/ro-crate-metadata.json", dir);
        let hub_url = format!(
            "{}/pipelines/{}",
            self.config().registry_url.trim_end_matches('/'),
            params.pipeline_id
        );
        if let Ok(meta_raw) = self.ssh_read_file(&meta_path).await {
            let cleaned = clean_content(&meta_raw);
            if let Ok(mut data) = serde_json::from_str::<serde_json::Value>(&cleaned) {
                if let Some(graph) = data.get_mut("@graph").and_then(|g| g.as_array_mut()) {
                    for node in graph.iter_mut() {
                        if node.get("@id").and_then(|v| v.as_str()) == Some("./") {
                            node["isBasedOn"] = serde_json::json!({ "@id": hub_url });
                            break;
                        }
                    }
                }
                if let Ok(updated) = serde_json::to_string_pretty(&data) {
                    if let Err(e) = self.ssh_write_file(&meta_path, &updated).await {
                        tracing::warn!(path = %meta_path, "failed to write metadata: {}", e);
                    }
                }
            }
        }

        let file_count = file_list.lines().count();
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Downloaded pipeline '{}' to {} (remote server)\nFiles ({}):\n{}\n\n\
             NEXT — offer a summary (optional): ask the user, in their language, whether they want a detailed summary of what this pipeline does and how it is structured. \
             If yes, call review_pipeline with pipeline_dir='{}' and present the summary per that tool's instructions. \
             If no, continue without it. (The mandatory security review already ran before download; this summary is a separate, optional correctness/understanding aid.)",
            pipeline.name, dir, file_count, file_list, dir
        ))]))
    }

    #[tool(description = "Upload a pipeline to GitHub. This only pushes code — versioning and tagging happen during publish_pipeline. After this tool succeeds, you MUST call publish_pipeline with the returned github_url to publish to the registry — unless the user explicitly said 'upload to GitHub only'. IMPORTANT: You MUST provide a complete list of ALL files needed to run the pipeline in the 'files' parameter. Include every file you created: Snakefile, Dockerfile, config.yaml, ro-crate-metadata.json, README.md, and any additional files such as scripts/*.py, requirements.txt, .dockerignore, etc. Do NOT omit any file — if the pipeline needs it to run, it must be in the list. REQUIRES GITHUB: If this tool returns a GitHub-login error, tell the user to open the AutoPipe app, connect GitHub from the GitHub panel, and try again. No restart is needed. REPO MODE: Call get_workspace_info first to see the upload mode. If 'single repo' mode, do NOT ask for a repo name — files go to the configured repository under pipelines/ subdirectory. If 'per-pipeline repo' mode, ask the user for a repository name and pass it as repo_name. CRITICAL — PIPELINE NAME RULE: The pipeline name is read from `ro-crate-metadata.json` -> `@graph[@id=='./']` -> `name` field. The GitHub directory path is `pipelines/<that name>/`, and the registry will register under that exact name. If the user asks to publish under a DIFFERENT name from the existing pipeline (e.g., 'publish this as test instead of aptaselect'), you MUST: (1) edit `ro-crate-metadata.json` in pipeline_dir to set `name` to the new value BEFORE calling this tool, (2) verify by reading the file back, (3) only then call upload_pipeline. Failing to update ro-crate FIRST will cause the registry to register under the old name, creating a duplicate version of the wrong pipeline. NEVER trust the in-memory pipeline name from a previous load_pipeline / download_pipeline call — always read the ro-crate file fresh from pipeline_dir. DIRECTORY CONFLICT: If the upload target already contains pipeline files, this tool returns (as a normal success result) guidance describing a conflict — this is NOT a completed upload; treat it as a HARD STOP and do not act automatically. You MUST tell the user (in their language) that a repository/location with this name already contains pipeline files and that uploading on top of them can mix the two and produce an INCOMPLETE or BROKEN pipeline, then ask them to choose: change the repository name (per-pipeline mode) or the pipeline name (single-repo mode), OR upload as-is. You MUST NOT set confirm_overwrite=true on your own or in the same turn — only after the user explicitly chooses 'upload as-is' may you re-call with confirm_overwrite=true. Do NOT mention the Hub or its registry status to the user; it is not relevant to them.")]
    async fn upload_pipeline(
        &self,
        Parameters(params): Parameters<UploadPipelineParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let config = self.config();
        let token = match &config.github_token {
            Some(t) if !t.is_empty() => t.clone(),
            _ => {
                return Ok(CallToolResult::error(vec![Content::text(
                    "GitHub login is required. Please open the AutoPipe app, connect GitHub from the GitHub panel, and try again.",
                )]));
            }
        };

        // Translate Windows paths (e.g. C:\Users\me\pipeline) to WSL form so
        // the SSH side can find the directory on a WSL backend.
        let pipeline_dir_translated = windows_to_wsl(&params.pipeline_dir);
        let dir = &pipeline_dir_translated;

        // Ensure ro-crate-metadata.json is always included
        let mut files = params.files;
        let meta_filename = "ro-crate-metadata.json".to_string();
        if !files.iter().any(|f| f == &meta_filename) {
            files.push(meta_filename);
        }

        // Read ro-crate-metadata.json first (required for pipeline name)
        let meta_raw = match self.ssh_read_file(&format!("{}/ro-crate-metadata.json", dir)).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Cannot read ro-crate-metadata.json: {}", e
                ))]));
            }
        };
        let cleaned_meta = clean_content(&meta_raw);
        let metadata: PipelineMetadata = match parse_ro_crate_metadata(&cleaned_meta) {
            Ok(m) => m,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid ro-crate-metadata.json: {}", e
                ))]));
            }
        };

        let meta_json: serde_json::Value = serde_json::from_str(&cleaned_meta).unwrap_or_default();
        let metadata_json_str = serde_json::to_string_pretty(&meta_json).unwrap_or_default();

        let pipeline_name = &metadata.name;
        let (repo_name, path_prefix) = if config.per_pipeline_repo {
            match &params.repo_name {
                Some(name) => (name.clone(), String::new()),
                None => {
                    return Ok(CallToolResult::error(vec![Content::text(
                        "Per-pipeline repo mode is enabled but no repo_name provided. Ask the user for a repository name."
                    )]));
                }
            }
        } else {
            (config.github_repo.clone(), format!("pipelines/{}/", pipeline_name))
        };

        let client = reqwest::Client::new();

        // 1. Get GitHub username
        let user_resp = client
            .get("https://api.github.com/user")
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "autopipe-desktop")
            .send()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let user: serde_json::Value = user_resp.json().await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let owner = user["login"].as_str().unwrap_or_default().to_string();

        // 2. Ensure repo exists
        let repo_check = client
            .get(format!("https://api.github.com/repos/{}/{}", owner, repo_name))
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "autopipe-desktop")
            .send()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let mut repo_just_created = false;
        if repo_check.status() == reqwest::StatusCode::NOT_FOUND {
            let create_resp = client
                .post("https://api.github.com/user/repos")
                .header("Authorization", format!("Bearer {}", token))
                .header("User-Agent", "autopipe-desktop")
                .json(&serde_json::json!({
                    "name": repo_name,
                    "description": "AutoPipe pipelines",
                    "auto_init": true
                }))
                .send()
                .await
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

            if !create_resp.status().is_success() {
                let err_text = create_resp.text().await.unwrap_or_default();
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Failed to create GitHub repo: {}", err_text
                ))]));
            }
            repo_just_created = true;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }

        // ── Directory-conflict check ────────────────────────────────────
        // If the target subdirectory (single-repo mode) or the repo root
        // (per-pipeline mode) already holds an EXISTING pipeline but the Hub
        // has no record of a pipeline by this name and author, the existing
        // files might belong to a different pipeline (e.g. one that was
        // unpublished). Rather than silently inheriting those residual files
        // via base_tree, we return guidance asking the user to confirm before
        // proceeding (see the confirm_overwrite handling below).
        //
        // We detect an existing pipeline by the presence of a pipeline-defining
        // marker file (Snakefile or ro-crate-metadata.json) at the target
        // location — NOT by "any file". This is important because a freshly
        // created repo (auto_init) contains only a README.md, which must not
        // be mistaken for a residual pipeline. We also skip the check entirely
        // when this upload just created the repo.
        let registry_url = config.registry_url.trim_end_matches('/').to_string();
        let dir_prefix = path_prefix.trim_end_matches('/').to_string();
        let tree_resp = if repo_just_created {
            None
        } else {
            client
                .get(format!(
                    "https://api.github.com/repos/{}/{}/git/trees/main?recursive=1",
                    owner, repo_name
                ))
                .header("Authorization", format!("Bearer {}", token))
                .header("User-Agent", "autopipe-desktop")
                .send()
                .await
                .ok()
        };
        if let Some(r) = tree_resp {
            if r.status().is_success() {
                if let Ok(tree_body) = r.json::<serde_json::Value>().await {
                    let entries = tree_body["tree"].as_array().cloned().unwrap_or_default();
                    // An existing pipeline is identified by a marker file at
                    // the target location.
                    let has_marker = |fname: &str| -> bool {
                        let target = if dir_prefix.is_empty() {
                            fname.to_string()
                        } else {
                            format!("{}/{}", dir_prefix, fname)
                        };
                        entries.iter().any(|e| {
                            e["type"].as_str() == Some("blob")
                                && e["path"].as_str() == Some(target.as_str())
                        })
                    };
                    let dir_exists = has_marker("Snakefile") || has_marker("ro-crate-metadata.json");

                    if dir_exists {
                        // Check the Hub for a matching name + author row. The
                        // existing /api/pipelines endpoint returns the latest
                        // version per name, which is enough: if any row with
                        // this name + this author exists, the upload is part
                        // of the legitimate version-upgrade flow.
                        let list_resp = client
                            .get(format!("{}/api/pipelines", registry_url))
                            .send()
                            .await;
                        let has_hub_record = match list_resp {
                            Ok(lr) if lr.status().is_success() => {
                                let body = lr
                                    .json::<serde_json::Value>()
                                    .await
                                    .unwrap_or_else(|_| serde_json::Value::Null);
                                let list = body.as_array().cloned().unwrap_or_default();
                                list.iter().any(|p| {
                                    p["name"].as_str() == Some(pipeline_name.as_str())
                                        && p["author"].as_str() == Some(owner.as_str())
                                })
                            }
                            _ => false,
                        };

                        // Conflict detected. Unless the user has already
                        // confirmed they want to overwrite, return detailed
                        // guidance (as a SUCCESS result, not an error) so the
                        // assistant explains the risk and asks the user, then
                        // re-calls with confirm_overwrite=true.
                        if !has_hub_record && params.confirm_overwrite != Some(true) {
                            // Mode-specific wording: per-pipeline mode lets the
                            // user pick a different repository name; single-repo
                            // mode keys off the pipeline name in ro-crate. The
                            // Hub registry status is intentionally NOT surfaced
                            // to the user — only the repo/file situation is.
                            let (thing, change_choice, change_how) = if dir_prefix.is_empty() {
                                (
                                    format!("A GitHub repository named `{}`", repo_name),
                                    "repository name",
                                    "ask the user for a new repository name and call upload_pipeline again with that repo_name",
                                )
                            } else {
                                (
                                    format!("The directory `{}/` in `{}/{}`", dir_prefix, owner, repo_name),
                                    "pipeline name",
                                    "change the `name` field in ro-crate-metadata.json (the dataset node with @id \"./\") to a unique name and retry",
                                )
                            };
                            return Ok(CallToolResult::success(vec![Content::text(format!(
                                "⚠️ ACTION REQUIRED — NOT UPLOADED YET. This is a HARD STOP: do NOT call upload_pipeline with confirm_overwrite=true on your own or in the same turn. You MUST ask the user first and act on their explicit choice.\n\n\
                                 {thing} already contains pipeline files. Tell the user, in their language, and ask them to choose — for example: \"A {choice} with this name already contains pipeline files. If you upload your pipeline on top of them, it can get mixed with the existing files and end up incomplete or broken. We recommend using a different {choice}. Would you like to change the {choice}, or upload here as-is?\"\n\n\
                                 - If the user wants to change it: {change_how} (do NOT set confirm_overwrite).\n\
                                 - If the user wants to upload as-is: call upload_pipeline again with confirm_overwrite=true.\n\n\
                                 Do NOT mention the Hub or its registry status to the user — it is not relevant to them.",
                                thing = thing, choice = change_choice, change_how = change_how
                            ))]));
                        }
                    }
                }
            }
        }

        // Read all specified files from SSH
        let mut file_contents: Vec<(String, String)> = Vec::new();
        for file_path in &files {
            let remote_path = format!("{}/{}", dir, file_path);
            let content = match self.ssh_read_file(&remote_path).await {
                Ok(c) => c,
                Err(e) => {
                    return Ok(CallToolResult::error(vec![Content::text(format!(
                        "Cannot read '{}': {}", file_path, e
                    ))]));
                }
            };
            let cleaned = if file_path == "ro-crate-metadata.json" {
                metadata_json_str.clone()
            } else if file_path == "Snakefile" || file_path.ends_with(".yaml") || file_path.ends_with(".yml") {
                normalize_paths(&clean_content(&content))
            } else {
                clean_content(&content)
            };
            file_contents.push((file_path.clone(), cleaned));
        }

        // 3. Get latest commit SHA on main branch (retry up to 5 times for newly created repos)
        let mut latest_sha = String::new();
        for attempt in 0..5 {
            let ref_resp = client
                .get(format!(
                    "https://api.github.com/repos/{}/{}/git/ref/heads/main",
                    owner, repo_name
                ))
                .header("Authorization", format!("Bearer {}", token))
                .header("User-Agent", "autopipe-desktop")
                .send()
                .await
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

            let ref_body: serde_json::Value = ref_resp.json().await
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
            latest_sha = ref_body["object"]["sha"].as_str().unwrap_or_default().to_string();

            if !latest_sha.is_empty() {
                break;
            }
            if attempt < 4 {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }

        if latest_sha.is_empty() {
            return Ok(CallToolResult::error(vec![Content::text(
                "Could not get latest commit SHA. The repository may not have a main branch. \
                 Please ensure the repository is initialized with at least one commit.",
            )]));
        }

        // 4. Get base tree SHA
        let commit_resp = client
            .get(format!(
                "https://api.github.com/repos/{}/{}/git/commits/{}",
                owner, repo_name, latest_sha
            ))
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "autopipe-desktop")
            .send()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let commit_body: serde_json::Value = commit_resp.json().await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let base_tree = commit_body["tree"]["sha"].as_str().unwrap_or_default().to_string();

        // 5. Create tree with pipeline files
        let tree_items: Vec<serde_json::Value> = file_contents
            .iter()
            .filter(|(_, content)| !content.is_empty())
            .map(|(name, content)| {
                serde_json::json!({
                    "path": format!("{}{}", path_prefix, name),
                    "mode": "100644",
                    "type": "blob",
                    "content": content
                })
            })
            .collect();

        let tree_resp = client
            .post(format!(
                "https://api.github.com/repos/{}/{}/git/trees",
                owner, repo_name
            ))
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "autopipe-desktop")
            .json(&serde_json::json!({
                "base_tree": base_tree,
                "tree": tree_items
            }))
            .send()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let tree_body: serde_json::Value = tree_resp.json().await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let new_tree_sha = tree_body["sha"].as_str().unwrap_or_default().to_string();

        // 6. Create commit
        let commit_msg = params.commit_message.unwrap_or_else(|| {
            format!("Upload {}", pipeline_name)
        });

        let new_commit_resp = client
            .post(format!(
                "https://api.github.com/repos/{}/{}/git/commits",
                owner, repo_name
            ))
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "autopipe-desktop")
            .json(&serde_json::json!({
                "message": commit_msg,
                "tree": new_tree_sha,
                "parents": [latest_sha]
            }))
            .send()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let new_commit_body: serde_json::Value = new_commit_resp.json().await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let new_commit_sha = new_commit_body["sha"].as_str().unwrap_or_default().to_string();

        // 7. Update ref to point to new commit
        let update_ref = client
            .patch(format!(
                "https://api.github.com/repos/{}/{}/git/refs/heads/main",
                owner, repo_name
            ))
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "autopipe-desktop")
            .json(&serde_json::json!({ "sha": new_commit_sha }))
            .send()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        if !update_ref.status().is_success() {
            let err = update_ref.text().await.unwrap_or_default();
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to update branch ref: {}", err
            ))]));
        }

        let github_url = if config.per_pipeline_repo {
            format!("https://github.com/{}/{}", owner, repo_name)
        } else {
            format!("https://github.com/{}/{}/tree/main/pipelines/{}",
                owner, repo_name, pipeline_name)
        };
        let commit_url = format!(
            "https://github.com/{}/{}/commit/{}",
            owner, repo_name, new_commit_sha
        );

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Successfully uploaded '{}' to GitHub!\n\
             Pipeline URL: {}\n\
             Commit: {}",
            pipeline_name,
            github_url,
            commit_url
        ))]))
    }

    #[tool(description = "Publish a pipeline from GitHub to the AutoPipe registry. PREREQUISITE: Call upload_pipeline FIRST. The registry reads the pipeline name from ro-crate-metadata.json on GitHub — NOT from any parameter. So the name in ro-crate-metadata.json (in the directory referenced by github_url) is what gets registered. Duplicate detection is handled automatically by this tool. Same name + same author: returns existing pipeline info — you MUST ask the user 'Would you like to register this as a new version of the existing pipeline?' before proceeding. If user agrees, call this tool again with forked_from set to the existing pipeline_id. Same name + different author: returns info for user to choose — either change the pipeline name or mark as 'Based on' by setting forked_from to the existing pipeline_id. CRITICAL — RENAME GUARD: If the user wanted a NEW name (e.g., 'publish as test' for a pipeline originally named aptaselect), the rename must already be reflected in BOTH (a) the GitHub directory path inside the github_url AND (b) the `name` field of ro-crate-metadata.json at that path. Before calling this tool, fetch the ro-crate at github_url and verify the name field matches what the user asked for. If the name does not match, STOP and re-run upload_pipeline with the corrected ro-crate first. Calling publish_pipeline with a stale ro-crate will register under the WRONG name and create a duplicate version of the original pipeline. MANDATORY — LINEAGE CONFIRMATION: BEFORE calling this tool, you MUST fetch ro-crate-metadata.json at the github_url and check whether the dataset node (@id == './') contains an `isBasedOn` field. If it does AND the URL points to this AutoPipe Hub (matches the configured registry_url + '/pipelines/<id>' pattern), you MUST first ask the user a confirmation question in their language. Example (in English; translate to the user's language at runtime): 'It looks like this pipeline was downloaded from <original name>(#<original id>) and modified. Is that correct? If yes, the source (forked_from) will be recorded automatically. If not, the lineage will be cleared and this will be registered as an independent pipeline.' Look up the original pipeline's name and id from the isBasedOn URL (the trailing /pipelines/<id> part) and the registry's get_pipeline endpoint. If the user confirms it IS a fork: call this tool normally without forked_from — auto-detection will populate it. If the user says it is NOT based on the original (independent pipeline): call this tool with forked_from=null AND instruct the user to remove the isBasedOn field from ro-crate-metadata.json so future publishes are clean. Skip this confirmation only when isBasedOn is absent or points to an external (non-Hub) URL.")]
    async fn publish_pipeline(
        &self,
        Parameters(params): Parameters<PublishPipelineParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let config = self.config();
        let token = match &config.github_token {
            Some(t) if !t.is_empty() => t.clone(),
            _ => {
                return Ok(CallToolResult::error(vec![Content::text(
                    "GitHub login is required. Please open the AutoPipe app, connect GitHub from the GitHub panel, and try again.",
                )]));
            }
        };

        let base = config.registry_url.trim_end_matches('/');
        let client = reqwest::Client::new();

        // Get GitHub username for author comparison
        let user_resp = client
            .get("https://api.github.com/user")
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "autopipe-desktop")
            .send()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let github_user: serde_json::Value = user_resp.json().await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let my_author = github_user["login"].as_str().unwrap_or_default().to_string();

        // Parse github_url to get owner/repo
        let is_single_repo = params.github_url.contains("/pipelines/");

        let (gh_owner, gh_repo, _, subpath) = match parse_github_url(&params.github_url) {
            Some(parsed) => parsed,
            None => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid GitHub URL: {}", params.github_url
                ))]));
            }
        };

        // Fetch pipeline name
        let pipeline_name = if is_single_repo {
            // Single repo: extract name from URL path (pipelines/{name})
            let parts: Vec<&str> = params.github_url.trim_end_matches('/').split('/').collect();
            let mut name = String::new();
            for (i, part) in parts.iter().enumerate() {
                if *part == "pipelines" {
                    if let Some(n) = parts.get(i + 1) {
                        name = n.to_string();
                    }
                    break;
                }
            }
            name
        } else {
            // Per-pipeline repo: read from ro-crate-metadata.json at root
            if let Some(meta_str) = fetch_github_file(&client, &gh_owner, &gh_repo, "ro-crate-metadata.json", Some(token.as_str())).await {
                let meta: serde_json::Value = serde_json::from_str(&meta_str).unwrap_or_default();
                if let Some(graph) = meta["@graph"].as_array() {
                    graph.iter()
                        .find(|n| n["@id"].as_str() == Some("./"))
                        .and_then(|ds| ds["name"].as_str())
                        .unwrap_or_default()
                        .to_string()
                } else {
                    meta["name"].as_str().unwrap_or_default().to_string()
                }
            } else {
                String::new()
            }
        };

        // If single repo and name extraction failed, fallback to metadata
        let pipeline_name = if pipeline_name.is_empty() {
            let meta_path = if is_single_repo {
                format!("{}/ro-crate-metadata.json", subpath)
            } else {
                "ro-crate-metadata.json".to_string()
            };
            if let Some(meta_str) = fetch_github_file(&client, &gh_owner, &gh_repo, &meta_path, Some(token.as_str())).await {
                let meta: serde_json::Value = serde_json::from_str(&meta_str).unwrap_or_default();
                if let Some(graph) = meta["@graph"].as_array() {
                    graph.iter()
                        .find(|n| n["@id"].as_str() == Some("./"))
                        .and_then(|ds| ds["name"].as_str())
                        .unwrap_or_default()
                        .to_string()
                } else {
                    meta["name"].as_str().unwrap_or_default().to_string()
                }
            } else {
                String::new()
            }
        } else {
            pipeline_name
        };

        // Auto-detect duplicate: search registry for same name
        let mut resolved_forked_from = params.forked_from;
        if resolved_forked_from.is_none() && !pipeline_name.is_empty() {
            if let Ok(search_resp) = client
                .get(format!("{}/api/pipelines", base))
                .query(&[("q", &pipeline_name)])
                .send()
                .await
            {
                if let Ok(results) = search_resp.json::<Vec<serde_json::Value>>().await {
                    let exact_match = results.iter().find(|p| {
                        p["name"].as_str().map(|n| n == pipeline_name).unwrap_or(false)
                    });

                    if let Some(existing) = exact_match {
                        let existing_author = existing["author"].as_str().unwrap_or_default();
                        let existing_id = existing["pipeline_id"].as_i64().unwrap_or(0);
                        let existing_version = existing["version"].as_str().unwrap_or("?");

                        if existing_author == my_author {
                            // Same name + same author → ask user before version upgrade
                            return Ok(CallToolResult::error(vec![Content::text(format!(
                                "Your pipeline '{}' v{} already exists in the registry (ID: {}).\n\
                                 Ask the user whether to publish as a version upgrade of the existing pipeline.\n\
                                 If yes: call publish_pipeline again with forked_from={}.\n\
                                 If no: the user should change the pipeline name and retry.",
                                pipeline_name, existing_version, existing_id, existing_id
                            ))]));
                        } else {
                            // Same name + different author → ask user
                            return Ok(CallToolResult::error(vec![Content::text(format!(
                                "A pipeline named '{}' already exists by '{}' (v{}, ID: {}).\n\
                                 Ask the user:\n\
                                 1. Change the pipeline name to avoid conflict\n\
                                 2. Mark as 'Based on' this pipeline (call publish_pipeline again with forked_from={})",
                                pipeline_name, existing_author, existing_version, existing_id, existing_id
                            ))]));
                        }
                    }
                }
            }
        }

        // Determine version from GitHub tags and update metadata
        // Fetch existing tags for this pipeline: refs/tags/{pipeline_name}/v*
        let tag_prefix = pipeline_name.replace(' ', "-");
        let tags_url = format!(
            "https://api.github.com/repos/{}/{}/git/matching-refs/tags/{}/v",
            gh_owner, gh_repo, tag_prefix
        );
        let tags_resp = client
            .get(&tags_url)
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "autopipe-desktop")
            .send()
            .await;

        // Also check registry for existing version (fallback when repo has no tags)
        let registry_version: Option<String> = if let Ok(search_resp) = client
            .get(format!("{}/api/pipelines", base))
            .query(&[("q", &pipeline_name)])
            .send()
            .await
        {
            search_resp.json::<Vec<serde_json::Value>>().await.ok()
                .and_then(|results| results.iter()
                    .find(|p| p["name"].as_str().map(|n| n == pipeline_name).unwrap_or(false))
                    .and_then(|p| p["version"].as_str())
                    .map(|v| v.to_string()))
        } else { None };

        let version = match tags_resp {
            Ok(resp) if resp.status().is_success() => {
                let tags: Vec<serde_json::Value> = resp.json().await.unwrap_or_default();
                if tags.is_empty() {
                    // No tags: check registry version
                    if let Some(ref reg_ver) = registry_version {
                        let parts: Vec<u32> = reg_ver.split('.').map(|p| p.parse().unwrap_or(0)).collect();
                        if parts.len() == 3 {
                            format!("{}.{}.{}", parts[0], parts[1], parts[2] + 1)
                        } else {
                            "1.0.0".to_string()
                        }
                    } else {
                        "1.0.0".to_string()
                    }
                } else {
                    // Find the latest version from tags
                    let mut latest = (1u32, 0u32, 0u32);
                    for tag in &tags {
                        if let Some(ref_str) = tag["ref"].as_str() {
                            // ref: "refs/tags/pipeline_name/v1.2.3"
                            if let Some(ver_str) = ref_str.rsplit("/v").next() {
                                let parts: Vec<u32> = ver_str.split('.')
                                    .map(|p| p.parse().unwrap_or(0))
                                    .collect();
                                if parts.len() == 3 {
                                    let v = (parts[0], parts[1], parts[2]);
                                    if v > latest { latest = v; }
                                }
                            }
                        }
                    }
                    format!("{}.{}.{}", latest.0, latest.1, latest.2 + 1)
                }
            }
            _ => "1.0.0".to_string(),
        };

        // Update version in GitHub metadata before publishing
        let meta_path = if is_single_repo {
            format!("pipelines/{}/ro-crate-metadata.json", pipeline_name)
        } else {
            "ro-crate-metadata.json".to_string()
        };
        if let Some(meta_str) = fetch_github_file(&client, &gh_owner, &gh_repo, &meta_path, Some(token.as_str())).await {
            let mut meta_json: serde_json::Value = serde_json::from_str(&meta_str).unwrap_or_default();

            // Update version in both top-level and RO-Crate @graph
            meta_json["version"] = serde_json::Value::String(version.clone());
            if let Some(graph) = meta_json["@graph"].as_array_mut() {
                if let Some(dataset) = graph.iter_mut().find(|n| n["@id"].as_str() == Some("./")) {
                    dataset["version"] = serde_json::Value::String(version.clone());
                }
            }
            let updated_meta = serde_json::to_string_pretty(&meta_json).unwrap_or_default();

            // Commit the version update to GitHub
            let ref_resp = client
                .get(format!(
                    "https://api.github.com/repos/{}/{}/git/ref/heads/main",
                    gh_owner, gh_repo
                ))
                .header("Authorization", format!("Bearer {}", token))
                .header("User-Agent", "autopipe-desktop")
                .send()
                .await;

            if let Ok(ref_r) = ref_resp {
                if let Ok(ref_body) = ref_r.json::<serde_json::Value>().await {
                    let latest_sha = ref_body["object"]["sha"].as_str().unwrap_or_default().to_string();
                    if !latest_sha.is_empty() {
                        // Get base tree
                        if let Ok(commit_r) = client.get(format!(
                            "https://api.github.com/repos/{}/{}/git/commits/{}",
                            gh_owner, gh_repo, latest_sha
                        )).header("Authorization", format!("Bearer {}", token))
                        .header("User-Agent", "autopipe-desktop")
                        .send().await {
                            if let Ok(commit_body) = commit_r.json::<serde_json::Value>().await {
                                let base_tree = commit_body["tree"]["sha"].as_str().unwrap_or_default();

                                // Create tree with updated metadata
                                let encoded_meta_path = meta_path.replace(" ", "%20");
                                let _ = encoded_meta_path; // suppress warning, we use meta_path directly in JSON
                                if let Ok(tree_r) = client.post(format!(
                                    "https://api.github.com/repos/{}/{}/git/trees",
                                    gh_owner, gh_repo
                                )).header("Authorization", format!("Bearer {}", token))
                                .header("User-Agent", "autopipe-desktop")
                                .json(&serde_json::json!({
                                    "base_tree": base_tree,
                                    "tree": [{
                                        "path": meta_path,
                                        "mode": "100644",
                                        "type": "blob",
                                        "content": updated_meta
                                    }]
                                })).send().await {
                                    if let Ok(tree_body) = tree_r.json::<serde_json::Value>().await {
                                        let new_tree = tree_body["sha"].as_str().unwrap_or_default();

                                        // Create commit
                                        if let Ok(commit_r2) = client.post(format!(
                                            "https://api.github.com/repos/{}/{}/git/commits",
                                            gh_owner, gh_repo
                                        )).header("Authorization", format!("Bearer {}", token))
                                        .header("User-Agent", "autopipe-desktop")
                                        .json(&serde_json::json!({
                                            "message": format!("Publish {} v{}", pipeline_name, version),
                                            "tree": new_tree,
                                            "parents": [latest_sha]
                                        })).send().await {
                                            if let Ok(commit_body2) = commit_r2.json::<serde_json::Value>().await {
                                                let new_sha = commit_body2["sha"].as_str().unwrap_or_default().to_string();

                                                // Update branch ref
                                                let _ = client.patch(format!(
                                                    "https://api.github.com/repos/{}/{}/git/refs/heads/main",
                                                    gh_owner, gh_repo
                                                )).header("Authorization", format!("Bearer {}", token))
                                                .header("User-Agent", "autopipe-desktop")
                                                .json(&serde_json::json!({ "sha": new_sha }))
                                                .send().await;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Resolve the main HEAD SHA up front so the registry row and the
        // tag we create afterwards both reference the exact same commit.
        // If this lookup fails we still proceed with the publish (without
        // git_tag/commit_sha) so old behavior is preserved as a fallback;
        // the row can be populated later by the backfill script.
        let tag_name = format!("{}/v{}", pipeline_name.replace(' ', "-"), version);
        let main_sha: Option<String> = match client.get(format!(
            "https://api.github.com/repos/{}/{}/git/ref/heads/main",
            gh_owner, gh_repo
        ))
        .header("Authorization", format!("Bearer {}", token))
        .header("User-Agent", "autopipe-desktop")
        .send().await {
            Ok(r) => r.json::<serde_json::Value>().await.ok()
                .and_then(|j| j["object"]["sha"].as_str().map(|s| s.to_string())),
            Err(_) => None,
        };

        // Call registry publish endpoint. Include git_tag and commit_sha
        // only when we successfully resolved the main HEAD SHA — otherwise
        // the tag we promise to create below would not exist.
        let mut publish_body = serde_json::json!({
            "github_url": params.github_url,
            "github_token": token,
            "forked_from": resolved_forked_from,
        });
        if let Some(ref sha) = main_sha {
            publish_body["git_tag"] = serde_json::Value::String(tag_name.clone());
            publish_body["commit_sha"] = serde_json::Value::String(sha.clone());
        }

        let resp = client
            .post(format!("{}/api/publish", base))
            .json(&publish_body)
            .send()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let status = resp.status();
        let body: serde_json::Value = resp.json().await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        if status.is_success() {
            // Create version tag at the SAME SHA we recorded in the registry.
            // Best-effort: registry insert is the source of truth, and the
            // backfill script can re-create the tag later if this step is
            // skipped or fails.
            if let Some(sha) = main_sha.as_deref() {
                let _ = client.post(format!(
                    "https://api.github.com/repos/{}/{}/git/refs",
                    gh_owner, gh_repo
                )).header("Authorization", format!("Bearer {}", token))
                .header("User-Agent", "autopipe-desktop")
                .json(&serde_json::json!({
                    "ref": format!("refs/tags/{}", tag_name),
                    "sha": sha
                })).send().await;
            }

            let pipeline_id = body["pipeline_id"].as_i64().unwrap_or(0);
            let name = body["name"].as_str().unwrap_or("unknown");
            let web_url = format!("{}/pipelines/{}", base, pipeline_id);
            Ok(CallToolResult::success(vec![Content::text(format!(
                "Successfully published '{}' v{} to the registry!\n\
                 Web page: {}\n\
                 Pipeline ID: {}",
                name, version, web_url, pipeline_id
            ))]))
        } else {
            let error_msg = body["error"].as_str().unwrap_or("Unknown error");
            Ok(CallToolResult::error(vec![Content::text(format!(
                "Publish failed: {}", error_msg
            ))]))
        }
    }

    #[tool(description = "Remove a pipeline that you previously published to Autopipe Hub. \
This deletes (1) the Hub registry record(s), and (2) the corresponding GitHub git tag(s) of the form `refs/tags/<pipeline-name>/v<version>` so the same version can be cleanly republished later. \
GITHUB SOURCE FILES ARE NOT DELETED BY THIS TOOL. The `pipelines/<name>/` directory and its files remain in your GitHub repository. If the user asks you to also delete the pipeline source code or files from GitHub, you MUST refuse and tell the user: this tool can only remove the pipeline from the Autopipe Hub and clean up the version tag. To delete the actual source files, the user must do it themselves on GitHub — by deleting the `pipelines/<name>/` directory via the GitHub website, or by deleting the entire repository. \
Only the pipeline's author can unpublish (the Hub verifies this against your GitHub token). GitHub login is required; if no token is configured the tool will tell you to connect GitHub via the AutoPipe app's GitHub panel. \
TWO-CALL PROTOCOL: \
First call (without `scope`): the tool fetches the pipeline's version chain — that is, all rows on the Hub with the same name and same author — and returns the list. You MUST present this list to the user IN THEIR LANGUAGE and ask whether to delete only the latest version or every version (example wording in English; translate to the user's language at runtime: 'This pipeline has N version(s) registered. Delete only the latest version, or all versions?'). Wait for an explicit answer. \
Second call (with `scope='latest'` or `scope='all'`): performs the deletion. Confirm with the user once more in their language before this second call. \
If other users have forked this pipeline, their forks remain on the Hub but their 'based on' reference becomes a dangling pointer that the Hub UI shows as 'original pipeline has been deleted'.")]
    async fn unpublish_pipeline(
        &self,
        Parameters(params): Parameters<UnpublishParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let config = self.config();

        // 1. GitHub login is required so the Hub can verify that the caller
        //    actually owns the pipeline they want to remove.
        let token = match &config.github_token {
            Some(t) if !t.is_empty() => t.clone(),
            _ => {
                return Ok(CallToolResult::error(vec![Content::text(
                    "GitHub login is required to unpublish a pipeline because the Hub verifies ownership via your GitHub token. Please open the AutoPipe app, connect GitHub from the GitHub panel, and try again."
                )]));
            }
        };

        let base = config.registry_url.trim_end_matches('/');
        let client = reqwest::Client::new();

        // 2. Fetch the version chain (same name + same author).
        let chain_url = format!("{}/api/pipelines/{}/versions", base, params.pipeline_id);
        let resp = client
            .get(&chain_url)
            .send()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        if !resp.status().is_success() {
            return Ok(CallToolResult::error(vec![Content::text(
                "Pipeline not found on the Hub."
            )]));
        }
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let versions = body["versions"].as_array().cloned().unwrap_or_default();
        if versions.is_empty() {
            return Ok(CallToolResult::error(vec![Content::text(
                "Pipeline not found on the Hub."
            )]));
        }

        // 3. First call (no scope): return the discovered version list so the
        //    LLM can ask the user which scope to use.
        if params.scope.is_none() {
            let info = versions
                .iter()
                .map(|v| {
                    format!(
                        "- pipeline_id={}, version={}",
                        v["pipeline_id"],
                        v["version"].as_str().unwrap_or("?")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "Pipeline '{}' (by {}) has {} version(s):\n{}\n\nAsk the user whether to delete only the latest version or all versions, then call this tool again with scope='latest' or scope='all'.",
                versions[0]["name"].as_str().unwrap_or(""),
                versions[0]["author"].as_str().unwrap_or(""),
                versions.len(),
                info
            ))]));
        }

        // 4. Second call: figure out which rows to remove based on scope.
        let scope = params.scope.clone().unwrap();
        let targets: Vec<&serde_json::Value> = match scope.as_str() {
            // `versions` is sorted newest-first by createdAt in getVersionChain.
            "latest" => vec![&versions[0]],
            "all" => versions.iter().collect(),
            _ => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid scope '{}'. Use 'latest' or 'all'.",
                    scope
                ))]));
            }
        };

        // The version chain shares one github_url because rows in a chain
        // share name + author, and uploads always go to the same repo.
        let github_url = versions[0]["github_url"].as_str().unwrap_or("");
        let parsed_repo = parse_github_url(github_url);

        let mut results: Vec<String> = Vec::new();
        for v in &targets {
            let pid = v["pipeline_id"].as_i64().unwrap_or(0);
            let pname = v["name"].as_str().unwrap_or("");
            let pver = v["version"].as_str().unwrap_or("");

            // 4a. Delete the Hub registry row.
            let url = format!("{}/api/pipelines/{}", base, pid);
            let resp = client
                .delete(&url)
                .header("Authorization", format!("Bearer {}", token))
                .send()
                .await
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
            let status = resp.status().as_u16();
            if status != 200 {
                let reason = match status {
                    401 => "authentication failed",
                    403 => "forbidden (not your pipeline)",
                    404 => "not found (already deleted?)",
                    _ => "HTTP error",
                };
                results.push(format!(
                    "✗ pipeline_id={} Hub delete failed: {} (HTTP {})",
                    pid, reason, status
                ));
                continue;
            }

            // 4b. Delete the GitHub tag created at publish time so the same
            //     version number can be republished cleanly later. Missing
            //     tag (422 / 404) is treated as success: older pipelines
            //     published before tagging existed will simply have nothing
            //     to remove.
            if let Some((owner, repo, _, _)) = &parsed_repo {
                let tag = format!("{}/v{}", pname.replace(' ', "-"), pver);
                let tag_url = format!(
                    "https://api.github.com/repos/{}/{}/git/refs/tags/{}",
                    owner, repo, tag
                );
                let tag_resp = client
                    .delete(&tag_url)
                    .header("Authorization", format!("Bearer {}", token))
                    .header("User-Agent", "autopipe-desktop")
                    .send()
                    .await;
                match tag_resp {
                    Ok(r)
                        if r.status().is_success()
                            || r.status().as_u16() == 422
                            || r.status().as_u16() == 404 =>
                    {
                        results.push(format!(
                            "✓ pipeline_id={} deleted (Hub row + tag {})",
                            pid, tag
                        ));
                    }
                    Ok(r) => results.push(format!(
                        "✓ pipeline_id={} Hub deleted, but tag {} cleanup returned HTTP {}",
                        pid,
                        tag,
                        r.status().as_u16()
                    )),
                    Err(_) => results.push(format!(
                        "✓ pipeline_id={} Hub deleted, but tag {} cleanup failed (network)",
                        pid, tag
                    )),
                }
            } else {
                results.push(format!(
                    "✓ pipeline_id={} Hub deleted (could not parse github_url for tag cleanup)",
                    pid
                ));
            }
        }

        let pipeline_name = versions[0]["name"].as_str().unwrap_or("");
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Unpublish complete (scope={}, {} version(s) processed):\n{}\n\nNote: GitHub source files in `pipelines/{}/` remain in your repository. This tool does not delete those files — if the user wants to remove them, they must do so directly from GitHub (delete the `pipelines/{}/` directory via the GitHub website, or delete the entire repository). Future uploads of the same pipeline name will overwrite these files; if the user plans to publish a different pipeline under the same name, advise them to change the pipeline name in `ro-crate-metadata.json` to avoid mixing files from this version with the new one.",
            scope,
            targets.len(),
            results.join("\n"),
            pipeline_name,
            pipeline_name
        ))]))
    }

    #[tool(description = "Validate a pipeline directory structure on the remote SSH server")]
    async fn validate_pipeline(
        &self,
        Parameters(params): Parameters<PipelineDirParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Translate Windows-style paths to WSL form before any SSH lookup.
        let pipeline_dir_translated = windows_to_wsl(&params.pipeline_dir);
        let dir = &pipeline_dir_translated;
        let mut errors: Vec<String> = Vec::new();
        // `notices` carries advisory information about state the validator
        // changed on the user's behalf (e.g., auto-filling `#author.name`).
        // Validation still PASSES when only notices are present.
        let mut notices: Vec<String> = Vec::new();
        let required = [
            "Snakefile",
            "Dockerfile",
            "config.yaml",
            "ro-crate-metadata.json",
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
                    if *f == "ro-crate-metadata.json" {
                        match parse_ro_crate_metadata(&content) {
                            Ok(m) => {
                                if m.name.is_empty() {
                                    errors.push("ro-crate-metadata.json: 'name' is empty".into());
                                }
                                // Author auto-fix when GitHub is connected.
                                // We never block the user on this — the AI is
                                // supposed to leave the field empty during
                                // generation if GitHub wasn't connected yet,
                                // and rely on this validator to backfill it
                                // the first time the user runs validation
                                // after connecting GitHub.
                                if m.author.is_empty() {
                                    let token = self.config().github_token.clone();
                                    match fetch_github_login(token.as_deref()).await {
                                        Some(login) => {
                                            match fill_ro_crate_author(&content, &login) {
                                                Ok(updated) => {
                                                    match self
                                                        .ssh_write_file(&path, &updated)
                                                        .await
                                                    {
                                                        Ok(_) => {
                                                            notices.push(format!(
                                                                "Auto-filled '#author.name' in ro-crate-metadata.json with '{}' from your GitHub login.",
                                                                login
                                                            ));
                                                        }
                                                        Err(e) => {
                                                            errors.push(format!(
                                                                "ro-crate-metadata.json: '#author.name' was empty and auto-fill via SSH failed ({}). Check your SSH connection and re-run validate_pipeline; it will retry the fix automatically.",
                                                                e
                                                            ));
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    errors.push(format!(
                                                        "ro-crate-metadata.json: '#author.name' is empty and auto-fill failed to rewrite the file ({}).",
                                                        e
                                                    ));
                                                }
                                            }
                                        }
                                        None => {
                                            errors.push(
                                                "ro-crate-metadata.json: '#author.name' is empty and GitHub is not connected. Open the AutoPipe app, complete the GitHub connection step, then re-run validate_pipeline (it will auto-fill the author from your GitHub login).".into()
                                            );
                                        }
                                    }
                                }
                                // `tools` is intentionally not enforced here.
                                // Pipelines that don't call any bioinformatics
                                // tool (e.g. trivial format conversions, test
                                // fixtures) are still legitimate; the template
                                // guide tells the LLM to populate the field
                                // when real tools are used.
                            }
                            Err(e) => {
                                errors.push(format!("ro-crate-metadata.json: invalid - {}", e))
                            }
                        }
                    }
                }
            }
        }

        if errors.is_empty() {
            let mut msg = String::from("Validation passed. All files present and valid.");
            if !notices.is_empty() {
                msg.push_str("\n\nNotes:");
                for n in &notices {
                    msg.push_str(&format!("\n- {}", n));
                }
            }
            Ok(CallToolResult::success(vec![Content::text(msg)]))
        } else {
            Ok(CallToolResult::error(vec![Content::text(format!(
                "Validation errors:\n{}",
                errors.join("\n")
            ))]))
        }
    }

    #[tool(description = "Read a pipeline's FULL source on the remote server (line-numbered) plus deterministic consistency checks, so you can give the user a natural-language SUMMARY REPORT of what the pipeline does and whether the code looks correct. The summary is written by YOU from the returned code — this tool does not call an LLM; it just gathers every code/config file and runs static checks so your summary is complete and grounded. WHEN TO USE: (1) after GENERATING a pipeline (once files are written and validate_pipeline passes) — ALWAYS call this, present the summary, and get the user's confirmation before build_image; (2) after DOWNLOADING a pipeline — only when the user asked for a detailed summary. Follow the INSTRUCTIONS block in the result exactly: cite file:line for every issue, describe only what is in the code, note any assumptions, and ask the user to confirm or request fixes before proceeding.")]
    async fn review_pipeline(
        &self,
        Parameters(params): Parameters<PipelineDirParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dir = windows_to_wsl(&params.pipeline_dir);
        let dir = dir.trim_end_matches('/').to_string();

        // 1. List every file under the pipeline dir (skip .git internals).
        let list_cmd = format!(
            "find '{}' -type f -not -path '*/.git/*'",
            shell_escape(&dir)
        );
        let listing = match self.ssh_run(&list_cmd).await {
            Ok((out, 0)) => out,
            Ok((out, _)) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Cannot list pipeline directory '{}': {}",
                    dir,
                    out.trim()
                ))]));
            }
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "SSH error while listing '{}': {}",
                    dir, e
                ))]));
            }
        };

        // 2. Keep only reviewable source/config files, as relative paths.
        let prefix = format!("{}/", dir);
        let mut rel_paths: Vec<String> = listing
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .filter_map(|abs| abs.strip_prefix(&prefix).map(|r| r.to_string()))
            .filter(|rel| is_code_review_target(rel))
            .collect();
        rel_paths.sort();

        if rel_paths.is_empty() {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "No reviewable source files found in '{}'. Check the path is a pipeline directory.",
                dir
            ))]));
        }

        // 3. Read each file, enforcing per-file and total size caps so a huge
        //    script can't blow up the context. (relpath, content, truncated)
        const PER_FILE_CAP: usize = 40_000;
        const TOTAL_CAP: usize = 200_000;
        let mut total: usize = 0;
        let mut files: Vec<(String, String, bool)> = Vec::new();
        let mut skipped: Vec<String> = Vec::new();
        for rel in &rel_paths {
            if total >= TOTAL_CAP {
                skipped.push(rel.clone());
                continue;
            }
            let abs = format!("{}/{}", dir, rel);
            let content = match self.ssh_read_file(&abs).await {
                Ok(c) => c,
                Err(_) => {
                    skipped.push(rel.clone());
                    continue;
                }
            };
            let mut truncated = false;
            let mut body = content;
            if body.len() > PER_FILE_CAP {
                let mut cut = PER_FILE_CAP;
                while cut > 0 && !body.is_char_boundary(cut) {
                    cut -= 1;
                }
                body.truncate(cut);
                truncated = true;
            }
            total += body.len();
            files.push((rel.clone(), body, truncated));
        }

        // 4. Deterministic checks (facts with file:line anchors).
        let findings = review_checks(&files);

        // 5. Assemble: checks summary + line-numbered source dump + instructions.
        let mut out = String::new();
        out.push_str(&format!(
            "PIPELINE REVIEW — {} ({} file{})\n\n",
            dir,
            files.len(),
            if files.len() == 1 { "" } else { "s" }
        ));

        out.push_str("=== DETERMINISTIC CHECKS (facts to anchor your report; cite these with file:line) ===\n");
        if findings.is_empty() {
            out.push_str("No structural problems found by static checks. Still read the code below and summarize the actual logic.\n");
        } else {
            for f in &findings {
                let loc = match f.line {
                    Some(n) => format!("{}:{}", f.file, n),
                    None => f.file.clone(),
                };
                let tag = match f.severity {
                    "issue" => "ISSUE",
                    "to-fill" => "TO-FILL",
                    _ => "NOTE",
                };
                out.push_str(&format!("[{}] {} — {}\n", tag, loc, f.message));
            }
        }
        out.push('\n');

        if !skipped.is_empty() {
            out.push_str(&format!(
                "NOTE: {} file(s) not shown (unreadable or size cap reached): {}\n\n",
                skipped.len(),
                skipped.join(", ")
            ));
        }

        out.push_str("=== SOURCE CODE (line-numbered) ===\n");
        for (rel, content, truncated) in &files {
            out.push_str(&format!("\n----- {} -----\n", rel));
            for (i, line) in content.lines().enumerate() {
                out.push_str(&format!("{:>4}  {}\n", i + 1, line));
            }
            if *truncated {
                out.push_str("... [truncated — file exceeds the per-file size cap] ...\n");
            }
        }

        out.push_str(REVIEW_INSTRUCTIONS);

        Ok(CallToolResult::success(vec![Content::text(out)]))
    }

    // ── Execution tools (via SSH) ───────────────────────────────

    #[tool(description = "Build a Docker image for a pipeline on the remote server via SSH. The build runs in the background and returns immediately. After calling this, automatically call check_build_status every 10 seconds until the build completes. Do NOT ask the user to check — poll automatically. If the build fails, analyze the log, call cleanup_failed, fix the pipeline, and retry. Multi-client note: do not start two builds for the same image_name from different AI clients at once. REVIEW GATE: if this pipeline was just GENERATED, you MUST have already run review_pipeline, presented the summary, and gotten the user's confirmation before building — if you have not, do that first.")]
    async fn build_image(
        &self,
        Parameters(params): Parameters<BuildParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let log_dir = self.config().full_output_dir();
        let log_path = format!("{}/build_{}.log", log_dir.trim_end_matches('/'), params.image_name);

        let _ = self.ssh_run(&format!("mkdir -p '{}'", shell_escape(&log_dir))).await;
        let pipeline_dir = windows_to_wsl(&params.pipeline_dir);
        let cmd = format!(
            "cd '{}' && nohup docker build -t '{}' . > '{}' 2>&1 &\necho $!",
            shell_escape(&pipeline_dir), shell_escape(&params.image_name), shell_escape(&log_path)
        );

        match self.ssh_run(&cmd).await {
            Ok((output, 0)) => {
                let pid = output.trim();
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Docker build started in background (PID: {}).\nLog: {}\nNow call check_build_status with image_name='{}' every 10 seconds to monitor progress.",
                    pid, log_path, params.image_name
                ))]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
            _ => Ok(CallToolResult::error(vec![Content::text("Failed to start build")])),
        }
    }

    #[tool(description = "Check the status of a background Docker build started by build_image. Returns building/success/failed status with recent log output. Call this automatically every 10 seconds after build_image — do NOT wait for the user to ask.")]
    async fn check_build_status(
        &self,
        Parameters(params): Parameters<CheckBuildParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Check if docker build process is still running
        let check_cmd = format!(
            "ps aux | grep 'docker build.*{}' | grep -v grep | head -1",
            shell_escape(&params.image_name)
        );
        let is_running = match self.ssh_run(&check_cmd).await {
            Ok((output, _)) => !output.trim().is_empty(),
            Err(_) => false,
        };

        // Check if image exists (build succeeded)
        let image_check = format!("docker images -q '{}' 2>/dev/null", shell_escape(&params.image_name));
        let image_exists = match self.ssh_run(&image_check).await {
            Ok((output, 0)) => !output.trim().is_empty(),
            _ => false,
        };

        // Get recent log output
        let log_dir = self.config().full_output_dir();
        let log_path = format!("{}/build_{}.log", log_dir.trim_end_matches('/'), params.image_name);
        let actual_log_path = log_path.clone();
        let actual_log_path = match self.ssh_run(&format!("test -f '{}' && echo exists", shell_escape(&log_path))).await {
            Ok((output, 0)) if output.trim().contains("exists") => log_path,
            _ => {
                // Fallback: search in pipelines dir (for old builds)
                let pipelines_dir = self.config().full_pipelines_dir();
                let find_log = format!(
                    "find '{}' -name 'build_{}.log' 2>/dev/null | head -1",
                    shell_escape(&pipelines_dir), shell_escape(&params.image_name)
                );
                match self.ssh_run(&find_log).await {
                    Ok((output, 0)) if !output.trim().is_empty() => output.trim().to_string(),
                    _ => actual_log_path,
                }
            }
        };

        let tail_cmd = format!("tail -30 '{}' 2>/dev/null", shell_escape(&actual_log_path));
        let recent_log = match self.ssh_run(&tail_cmd).await {
            Ok((output, _)) => output,
            Err(_) => "Log not found".to_string(),
        };

        if is_running {
            Ok(CallToolResult::success(vec![Content::text(format!(
                "Build in progress...\n\nRecent log:\n{}",
                recent_log
            ))]))
        } else if image_exists {
            // Clean up build log
            let _ = self.ssh_run(&format!("rm -f '{}'", shell_escape(&actual_log_path))).await;
            Ok(CallToolResult::success(vec![Content::text(format!(
                "Build completed successfully! Image '{}' is ready.\n\nFinal log:\n{}",
                params.image_name, recent_log
            ))]))
        } else {
            Ok(CallToolResult::error(vec![Content::text(format!(
                "Build failed.\n\nBuild log:\n{}\n\nNext steps: Analyze the error above, fix the pipeline code (Dockerfile or Snakefile), then retry build_image. Only call cleanup_failed if you need to remove the Docker image before rebuilding.",
                recent_log
            ))]))
        }
    }

    #[tool(description = "Dry-run a pipeline (snakemake -n -p) on the remote server via SSH. If output_dir is omitted, uses the configured output directory.")]
    async fn dry_run(
        &self,
        Parameters(params): Parameters<DryRunParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let cores = params.cores.unwrap_or(8);
        let dry_run_dir = format!("{}/.dry_run_tmp", self.config().full_output_dir().trim_end_matches('/'));
        // Convert any Windows-style paths supplied by the user.
        let input_dir = windows_to_wsl(&params.input_dir);
        let output_dir = params
            .output_dir
            .filter(|s| !s.is_empty())
            .map(|s| windows_to_wsl(&s))
            .unwrap_or_else(|| dry_run_dir.clone());

        let _ = self.ssh_run(&format!("mkdir -p '{}'", shell_escape(&output_dir))).await;

        let pipeline_dir = self.find_pipeline_dir(&params.image_name).await;
        let pipeline_mount = match &pipeline_dir {
            Some(dir) => format!("-v '{}:/pipeline'", shell_escape(dir)),
            None => String::new(),
        };

        let symlink_targets = self.resolve_symlink_targets(&input_dir).await;
        let symlink_mounts: String = symlink_targets.iter()
            .map(|t| format!(" -v '{}:{}:ro'", shell_escape(t), shell_escape(t)))
            .collect();

        let (docker_socket_mount, host_path_mounts, host_env_vars) = if params.needs_docker_socket.unwrap_or(false) {
            let socket = self.resolve_docker_socket_mount().await;
            let mut host_mounts = format!(
                " -v '{}:{}:ro' -v '{}:{}'",
                shell_escape(&input_dir), shell_escape(&input_dir),
                shell_escape(&output_dir), shell_escape(&output_dir),
            );
            if let Some(ref dir) = pipeline_dir {
                host_mounts += &format!(" -v '{}:{}'", shell_escape(dir), shell_escape(dir));
            }
            // Include symlink targets in host_path_mounts so child containers can access them
            for target in &symlink_targets {
                host_mounts += &format!(" -v '{}:{}:ro'", shell_escape(target), shell_escape(target));
            }
            let env_vars = format!(
                " -e HOST_INPUT_DIR='{}' -e HOST_OUTPUT_DIR='{}'{}",
                shell_escape(&input_dir),
                shell_escape(&output_dir),
                pipeline_dir.as_ref().map(|d| format!(" -e HOST_PIPELINE_DIR='{}'", shell_escape(d))).unwrap_or_default(),
            );
            (socket, host_mounts, env_vars)
        } else {
            (String::new(), String::new(), String::new())
        };

        let cmd = format!(
            "docker run --rm --entrypoint snakemake {}{}{}{} -v '{}:/input:ro'{} -v '{}:/output' -w /output '{}' --cores {} --rerun-incomplete --snakefile /pipeline/Snakefile --configfile /pipeline/config.yaml -n -p",
            pipeline_mount, docker_socket_mount, host_path_mounts, host_env_vars, shell_escape(&input_dir), symlink_mounts, shell_escape(&output_dir), shell_escape(&params.image_name), cores
        );

        let result = match self.ssh_run(&cmd).await {
            Ok((output, 0)) => Ok(CallToolResult::success(vec![Content::text(output)])),
            Ok((output, _)) => Ok(CallToolResult::error(vec![Content::text(output)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        };

        // Clean up dry_run temp directory
        if output_dir == dry_run_dir {
            let _ = self.ssh_run(&format!(
                "docker run --rm -v '{}:/target' alpine rm -rf /target",
                shell_escape(&dry_run_dir)
            )).await;
        }

        result
    }

    #[tool(description = "Mount an S3 bucket on the AutoPipe-provisioned VM as a local directory (via rclone using the VM's IAM instance role — no keys), so a pipeline can read it as input WITHOUT downloading. Returns the mount path; pass it as `input_dir` to execute_pipeline. REQUIRES a VM provisioned by AutoPipe (its instance profile grants keyless S3 access) with rclone + fuse installed (done automatically at provisioning). If `bucket` is omitted, uses the bucket selected in the AutoPipe app. Use this before execute_pipeline when the input data lives in the user's S3 bucket.")]
    async fn mount_s3_bucket(
        &self,
        Parameters(params): Parameters<MountS3Params>,
    ) -> Result<CallToolResult, ErrorData> {
        let config = self.config();
        // S3 mounts on an AutoPipe-provisioned AWS VM (it carries the IAM instance
        // role for keyless access). This only makes sense on the Cloud VM tab with
        // a live VM; on the SSH tab we'd be on a self-managed server with no role.
        if config.connection_type != "cloud" || config.aws_vm_host.trim().is_empty() {
            return Ok(CallToolResult::error(vec![Content::text(
                "No AutoPipe AWS VM is active, so there is nothing to mount S3 on. \
                 In AutoPipe, choose the 'Cloud VM' tab, connect your AWS account, and click \
                 'Provision VM'. That VM gets an IAM role for keyless S3 access and AutoPipe \
                 routes the connection to it automatically. (The 'SSH server' tab connects to \
                 your own server, which has no S3 access.)"
                    .to_string(),
            )]));
        }
        let bucket = params
            .bucket
            .filter(|b| !b.trim().is_empty())
            .unwrap_or_else(|| config.aws_bucket.clone());
        if bucket.trim().is_empty() {
            return Ok(CallToolResult::error(vec![Content::text(
                "No S3 bucket specified and none is selected in the AutoPipe app.".to_string(),
            )]));
        }
        let region = if config.aws_region.trim().is_empty() {
            "us-east-1".to_string()
        } else {
            config.aws_region.clone()
        };
        let prefix = params
            .prefix
            .map(|p| p.trim().trim_matches('/').to_string())
            .filter(|p| !p.is_empty());
        let remote_path = match &prefix {
            Some(p) => format!("{}/{}", bucket.trim(), p),
            None => bucket.trim().to_string(),
        };
        let mount_path = "/home/ubuntu/s3input".to_string();

        // rclone on-the-fly S3 backend using the instance role (env_auth),
        // mounted read-only as a background daemon. Ensure fuse3 first.
        let cmd = format!(
            "sudo apt-get install -y -qq fuse3 >/dev/null 2>&1 || true; \
             mkdir -p '{mp}'; \
             fusermount -u '{mp}' >/dev/null 2>&1 || true; \
             rclone mount ':s3,provider=AWS,env_auth=true,region={region}:{remote}' '{mp}' \
                 --read-only --daemon --dir-cache-time 10s --vfs-cache-mode minimal \
                 >/tmp/autopipe-rclone-mount.log 2>&1; \
             sleep 3; \
             if ls '{mp}' >/dev/null 2>&1; then echo MOUNT_OK; else echo MOUNT_FAIL; cat /tmp/autopipe-rclone-mount.log; fi",
            mp = mount_path,
            region = shell_escape(&region),
            remote = shell_escape(&remote_path),
        );

        match self.ssh_run(&cmd).await {
            Ok((out, _)) if out.contains("MOUNT_OK") => Ok(CallToolResult::success(vec![
                Content::text(format!(
                    "Mounted s3://{} at {} on the VM (read-only, via the VM's IAM role — no keys).\n\
                     Pass input_dir='{}' to execute_pipeline to run on this data without downloading it.",
                    remote_path, mount_path, mount_path
                )),
            ])),
            Ok((out, _)) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to mount s3://{}: {}\n\
                 Checklist: (1) the VM was provisioned by AutoPipe (has the instance role), \
                 (2) rclone + fuse3 are installed, (3) the bucket name and region are correct.",
                remote_path,
                out.trim()
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "SSH error while mounting S3: {e}"
            ))])),
        }
    }

    #[tool(description = "Execute a pipeline in the background on the remote server via SSH. Outputs are stored at {configured_output_dir}/{run_name}/. Logs are written to {output_dir}/{run_name}/pipeline.log. This tool monitors the first ~90 seconds for early failures before returning. Snakemake automatically skips completed steps, so if a pipeline fails you can fix the code and re-run with the SAME run_name — only the failed and downstream steps will re-execute. Do NOT call cleanup_failed after execution failures; instead fix the Snakefile and re-run. Tell the user they can check progress later with list_running_pipelines, even from a new conversation session. Multi-client note: this AutoPipe instance may be shared by multiple AI clients (Claude Desktop, Cursor, Codex, etc.); avoid running pipelines with the same run_name simultaneously from different clients — only one execution per run_name at a time. BEFORE running, briefly recap the config.yaml values relevant to the step the user is about to run (one short line each: current value plus a few words). If the pipeline runs in stages and the user is running only a later or optional stage, recap ONLY that stage's values, not the ones already used by earlier stages; if the stages are not clearly separable, recap all configurable values. Then check required values: if any required config value is still blank, do NOT run. If you can derive a sensible value from the config or context, propose it, explain where it comes from, and ask whether to fill it in and run; if you cannot derive one, ask the user to provide it. Never fill a required value on your own without the user's confirmation — run only after the user confirms. If this pipeline was just generated and you have not yet presented a review_pipeline summary and gotten confirmation, do that first.")]
    async fn execute_pipeline(
        &self,
        Parameters(params): Parameters<ExecuteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let cores = params.cores.unwrap_or(8);
        let output_dir = self.resolve_output_dir(&params.run_name);
        let container_name = format!("{}-run", params.run_name);
        let log_path = format!("{}/pipeline.log", output_dir.trim_end_matches('/'));

        // Translate any Windows-style paths the user provided into WSL form so
        // the SSH backend (which is always Linux) can find them.
        let input_dir = windows_to_wsl(&params.input_dir);

        let pipeline_dir = self.find_pipeline_dir(&params.image_name).await;
        let pipeline_mount = match &pipeline_dir {
            Some(dir) => format!("-v '{}:/pipeline'", shell_escape(dir)),
            None => String::new(),
        };

        let symlink_targets = self.resolve_symlink_targets(&input_dir).await;
        let symlink_mounts: String = symlink_targets.iter()
            .map(|t| format!(" -v '{}:{}:ro'", shell_escape(t), shell_escape(t)))
            .collect();

        let (docker_socket_mount, host_path_mounts, host_env_vars) = if params.needs_docker_socket.unwrap_or(false) {
            let socket = self.resolve_docker_socket_mount().await;
            let mut host_mounts = format!(
                " -v '{}:{}:ro' -v '{}:{}'",
                shell_escape(&input_dir), shell_escape(&input_dir),
                shell_escape(&output_dir), shell_escape(&output_dir),
            );
            if let Some(ref dir) = pipeline_dir {
                host_mounts += &format!(" -v '{}:{}'", shell_escape(dir), shell_escape(dir));
            }
            for target in &symlink_targets {
                host_mounts += &format!(" -v '{}:{}:ro'", shell_escape(target), shell_escape(target));
            }
            let env_vars = format!(
                " -e HOST_INPUT_DIR='{}' -e HOST_OUTPUT_DIR='{}'{}",
                shell_escape(&input_dir),
                shell_escape(&output_dir),
                pipeline_dir.as_ref().map(|d| format!(" -e HOST_PIPELINE_DIR='{}'", shell_escape(d))).unwrap_or_default(),
            );
            (socket, host_mounts, env_vars)
        } else {
            (String::new(), String::new(), String::new())
        };

        let _ = self.ssh_run(&format!("docker rm -f '{}' 2>/dev/null", shell_escape(&container_name))).await;
        let _ = self.ssh_run(&format!("mkdir -p '{}'", shell_escape(&output_dir))).await;

        // Resolve the pipeline's identity from its ro-crate so a pipeline viewer
        // can later recognise this run's output by name (image_name is user-chosen
        // and unreliable; the ro-crate `name` is the real identity).
        let (pipeline_name, based_on): (Option<String>, Option<String>) =
            if let Some(ref dir) = pipeline_dir {
                let rc = format!("{}/ro-crate-metadata.json", dir.trim_end_matches('/'));
                match self.ssh_run(&format!("cat '{}'", shell_escape(&rc))).await {
                    Ok((content, 0)) => match parse_ro_crate_metadata(&clean_content(&content)) {
                        Ok(md) => (Some(md.name), md.based_on_url),
                        Err(_) => (None, None),
                    },
                    _ => (None, None),
                }
            } else {
                (None, None)
            };

        // Write run metadata for list_running_pipelines (use base64 to avoid heredoc injection)
        let run_meta = serde_json::json!({
            "run_name": params.run_name,
            "image_name": params.image_name,
            "pipeline": pipeline_name,
            "based_on": based_on,
            "container_name": container_name,
            "input_dir": input_dir,
            "started_at": chrono::Utc::now().to_rfc3339()
        }).to_string();
        let meta_path = format!("{}/.autopipe-run.json", output_dir.trim_end_matches('/'));
        if let Err(e) = self.ssh_write_file(&meta_path, &run_meta).await {
            tracing::warn!(path = %meta_path, "failed to write run metadata: {}", e);
        }

        let cmd = format!(
            "nohup docker run --entrypoint snakemake --name '{}' {}{}{}{} -v '{}:/input:ro'{} -v '{}:/output' -w /output '{}' --cores {} --rerun-incomplete --snakefile /pipeline/Snakefile --configfile /pipeline/config.yaml > '{}' 2>&1 &\necho $!",
            shell_escape(&container_name), pipeline_mount, docker_socket_mount, host_path_mounts, host_env_vars, shell_escape(&input_dir), symlink_mounts, shell_escape(&output_dir), shell_escape(&params.image_name), cores, shell_escape(&log_path)
        );

        match self.ssh_run(&cmd).await {
            Ok((output, 0)) => {
                let pid = output.trim().lines().last().unwrap_or("unknown");

                // Monitor first ~90 seconds for early failures (check at 10s, 30s, 60s, 90s)
                let check_intervals = [10u64, 20, 30, 30];
                for wait_secs in check_intervals {
                    tokio::time::sleep(std::time::Duration::from_secs(wait_secs)).await;

                    let still_running = match self.ssh_run(&format!(
                        "docker inspect -f '{{{{.State.Running}}}}' '{}' 2>/dev/null", shell_escape(&container_name)
                    )).await {
                        Ok((out, 0)) => out.trim() == "true",
                        _ => false,
                    };

                    if !still_running {
                        // Container exited — check if it succeeded or failed
                        let log_tail = match self.ssh_run(&format!("tail -30 '{}' 2>/dev/null", shell_escape(&log_path))).await {
                            Ok((out, 0)) => out,
                            _ => "(no log available)".to_string(),
                        };

                        let has_error = log_tail.contains("Error") || log_tail.contains("error")
                            || log_tail.contains("FAILED") || log_tail.contains("failed")
                            || log_tail.contains("Exiting because a job execution failed");
                        let completed_ok = log_tail.contains("steps (100%) done")
                            || log_tail.contains("Nothing to be done");

                        if completed_ok {
                            return Ok(CallToolResult::success(vec![Content::text(format!(
                                "Pipeline completed successfully!\n\
                                 Output directory: {}\n\
                                 Log: {}\n\n{}",
                                output_dir, log_path, log_tail
                            ))]));
                        } else if has_error {
                            return Ok(CallToolResult::error(vec![Content::text(format!(
                                "Pipeline FAILED early (within 90s). Do NOT call cleanup_failed — intermediate results are preserved.\n\
                                 Fix the Snakefile and re-run execute_pipeline with the SAME run_name. Snakemake will skip completed steps automatically.\n\
                                 Container: {}\n\
                                 Output directory: {}\n\
                                 Log: {}\n\n{}",
                                container_name, output_dir, log_path, log_tail
                            ))]));
                        }
                    }
                }

                // Still running after 90s — no early errors detected
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Pipeline is running (no errors in first 90s). PID: {}, container: '{}'.\n\
                     Output directory: {}\n\
                     Log file: {}\n\
                     The user can check progress anytime (even in a new session) with list_running_pipelines.",
                    pid, container_name, output_dir, log_path
                ))]))
            }
            Ok((output, _)) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to start pipeline:\n{}",
                output
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(description = "List all pipeline runs (running, completed, or failed). Scans the output directory for .autopipe-run.json metadata files and checks container status. No parameters needed — call this when the user asks about pipeline status, progress, or running jobs.")]
    async fn list_running_pipelines(&self) -> Result<CallToolResult, ErrorData> {
        let output_base = self.config().full_output_dir();

        // Find all run metadata files (use glob instead of find -exec to avoid {} escaping issues over SSH)
        let find_cmd = format!(
            "for f in '{}'/*/.autopipe-run.json; do [ -f \"$f\" ] && cat \"$f\"; done 2>/dev/null",
            shell_escape(&output_base)
        );
        let meta_output = match self.ssh_run(&find_cmd).await {
            Ok((output, _)) => output,
            Err(e) => return Ok(CallToolResult::error(vec![Content::text(format!("Cannot scan output directory: {}", e))])),
        };

        if meta_output.trim().is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No pipeline runs found in the output directory."
            )]));
        }

        // Get running containers
        let docker_cmd = "docker ps --filter 'name=-run' --format '{{.Names}} {{.Status}}' 2>/dev/null";
        let running_containers = match self.ssh_run(docker_cmd).await {
            Ok((output, _)) => output,
            _ => String::new(),
        };

        let mut results = Vec::new();
        for line in meta_output.lines() {
            let line = line.trim();
            if !line.starts_with('{') { continue; }

            let meta: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let run_name = meta["run_name"].as_str().unwrap_or("unknown");
            let image = meta["image_name"].as_str().unwrap_or("unknown");
            let container = meta["container_name"].as_str().unwrap_or("unknown");
            let started = meta["started_at"].as_str().unwrap_or("unknown");

            // Check if container is running
            let is_running = running_containers.lines().any(|l| l.starts_with(container));
            let docker_status = running_containers.lines()
                .find(|l| l.starts_with(container))
                .unwrap_or("");

            // Read last line of pipeline.log for progress
            let log_path = format!("{}/{}/pipeline.log", output_base.trim_end_matches('/'), run_name);
            let last_line = match self.ssh_run(&format!("tail -3 '{}' 2>/dev/null", shell_escape(&log_path))).await {
                Ok((out, 0)) => {
                    let trimmed = out.trim();
                    if trimmed.len() > 200 { trimmed[..200].to_string() } else { trimmed.to_string() }
                }
                _ => "(no log)".to_string(),
            };

            let status = if is_running {
                format!("RUNNING ({})", docker_status.split_whitespace().skip(1).collect::<Vec<_>>().join(" "))
            } else if last_line.contains("100%) done") || last_line.contains("Nothing to be done") {
                "COMPLETED".to_string()
            } else if last_line.contains("Error") || last_line.contains("failed") || last_line.contains("FAILED") {
                "FAILED".to_string()
            } else {
                "STOPPED".to_string()
            };

            results.push(format!(
                "- {} [{}]\n  Image: {}\n  Started: {}\n  Log: {}",
                run_name, status, image, started, last_line
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Pipeline runs ({}):\n\n{}",
            results.len(),
            results.join("\n\n")
        ))]))
    }

    #[tool(description = "Check pipeline execution status. Inspects the Docker container for exit code, OOM kill, and other abnormal terminations. If the container has stopped, saves termination info to .autopipe-run.json and removes the container. Falls back to saved JSON when the container is already gone.")]
    async fn check_status(
        &self,
        Parameters(params): Parameters<StatusParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let output_dir = self.resolve_output_dir(&params.run_name);
        let container_name = format!("{}-run", params.run_name);
        let log_path = format!("{}/pipeline.log", output_dir.trim_end_matches('/'));
        let meta_path = format!("{}/.autopipe-run.json", output_dir.trim_end_matches('/'));

        // Step 1: Try docker inspect for full container state
        let inspect_fmt = "{{.State.Running}}|{{.State.ExitCode}}|{{.State.OOMKilled}}|{{.State.FinishedAt}}";
        let inspect_result = self.ssh_run(&format!(
            "docker inspect -f '{}' '{}' 2>/dev/null",
            inspect_fmt, shell_escape(&container_name)
        )).await;

        // Step 2: Read log tail
        let log_output = match self.ssh_run(&format!("tail -50 '{}' 2>/dev/null", shell_escape(&log_path))).await {
            Ok((output, 0)) => output,
            Ok((output, _)) => format!("(log not available: {})", output.trim()),
            Err(e) => format!("(cannot read log: {})", e),
        };

        match inspect_result {
            Ok((output, 0)) => {
                let parts: Vec<&str> = output.trim().splitn(4, '|').collect();
                if parts.len() == 4 {
                    let is_running = parts[0] == "true";
                    let exit_code = parts[1];
                    let oom_killed = parts[2] == "true";
                    let finished_at = parts[3];

                    if is_running {
                        // Container is still running
                        return Ok(CallToolResult::success(vec![Content::text(format!(
                            "Status: RUNNING\nContainer: {}\nOutput: {}\nLog ({}):\n{}",
                            container_name, output_dir, log_path, log_output
                        ))]));
                    }

                    // Container stopped — determine cause
                    let termination_reason = if oom_killed {
                        format!("OOM_KILLED (Out of Memory) — exit code: {}", exit_code)
                    } else if exit_code == "137" {
                        format!("KILLED (signal 9, likely OOM or manual kill) — exit code: {}", exit_code)
                    } else if exit_code == "139" {
                        format!("SEGFAULT (signal 11) — exit code: {}", exit_code)
                    } else if exit_code == "143" {
                        format!("TERMINATED (signal 15) — exit code: {}", exit_code)
                    } else if exit_code == "0" {
                        "COMPLETED (exit code: 0)".to_string()
                    } else {
                        format!("FAILED — exit code: {}", exit_code)
                    };

                    // Save termination info to .autopipe-run.json
                    let meta_read = self.ssh_run(&format!("cat '{}' 2>/dev/null", shell_escape(&meta_path))).await;
                    let mut meta: serde_json::Value = match &meta_read {
                        Ok((content, 0)) => serde_json::from_str(content.trim()).unwrap_or(serde_json::json!({})),
                        _ => serde_json::json!({}),
                    };
                    meta["exit_code"] = serde_json::json!(exit_code);
                    meta["oom_killed"] = serde_json::json!(oom_killed);
                    meta["finished_at"] = serde_json::json!(finished_at);
                    meta["termination_reason"] = serde_json::json!(termination_reason);
                    if let Err(e) = self.ssh_write_file(&meta_path, &meta.to_string()).await {
                        tracing::warn!(path = %meta_path, "failed to write metadata: {}", e);
                    }

                    // Remove stopped container to avoid accumulation
                    let _ = self.ssh_run(&format!("docker rm '{}' 2>/dev/null", shell_escape(&container_name))).await;

                    return Ok(CallToolResult::success(vec![Content::text(format!(
                        "Status: {}\nContainer: {} (removed after inspection)\nOutput: {}\nFinished at: {}\nLog ({}):\n{}",
                        termination_reason, container_name, output_dir, finished_at, log_path, log_output
                    ))]));
                }
            }
            _ => {}
        }

        // Step 3: Container doesn't exist — fall back to saved JSON metadata
        let meta_info = match self.ssh_run(&format!("cat '{}' 2>/dev/null", shell_escape(&meta_path))).await {
            Ok((content, 0)) => {
                match serde_json::from_str::<serde_json::Value>(content.trim()) {
                    Ok(meta) => {
                        let reason = meta["termination_reason"].as_str().unwrap_or("UNKNOWN");
                        let finished = meta["finished_at"].as_str().unwrap_or("unknown");
                        let exit_code = meta["exit_code"].as_str().unwrap_or("unknown");
                        let oom = meta["oom_killed"].as_bool().unwrap_or(false);

                        if meta.get("termination_reason").is_some() {
                            format!("Status: {} (from saved metadata)\nExit code: {}\nOOM killed: {}\nFinished at: {}",
                                reason, exit_code, oom, finished)
                        } else {
                            "Status: UNKNOWN (container gone, no termination info saved)".to_string()
                        }
                    }
                    Err(_) => "Status: UNKNOWN (container gone, metadata unreadable)".to_string(),
                }
            }
            _ => "Status: UNKNOWN (no container, no metadata found)".to_string(),
        };

        Ok(CallToolResult::success(vec![Content::text(format!(
            "{}\nContainer: {}\nOutput: {}\nLog ({}):\n{}",
            meta_info, container_name, output_dir, log_path, log_output
        ))]))
    }

    // ── Cleanup tools ────────────────────────────────────────────

    #[tool(description = "Clean up artifacts from a failed pipeline. By default, preserves the output directory (so Snakemake can resume from completed steps) and only removes the Docker image. Set remove_output=true ONLY when you want a completely fresh start. STOPPING A RUN: by default this aborts if the pipeline is still running. When the user asks to stop/abort a run, or a run is hung and needs to be killed, call with stop_running=true — it stops the container(s) first (docker stop), then cleans up. Uses Docker to handle root-owned files when normal rm fails due to permissions. For execution failures, prefer fixing the Snakefile and re-running execute_pipeline with the same run_name instead of calling this tool. Multi-client note: do NOT call this (especially with stop_running=true) on a run_name that another AI client may currently be executing — coordinate with the user first.")]
    async fn cleanup_failed(
        &self,
        Parameters(params): Parameters<CleanupFailedParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let output_dir = self.resolve_output_dir(&params.run_name);
        let mut results = Vec::new();

        // 1. Check if image has running containers
        let running_check = format!(
            "docker ps -q --filter ancestor='{}' 2>/dev/null",
            shell_escape(&params.image_name)
        );
        if let Ok((output, 0)) = self.ssh_run(&running_check).await {
            if !output.trim().is_empty() {
                if params.stop_running.unwrap_or(false) {
                    // The user asked to stop the run (or it is hung): stop the
                    // running container(s) for this image, then clean up. docker
                    // stop is graceful (SIGTERM, then SIGKILL after a grace period).
                    let stop_cmd = format!(
                        "docker ps -q --filter ancestor='{}' 2>/dev/null | xargs -r docker stop",
                        shell_escape(&params.image_name)
                    );
                    match self.ssh_run(&stop_cmd).await {
                        Ok((_, 0)) => results.push(format!(
                            "Stopped running containers for image: {}",
                            params.image_name
                        )),
                        Ok((err, _)) => return Ok(CallToolResult::error(vec![Content::text(format!(
                            "Failed to stop running containers for '{}': {}",
                            params.image_name, err.trim()
                        ))])),
                        Err(e) => return Ok(CallToolResult::error(vec![Content::text(format!(
                            "Error stopping running containers for '{}': {}",
                            params.image_name, e
                        ))])),
                    }
                } else {
                    return Ok(CallToolResult::error(vec![Content::text(format!(
                        "Cannot clean up: image '{}' has running containers. Pass stop_running=true to stop them first, or stop them manually.",
                        params.image_name
                    ))]));
                }
            }
        }

        // 2. Remove stopped container if it exists
        let container_name = format!("{}-run", params.run_name);
        let rm_container = format!("docker rm '{}' 2>/dev/null", shell_escape(&container_name));
        match self.ssh_run(&rm_container).await {
            Ok((_, 0)) => results.push(format!("Removed stopped container: {}", container_name)),
            _ => {} // container already gone, no action needed
        }

        // 3. Remove failed output directory (only if explicitly requested)
        if params.remove_output.unwrap_or(false) {
            let check_dir = format!("test -d '{}'", shell_escape(&output_dir));
            if let Ok((_, 0)) = self.ssh_run(&check_dir).await {
                let rm_cmd = format!("rm -rf '{}'", shell_escape(&output_dir));
                match self.ssh_run(&rm_cmd).await {
                    Ok((_, 0)) => results.push(format!("Removed output directory: {}", output_dir)),
                    Ok((err, _)) => {
                        // rm failed (likely root-owned files from Docker) — retry with Docker
                        let docker_rm = format!(
                            "docker run --rm -v '{}:/target' alpine rm -rf /target",
                            shell_escape(&output_dir)
                        );
                        match self.ssh_run(&docker_rm).await {
                            Ok((_, 0)) => {
                                let mkdir_cmd = format!("mkdir -p '{}'", shell_escape(&output_dir));
                                let _ = self.ssh_run(&mkdir_cmd).await;
                                results.push(format!("Removed output directory via Docker (root-owned files): {}", output_dir));
                            }
                            _ => results.push(format!("Failed to remove output directory (permission denied): {}", err.trim())),
                        }
                    }
                    Err(e) => results.push(format!("Error removing output directory: {}", e)),
                }
            } else {
                results.push(format!("Output directory not found (already clean): {}", output_dir));
            }
        } else {
            results.push(format!("Output directory preserved (Snakemake will resume from completed steps): {}", output_dir));
        }

        // 4. Remove Docker image
        let check_img = format!(
            "docker images -q '{}' 2>/dev/null",
            shell_escape(&params.image_name)
        );
        if let Ok((output, 0)) = self.ssh_run(&check_img).await {
            if !output.trim().is_empty() {
                let rmi_cmd = format!("docker rmi '{}' 2>/dev/null", shell_escape(&params.image_name));
                match self.ssh_run(&rmi_cmd).await {
                    Ok((_, 0)) => results.push(format!("Removed Docker image: {}", params.image_name)),
                    Ok((err, _)) => results.push(format!("Failed to remove image: {}", err.trim())),
                    Err(e) => results.push(format!("Error removing image: {}", e)),
                }
            } else {
                results.push(format!("Docker image not found (already clean): {}", params.image_name));
            }
        }

        // 5. Prune dangling images from failed builds
        let _ = self.ssh_run("docker image prune -f --filter dangling=true 2>/dev/null").await;
        results.push("Pruned dangling images from incomplete builds.".to_string());

        Ok(CallToolResult::success(vec![Content::text(
            results.join("\n"),
        )]))
    }

    #[tool(description = "Delete a pipeline's source code directory and its Docker image/containers from the remote server. \
ONLY call this when the user has explicitly said they want to delete the local pipeline code (e.g. 'delete the pipeline', 'remove the pipeline'). \
Do NOT call this during build errors, execution failures, code generation, or any troubleshooting — use cleanup_failed for those cases. \
Before calling this tool, ask the user once to confirm (e.g. 'Are you sure you want to delete the pipeline source code?'), then call this tool immediately after confirmation. Do NOT ask the user to run commands manually. \
Uses Docker to handle root-owned files so permissions are never an issue.")]
    async fn delete_pipeline(
        &self,
        Parameters(params): Parameters<DeletePipelineParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut results = Vec::new();
        // Translate any Windows-style path supplied by the user.
        let pipeline_dir = windows_to_wsl(&params.pipeline_dir);

        // 1. Stop and remove any running/stopped containers, and Docker image (if image_name provided)
        if let Some(ref image_name) = params.image_name {
            let stop_cmd = format!(
                "docker ps -q --filter ancestor='{}' 2>/dev/null | xargs -r docker stop 2>/dev/null; \
                 docker ps -aq --filter ancestor='{}' 2>/dev/null | xargs -r docker rm 2>/dev/null",
                shell_escape(image_name),
                shell_escape(image_name),
            );
            let _ = self.ssh_run(&stop_cmd).await;
            results.push(format!("Stopped and removed containers for image: {}", image_name));

            let check_img = format!("docker images -q '{}' 2>/dev/null", shell_escape(image_name));
            if let Ok((output, 0)) = self.ssh_run(&check_img).await {
                if !output.trim().is_empty() {
                    let rmi_cmd = format!("docker rmi '{}' 2>/dev/null", shell_escape(image_name));
                    match self.ssh_run(&rmi_cmd).await {
                        Ok((_, 0)) => results.push(format!("Removed Docker image: {}", image_name)),
                        Ok((err, _)) => results.push(format!("Failed to remove image: {}", err.trim())),
                        Err(e) => results.push(format!("Error removing image: {}", e)),
                    }
                } else {
                    results.push(format!("Docker image not found (already removed): {}", image_name));
                }
            }
        }

        // 3. Delete pipeline directory — try rm -rf first, fall back to Docker
        let check_dir = format!("test -d '{}' && echo 'exists'", shell_escape(&pipeline_dir));
        match self.ssh_run(&check_dir).await {
            Ok((output, 0)) if output.trim().contains("exists") => {
                let rm_cmd = format!("rm -rf '{}'", shell_escape(&pipeline_dir));
                match self.ssh_run(&rm_cmd).await {
                    Ok((_, 0)) => {
                        results.push(format!("Removed pipeline directory: {}", pipeline_dir));
                    }
                    _ => {
                        // Fallback: mount parent dir and delete subdirectory via Docker
                        let path = std::path::Path::new(&pipeline_dir);
                        let parent = path.parent()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|| "/".to_string());
                        let dirname = path.file_name()
                            .map(|f| f.to_string_lossy().to_string())
                            .unwrap_or_default();
                        let docker_rm = format!(
                            "docker run --rm -v '{}:/target' alpine rm -rf '/target/{}'",
                            shell_escape(&parent),
                            shell_escape(&dirname),
                        );
                        match self.ssh_run(&docker_rm).await {
                            Ok((_, 0)) => results.push(format!(
                                "Removed pipeline directory via Docker (root-owned files): {}",
                                pipeline_dir
                            )),
                            Ok((err, _)) => results.push(format!(
                                "Failed to remove pipeline directory: {}", err.trim()
                            )),
                            Err(e) => results.push(format!("Error removing pipeline directory: {}", e)),
                        }
                    }
                }
            }
            _ => results.push(format!("Pipeline directory not found: {}", pipeline_dir)),
        }

        Ok(CallToolResult::success(vec![Content::text(results.join("\n"))]))
    }

    // ── Remote file tools ───────────────────────────────────────

    #[tool(description = "Create a symbolic link on the remote SSH server. Use this to link input/output data instead of copying files. Prefer symlinks over cp for accessing result files and plots.")]
    async fn create_symlink(
        &self,
        Parameters(params): Parameters<CreateSymlinkParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let source = windows_to_wsl(&params.source);
        let target = windows_to_wsl(&params.target);

        match self.ssh_run(&format!("test -e '{}' && echo 'exists'", shell_escape(&source))).await {
            Ok((output, 0)) if output.trim().contains("exists") => {}
            _ => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Source path '{}' does not exist on remote server",
                    source
                ))]));
            }
        }

        if let Some(parent) = std::path::Path::new(&target).parent() {
            let _ = self
                .ssh_run(&format!("mkdir -p '{}'", shell_escape(&parent.to_string_lossy())))
                .await;
        }

        match self.ssh_run(&format!("ln -sf '{}' '{}'", shell_escape(&source), shell_escape(&target))).await {
            Ok((_, 0)) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Symlink created: {} -> {}",
                target, source
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
        let symlink_path = windows_to_wsl(&params.symlink_path);
        let cmd = format!(
            "test -L '{}' && rm '{}' && echo 'removed' || echo 'not_a_symlink'",
            shell_escape(&symlink_path), shell_escape(&symlink_path)
        );
        match self.ssh_run(&cmd).await {
            Ok((output, 0)) if output.trim().contains("removed") => {
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Symlink '{}' removed",
                    symlink_path
                ))]))
            }
            Ok((_, 0)) => Ok(CallToolResult::error(vec![Content::text(format!(
                "'{}' is not a symlink or does not exist",
                symlink_path
            ))])),
            Ok((output, _)) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed: {}",
                output.trim()
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(description = "List files and directories at a remote path on the SSH server. RESULTS-VIEWING WORKFLOW: when the user asks to view, check, or see results/output files: (1) call list_files to see what's there, (2) give the user a brief one-paragraph summary in chat (file count, names, sizes, optionally a 5-10 line excerpt from a single small text/log/summary file when it adds context), (3) tell them the visual viewer (show_results) is available for detailed inspection — especially needed for images, plots, genomic tracks, large tables, or any binary file. Do NOT ask 'viewer or chat?' and do NOT dump entire file contents into chat. If they explicitly ask for a specific file's full contents in chat, then use read_file for that one file.")]
    async fn list_files(
        &self,
        Parameters(params): Parameters<ListFilesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = windows_to_wsl(&params.path);
        match self.ssh_run(&format!("ls -la '{}'", shell_escape(&path))).await {
            Ok((output, 0)) => Ok(CallToolResult::success(vec![Content::text(output)])),
            Ok((output, _)) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Cannot list '{}': {}",
                path,
                output.trim()
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(description = "Read a file's contents on the remote SSH server. Two valid uses:\n\
                          (1) INTERNAL ANALYSIS — you (the AI) need to inspect a code or config file for your own work, e.g. the pre-download security review required by download_pipeline, or sanity-checking a Snakefile before a build. In this mode read silently: do NOT echo the full contents to the user, surface only specific findings (file path, line number, snippet, plain-English concern). No prior user permission is required because nothing is shown to the user yet.\n\
                          (2) USER ASKED FOR ONE FILE'S CONTENTS — the user explicitly said something like 'show me X' for a specific text/log/summary file. Read it and display the contents. For binary/image/large/genomic files (BAM, VCF, BCF, BED, GFF, CRAM, PNG, PDF, HDF5, etc.) do NOT use read_file — direct the user to show_results instead.\n\
                          INPUT DATA — SENSITIVE: Files under the input directory (user-provided data, possibly patient/PHI) must NOT be read without the user's consent. If you call read_file on one, the tool returns a consent request instead of the content — relay it to the user and only re-call with confirm_read=true after they explicitly agree; if they decline, ask them for the specific values you need (column names, group labels, sample IDs) instead of reading the file. Do NOT read raw user data at external paths directly either — stage user data with prepare_input (into the input directory) first, then read with consent. Large or binary files are refused by this tool — use show_results to view them.\n\
                          Do NOT proactively ask 'do you want to save this locally?' after displaying — only mention download_results if the user themselves expresses interest in keeping a copy.")]
    async fn read_file(
        &self,
        Parameters(params): Parameters<ReadFileParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = windows_to_wsl(&params.path);
        let config = self.config();

        // ── Privacy gate: input-directory files need explicit user consent ──
        // Files under the configured input directory are user-provided data
        // (often symlinked in via prepare_input) and may be sensitive (e.g.
        // patient/PHI data). Do not load them into the conversation without the
        // user agreeing. This gate lives in the read_file TOOL only — the shared
        // ssh_read_file helper is untouched, so upload/validate/download-review
        // reads of pipeline files are unaffected.
        let input_trimmed = config.full_input_dir().trim_end_matches('/').to_string();
        let under_input = !input_trimmed.is_empty()
            && (path == input_trimmed || path.starts_with(&format!("{}/", input_trimmed)));
        if under_input && params.confirm_read != Some(true) {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "NOT READ — consent required. '{}' is in the input directory and may be user-provided data, which can be sensitive (e.g. patient/PHI data). Do NOT load it into the conversation without the user's permission.\n\n\
                 Ask the user, in their language, whether you may read this input file:\n\
                 - If they agree: call read_file again with confirm_read=true.\n\
                 - If they decline: do NOT read it — instead ask them to tell you the specific information you need (e.g. column names, group labels, sample IDs) so you can proceed without exposing the data.",
                path
            ))]));
        }

        // ── Read at most MAX+1 bytes so huge/raw data is never loaded into the
        //    conversation (raw data belongs in the viewer, not the chat). This
        //    applies regardless of location, covering sensitive data the user
        //    may point at outside the input directory. ──
        const MAX: usize = 2 * 1024 * 1024; // 2 MB
        let read_cmd = format!("head -c {} '{}'", MAX + 1, shell_escape(&path));
        let content = match self.ssh_run(&read_cmd).await {
            Ok((out, 0)) => out,
            Ok((out, _)) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Cannot read '{}': {}", path, out.trim()
                ))]));
            }
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Cannot read '{}': {}", path, e
                ))]));
            }
        };

        if content.len() > MAX {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "NOT READ — '{}' is too large to load into the conversation (> {} MB). Large files are usually raw data; view it with show_results (in the browser) or save it with download_results instead.",
                path, MAX / (1024 * 1024)
            ))]));
        }
        if content.as_bytes().iter().take(8192).any(|&b| b == 0) {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "NOT READ — '{}' looks like a binary file (contains NUL bytes). Use show_results to view it in the browser instead of read_file.",
                path
            ))]));
        }
        Ok(CallToolResult::success(vec![Content::text(content)]))
    }

    #[tool(description = "Download file(s) from the remote SSH server to the user's local machine. Use this when the user wants to save result files locally. If local_dir is omitted, files are saved to the OS default Downloads folder. Tell the user the default path and ask if they want to change it. Supports single files and directories.")]
    async fn download_results(
        &self,
        Parameters(params): Parameters<DownloadResultsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // remote_path comes from the user / chat and may be a Windows-style
        // path; translate before using it on the (Linux) SSH backend. local_dir
        // is a real Windows path on the desktop side, so leave it untouched.
        let remote_path = windows_to_wsl(&params.remote_path);

        // Resolve local directory: user-specified or OS default Downloads folder
        let local_dir = match &params.local_dir {
            Some(dir) if !dir.is_empty() => dir.clone(),
            _ => {
                match dirs::download_dir() {
                    Some(p) => p.to_string_lossy().to_string(),
                    None => {
                        // Fallback: ~/Downloads
                        match dirs::home_dir() {
                            Some(h) => format!("{}/Downloads", h.to_string_lossy()),
                            None => return Ok(CallToolResult::error(vec![Content::text(
                                "Cannot determine Downloads folder. Please specify local_dir explicitly."
                            )])),
                        }
                    }
                }
            }
        };

        // Ensure local directory exists
        if let Err(e) = std::fs::create_dir_all(&local_dir) {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Cannot create local directory '{}': {}",
                local_dir, e
            ))]));
        }

        // Check if remote path is a directory or file
        let is_dir = match self.ssh_run(&format!("test -d '{}' && echo DIR || echo FILE", shell_escape(&remote_path))).await {
            Ok((output, 0)) => output.trim() == "DIR",
            _ => false,
        };

        if is_dir {
            // List files in the remote directory
            let files = match self.ssh_run(&format!("find '{}' -maxdepth 1 -type f -printf '%f\\n'", shell_escape(&remote_path))).await {
                Ok((output, 0)) => output.trim().to_string(),
                Ok((output, _)) => return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Cannot list directory '{}': {}", remote_path, output.trim()
                ))])),
                Err(e) => return Ok(CallToolResult::error(vec![Content::text(e)])),
            };

            if files.is_empty() {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "No files found in '{}'", remote_path
                ))]));
            }

            let file_list: Vec<&str> = files.lines().collect();
            let mut downloaded = Vec::new();
            let mut errors = Vec::new();

            for file_name in &file_list {
                let remote_file = format!("{}/{}", remote_path.trim_end_matches('/'), file_name);
                let local_file = format!("{}/{}", local_dir.trim_end_matches('/'), file_name);

                match self.ssh_download_base64(&remote_file, &local_file).await {
                    Ok(size) => downloaded.push(format!("  {} ({} bytes)", file_name, size)),
                    Err(e) => errors.push(format!("  {}: {}", file_name, e)),
                }
            }

            let mut msg = format!("Downloaded to: {}\n\n", local_dir);
            if !downloaded.is_empty() {
                msg.push_str(&format!("✓ {} file(s) saved:\n{}\n", downloaded.len(), downloaded.join("\n")));
            }
            if !errors.is_empty() {
                msg.push_str(&format!("\n✗ {} error(s):\n{}", errors.len(), errors.join("\n")));
            }
            Ok(CallToolResult::success(vec![Content::text(msg)]))
        } else {
            // Single file download
            let file_name = std::path::Path::new(&remote_path)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "downloaded_file".to_string());
            let local_file = format!("{}/{}", local_dir.trim_end_matches('/'), file_name);

            match self.ssh_download_base64(&remote_path, &local_file).await {
                Ok(size) => Ok(CallToolResult::success(vec![Content::text(format!(
                    "Downloaded to: {}\n✓ {} ({} bytes)",
                    local_file, file_name, size
                ))])),
                Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                    "Download failed for '{}': {}", remote_path, e
                ))])),
            }
        }
    }

    #[tool(description = "Write content to a file on the remote SSH server. Creates parent directories if needed.")]
    async fn write_file(
        &self,
        Parameters(params): Parameters<WriteFileParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = windows_to_wsl(&params.path);
        if let Some(parent) = std::path::Path::new(&path).parent() {
            let _ = self
                .ssh_run(&format!("mkdir -p '{}'", shell_escape(&parent.to_string_lossy())))
                .await;
        }

        match self.ssh_write_file(&path, &params.content).await {
            Ok(()) => Ok(CallToolResult::success(vec![Content::text(format!(
                "File written: {}",
                path
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Cannot write '{}': {}",
                path, e
            ))])),
        }
    }

    // ── Input preparation tools ─────────────────────────────────

    #[tool(description = "Prepare input data for pipeline execution by downloading a file from a URL or symlinking an existing file on the remote server into the configured pipelines_input directory. \
If source starts with http://, https://, or ftp://, the download runs in the background and returns immediately. \
After calling this for a URL, automatically call check_download_status with the returned filename every 10 seconds until complete. Do NOT ask the user to check — poll automatically. \
Otherwise, a symlink is created pointing to the given absolute path. \
Returns the destination directory path — pass this as input_dir to dry_run or execute_pipeline. \
If this tool fails, fall back to create_symlink using the dest_dir shown in the error message as the target directory. \
Always stage user-provided data here before inspecting it: read_file applies a consent gate to files in this input directory, so do not read raw user data at its original external path — bring it in with prepare_input first.")]
    async fn prepare_input(
        &self,
        Parameters(params): Parameters<PrepareInputParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let config = self.config();
        let base = config.full_input_dir();
        let dest_dir = match &params.subdir {
            Some(sub) if !sub.is_empty() => format!("{}/{}", base.trim_end_matches('/'), sub.trim_matches('/')),
            _ => base.clone(),
        };

        // Ensure destination directory exists (with hard timeout to prevent hang)
        if let Err(e) = self.ssh_run_timed(&format!("mkdir -p '{}'", shell_escape(&dest_dir)), 60).await {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to create input directory '{}': {}\nFallback: use create_symlink with target='{}/{}'",
                dest_dir, e, dest_dir, "<filename>"
            ))]));
        }

        let is_url = params.source.starts_with("http://")
            || params.source.starts_with("https://")
            || params.source.starts_with("ftp://");

        if is_url {
            // Infer filename from URL if not provided
            let filename = params.filename
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    params.source
                        .split('/')
                        .last()
                        .filter(|s| !s.is_empty())
                        .unwrap_or("downloaded_file")
                        .to_string()
                });

            let dest_file = format!("{}/{}", dest_dir.trim_end_matches('/'), filename);
            let log_path = format!("/tmp/autopipe_dl_{}.log", filename);
            let exit_path = format!("/tmp/autopipe_dl_{}.exit", filename);

            // Remove stale exit/log files from a previous attempt
            let _ = self.ssh_run_timed(&format!(
                "rm -f '{}' '{}'", shell_escape(&log_path), shell_escape(&exit_path)
            ), 10).await;

            let cmd = format!(
                "nohup sh -c \"docker run --rm --network host -v '{}:/downloads' alpine sh -c \
                 \\\"wget -qO '/downloads/{}' '{}' 2>&1 || curl -fsSL -o '/downloads/{}' '{}'\\\" \
                 > '{}' 2>&1; echo \\$? > '{}'\" >/dev/null 2>&1 & echo $!",
                shell_escape(&dest_dir),
                shell_escape(&filename),
                shell_escape(&params.source),
                shell_escape(&filename),
                shell_escape(&params.source),
                shell_escape(&log_path),
                shell_escape(&exit_path),
            );

            match self.ssh_run_timed(&cmd, 10).await {
                Ok((output, 0)) => {
                    let pid = output.trim();
                    Ok(CallToolResult::success(vec![Content::text(format!(
                        "Download started in background (PID: {}).\nDestination: {}\nNow call check_download_status with filename='{}' every 10 seconds to monitor progress.",
                        pid, dest_file, filename
                    ))]))
                }
                Ok((output, _)) => Ok(CallToolResult::error(vec![Content::text(format!(
                    "Failed to start download for '{}':\n{}", params.source, output.trim()
                ))])),
                Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
            }
        } else {
            // The source for the symlink branch is a remote-server filesystem
            // path. Translate Windows-style inputs (e.g. C:\data\foo) into
            // their WSL equivalents before invoking SSH commands.
            let source = windows_to_wsl(&params.source);
            let link_name = std::path::Path::new(&source)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "input".to_string());
            let link_path = format!("{}/{}", dest_dir.trim_end_matches('/'), link_name);

            // Symlink: verify source exists (with hard timeout)
            match self.ssh_run_timed(&format!("test -e '{}' && echo 'exists'", shell_escape(&source)), 60).await {
                Ok((output, 0)) if output.trim().contains("exists") => {}
                _ => {
                    return Ok(CallToolResult::error(vec![Content::text(format!(
                        "Source path '{}' does not exist on the remote server.\nFallback: use create_symlink with source='{}' target='{}'",
                        source, source, link_path
                    ))]));
                }
            }

            match self.ssh_run_timed(&format!("ln -sf '{}' '{}'", shell_escape(&source), shell_escape(&link_path)), 60).await {
                Ok((_, 0)) => Ok(CallToolResult::success(vec![Content::text(format!(
                    "Linked: {} -> {}\nUse as input_dir: {}",
                    link_path, source, dest_dir
                ))])),
                Ok((output, _)) => Ok(CallToolResult::error(vec![Content::text(format!(
                    "Failed to create symlink: {}\nFallback: use create_symlink with source='{}' target='{}'",
                    output.trim(), source, link_path
                ))])),
                Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                    "{}\nFallback: use create_symlink with source='{}' target='{}'",
                    e, source, link_path
                ))])),
            }
        }
    }

    #[tool(description = "Check the status of a background file download started by prepare_input. \
Returns downloading/success/failed status with recent log output. \
Call this automatically every 10 seconds after prepare_input — do NOT wait for the user to ask.")]
    async fn check_download_status(
        &self,
        Parameters(params): Parameters<CheckDownloadParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let log_path = format!("/tmp/autopipe_dl_{}.log", params.filename);
        let exit_path = format!("/tmp/autopipe_dl_{}.exit", params.filename);

        // Check if exit file exists (download finished)
        let exit_code = match self.ssh_run_timed(&format!("cat '{}' 2>/dev/null", shell_escape(&exit_path)), 10).await {
            Ok((output, 0)) if !output.trim().is_empty() => Some(output.trim().to_string()),
            _ => None,
        };

        // Get recent log output
        let recent_log = match self.ssh_run_timed(&format!("tail -20 '{}' 2>/dev/null", shell_escape(&log_path)), 10).await {
            Ok((output, _)) => output,
            Err(_) => String::new(),
        };

        match exit_code.as_deref() {
            Some("0") => {
                // Clean up temp files
                let _ = self.ssh_run_timed(&format!(
                    "rm -f '{}' '{}'", shell_escape(&log_path), shell_escape(&exit_path)
                ), 10).await;
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Download completed successfully: {}\n\nLog:\n{}",
                    params.filename, recent_log
                ))]))
            }
            Some(code) => {
                Ok(CallToolResult::error(vec![Content::text(format!(
                    "Download failed (exit code: {}).\n\nLog:\n{}",
                    code, recent_log
                ))]))
            }
            None => {
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Download in progress...\n\nRecent log:\n{}",
                    recent_log
                ))]))
            }
        }
    }

    #[tool(description = "Remove a file or symlink from the pipelines_input directory. \
Symlinks are removed directly. Regular files (including root-owned files created by Docker downloads) \
are removed via Docker to handle permission issues. relative_path is relative to pipelines_input \
(e.g. \"run-001/data.fastq\" or \"data.fastq\").")]
    async fn remove_input(
        &self,
        Parameters(params): Parameters<RemoveInputParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let input_base = self.config().full_input_dir();
        let full_path = format!(
            "{}/{}",
            input_base.trim_end_matches('/'),
            params.relative_path.trim_matches('/')
        );

        // If it's a symlink, remove it directly (symlinks are owned by the SSH user)
        match self.ssh_run(&format!("test -L '{}' && echo 'symlink'", shell_escape(&full_path))).await {
            Ok((output, 0)) if output.trim().contains("symlink") => {
                match self.ssh_run(&format!("rm '{}'", shell_escape(&full_path))).await {
                    Ok((_, 0)) => return Ok(CallToolResult::success(vec![Content::text(format!(
                        "Removed symlink: {}", full_path
                    ))])),
                    Ok((output, _)) => return Ok(CallToolResult::error(vec![Content::text(format!(
                        "Failed to remove symlink: {}", output.trim()
                    ))])),
                    Err(e) => return Ok(CallToolResult::error(vec![Content::text(e)])),
                }
            }
            _ => {}
        }

        // Regular file: try direct rm first, fall back to Docker for root-owned files
        match self.ssh_run(&format!("rm -rf '{}'", shell_escape(&full_path))).await {
            Ok((_, 0)) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Removed: {}", full_path
            ))])),
            Ok(_) => {
                // Likely root-owned — remove via Docker
                let parent = std::path::Path::new(&full_path)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| input_base.clone());
                let basename = std::path::Path::new(&full_path)
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default();
                let docker_cmd = format!(
                    "docker run --rm -v '{}:/target' alpine rm -rf '/target/{}'",
                    shell_escape(&parent),
                    shell_escape(&basename),
                );
                match self.ssh_run(&docker_cmd).await {
                    Ok((_, 0)) => Ok(CallToolResult::success(vec![Content::text(format!(
                        "Removed via Docker (root-owned): {}", full_path
                    ))])),
                    Ok((output, _)) => Ok(CallToolResult::error(vec![Content::text(format!(
                        "Failed to remove '{}': {}", full_path, output.trim()
                    ))])),
                    Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
                }
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    // ── Browser viewer ─────────────────────────────────────────

    /// Read the run marker's pipeline name (ro-crate name written at execute
    /// time) from a result directory, if present.
    async fn read_run_marker_pipeline(&self, dir: &str) -> Option<String> {
        let mp = format!("{}/.autopipe-run.json", dir.trim_end_matches('/'));
        match self.ssh_run(&format!("cat '{}' 2>/dev/null", shell_escape(&mp))).await {
            Ok((content, 0)) => serde_json::from_str::<serde_json::Value>(&clean_content(&content))
                .ok()
                .and_then(|v| v["pipeline"].as_str().map(|s| s.to_string())),
            _ => None,
        }
    }

    /// Ask the registry whether a NOT-installed pipeline viewer claims this
    /// pipeline name. Pipeline viewers declare no file extensions, so we only
    /// fetch the manifest of registry entries with an empty `extensions` list,
    /// then check their `pipeline_match.names`. Returns the viewer's name so the
    /// user can be told to install it.
    async fn registry_pipeline_viewer_for(&self, pipeline_name: &str) -> Option<String> {
        let base = self.config().registry_url;
        let base = base.trim_end_matches('/');
        let client = reqwest::Client::new();
        let list: serde_json::Value = client
            .get(format!("{}/api/plugins", base))
            .send().await.ok()?
            .json().await.ok()?;
        let arr = list.as_array()?;
        for p in arr {
            let ext_empty = p["extensions"].as_array().map(|a| a.is_empty()).unwrap_or(true);
            if !ext_empty {
                continue; // extension viewer, not a pipeline viewer
            }
            let gh = p["github_url"].as_str().unwrap_or("");
            // https://github.com/O/R/tree/REF → https://raw.githubusercontent.com/O/R/REF/manifest.json
            let raw = match gh.strip_prefix("https://github.com/") {
                Some(rest) => {
                    let parts: Vec<&str> = rest.split('/').collect();
                    if parts.len() >= 2 {
                        let reff = if parts.len() >= 4 && parts[2] == "tree" { parts[3] } else { "main" };
                        Some(format!("https://raw.githubusercontent.com/{}/{}/{}/manifest.json", parts[0], parts[1], reff))
                    } else { None }
                }
                None => None,
            };
            let Some(raw) = raw else { continue };
            if let Ok(resp) = client.get(&raw).send().await {
                if let Ok(m) = resp.json::<serde_json::Value>().await {
                    if m["plugin_type"].as_str() == Some("pipeline") {
                        if let Some(names) = m["pipeline_match"]["names"].as_array() {
                            if names.iter().any(|n| n.as_str().map(|s| s.eq_ignore_ascii_case(pipeline_name)).unwrap_or(false)) {
                                return p["name"].as_str().map(|s| s.to_string());
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Recognise a directory as a known pipeline's output and return the name of
    /// an INSTALLED pipeline-viewer plugin that claims it — matched first by the
    /// run marker's ro-crate pipeline name, then (fallback) by required-file
    /// signature for older results with no marker. None if nothing matches.
    async fn match_pipeline_viewer(&self, dir: &str, file_paths: &[String]) -> Option<String> {
        let plugins_dir = self.config().full_plugins_dir();
        // (viewer_name, rules) where each rule is (names, required_files). A dir
        // matches the viewer if ANY rule matches, so one viewer can claim several
        // folder shapes — e.g. the run folder (by marker name) AND the meme_out
        // subfolder (by the presence of meme.xml).
        let mut pipeline_plugins: Vec<(String, Vec<(Vec<String>, Vec<String>)>)> = Vec::new();
        let as_str_vec = |v: &serde_json::Value| -> Vec<String> {
            v.as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default()
        };
        if let Ok(entries) = std::fs::read_dir(&plugins_dir) {
            for entry in entries.flatten() {
                let mpath = entry.path().join("manifest.json");
                if let Ok(text) = std::fs::read_to_string(&mpath) {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                        if v["plugin_type"].as_str() != Some("pipeline") {
                            continue;
                        }
                        let vname = v["name"].as_str().unwrap_or_default().to_string();
                        if vname.is_empty() {
                            continue;
                        }
                        let mut rules: Vec<(Vec<String>, Vec<String>)> = Vec::new();
                        // New: pipeline_match.rules = [ { names?, required_files? }, ... ]
                        if let Some(rule_arr) = v["pipeline_match"]["rules"].as_array() {
                            for r in rule_arr {
                                rules.push((as_str_vec(&r["names"]), as_str_vec(&r["required_files"])));
                            }
                        }
                        // Legacy: top-level names / required_files as a single rule.
                        let top_names = as_str_vec(&v["pipeline_match"]["names"]);
                        let top_req = as_str_vec(&v["pipeline_match"]["required_files"]);
                        if !top_names.is_empty() || !top_req.is_empty() {
                            rules.push((top_names, top_req));
                        }
                        if !rules.is_empty() {
                            pipeline_plugins.push((vname, rules));
                        }
                    }
                }
            }
        }
        if pipeline_plugins.is_empty() {
            return None;
        }

        // Marker's pipeline name (ro-crate name written at execute time).
        let marker_name = self.read_run_marker_pipeline(dir).await;

        // Match required_files on TOP-LEVEL files only (dir/<name>, no further
        // '/'). file_paths is recursive so nested files can be *served*, but
        // required_files matching must stay shallow — otherwise a nested file
        // (e.g. meme_out/meme.xml inside a run folder, or any subfolder file)
        // could make an unrelated pipeline viewer claim someone else's output.
        let dir_prefix = format!("{}/", dir.trim_end_matches('/'));
        let basenames: std::collections::HashSet<String> = file_paths
            .iter()
            .filter(|p| {
                let rel = p.strip_prefix(&dir_prefix).unwrap_or(p.as_str());
                !rel.contains('/')
            })
            .map(|p| p.rsplit('/').next().unwrap_or(p).to_string())
            .collect();

        for (vname, rules) in &pipeline_plugins {
            for (names, req) in rules {
                if let Some(ref m) = marker_name {
                    if names.iter().any(|n| n.eq_ignore_ascii_case(m)) {
                        return Some(vname.clone());
                    }
                }
                if !req.is_empty() && req.iter().all(|f| basenames.contains(f)) {
                    return Some(vname.clone());
                }
            }
        }
        None
    }

    #[tool(description = "Open the Results Viewer in a browser for inspecting result files. Call this whenever the user wants a richer view of result files (images, plots, genomic tracks, large tables, binary formats) — there is no required hand-off from list_files; you can also call it proactively after summarising results in chat if it would help the user. Pass a DIRECTORY path to view all files in it, or a single FILE path to view only that file. File formats are handled by viewer plugins (auto-routed by file extension): defaults include images, PDF, text, CSV, FASTA/FASTQ, BAM/BED/GFF/CRAM/VCF/BCF, and HDF5 (h5ad). The viewer matches each file's extension against locally-installed plugins automatically — the user does NOT need to choose a plugin. If the user asks to view a format that no installed plugin handles, tell them to (a) open the AutoPipe desktop app and click the Plugins button to search the registry for a matching plugin, or (b) if no plugin exists, write their own following the Plugin development guide at https://autopipe.org/plugins/guide. Do NOT attempt to install plugins yourself — plugin installation is GUI-only. When the user asks to view a specific file, pass the exact file path — do NOT pass the parent directory. The viewer includes a sidebar file browser: from the opened file (or directory) the user can navigate the output directory tree — including parent folders, up to the run's output directory — and open other files on demand, each loaded lazily. So passing a single file is enough; the user can explore sibling and nearby files themselves without you calling show_results again. IMPORTANT workflow for genomics files that need a reference (BAM/BED/GFF/CRAM): (1) First call show_results WITHOUT the reference parameter. The viewer will NOT open yet — instead you will receive information about FASTA files in the directory. (2) Ask the user about the reference based on the response. (3) Then call show_results AGAIN: with reference=<fasta_filename> if the user confirmed, with reference=<user_provided_path> if they gave a different path, or with reference=\"none\" if the user has no reference. The viewer only opens on this second call. Without reference, only Data tabs are shown. With reference, both Data and IGV tabs appear. CRAM files cannot be displayed without a reference. VCF and BCF files do NOT require the reference workflow — they open directly with data tables. PIPELINE OUTPUT — ALWAYS ASK FIRST: when a folder is a pipeline's result folder (a pipeline viewer claims it — this applies to BOTH the sorting-result run folder AND a meme_out subfolder), this tool does NOT open immediately; it returns a message telling you to ask the user first. You MUST ask the user (dedicated combined viewer vs. opening files one by one), then re-call show_results on the SAME path with pipeline_viewer=\"yes\" or \"no\". Never open a pipeline result silently — this holds for sorting results and MEME results alike. MEME RESULTS — ASK SCOPE: to show motif analysis, ask the user whether they want it together with the sorting results (open the RUN folder → Sequences + Motif tabs) or MEME only (open the meme_out folder → a Motif-only view), then call show_results on the folder they chose.")]
    async fn show_results(
        &self,
        Parameters(params): Parameters<ShowResultsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Translate Windows paths to WSL form. The path is used by many SSH
        // commands and embedded in error messages below; binding it to a local
        // variable keeps every downstream usage consistent.
        let path = windows_to_wsl(&params.path);
        // Check if path is a directory or file
        let is_dir = match self
            .ssh_run(&format!(
                "test -d '{}' && echo DIR || echo FILE",
                shell_escape(&path)
            ))
            .await
        {
            Ok((output, 0)) => clean_content(&output).trim() == "DIR",
            _ => false,
        };

        let file_paths: Vec<String> = if is_dir {
            // List files in directory (non-recursive, max 50)
            match self
                .ssh_run(&format!(
                    "find '{}' -type f ! -path '*/.snakemake/*' ! -name '.*' ! -name 'Dockerfile' ! -name 'Snakefile*' ! -name '*.py' ! -name '*.sh' | head -200",
                    shell_escape(&path)
                ))
                .await
            {
                Ok((output, 0)) => clean_content(&output)
                    .trim()
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(|l| l.to_string())
                    .collect(),
                Ok((output, _)) => {
                    return Ok(CallToolResult::error(vec![Content::text(format!(
                        "Cannot list directory '{}': {}",
                        path,
                        clean_content(&output).trim()
                    ))]));
                }
                Err(e) => return Ok(CallToolResult::error(vec![Content::text(e)])),
            }
        } else {
            // Single file: check it exists, then load only that file
            match self
                .ssh_run(&format!(
                    "test -f '{}' && echo OK || echo NOT_FOUND",
                    shell_escape(&path)
                ))
                .await
            {
                Ok((output, 0)) if clean_content(&output).trim() == "OK" => {
                    vec![path.clone()]
                }
                _ => {
                    return Ok(CallToolResult::error(vec![Content::text(format!(
                        "File not found: {}",
                        path
                    ))]));
                }
            }
        };

        // Apply file type filter if specified
        let file_paths: Vec<String> = if let Some(ref filter) = params.filter {
            let allowed_exts: Vec<&str> = match filter.to_lowercase().as_str() {
                "image" | "images" => vec!["png", "jpg", "jpeg", "gif", "svg", "webp", "bmp", "tiff", "tif"],
                "text" => vec!["txt", "log", "csv", "tsv", "json", "yaml", "yml", "xml", "md"],
                "genomics" => vec!["bam", "cram", "vcf", "bcf", "bed", "gff", "gtf", "gff3", "fasta", "fa", "fastq", "fq", "bigwig", "bw", "bigbed", "bb"],
                "pdf" => vec!["pdf"],
                "hdf5" => vec!["h5ad", "h5", "hdf5"],
                _ => vec![],
            };
            if allowed_exts.is_empty() {
                file_paths
            } else {
                file_paths
                    .into_iter()
                    .filter(|p| {
                        let ext = p.rsplit('.').next().map(|e| e.to_lowercase()).unwrap_or_default();
                        allowed_exts.contains(&ext.as_str())
                    })
                    .collect()
            }
        } else {
            file_paths
        };

        // A directory holding only subdirectories — a run's output folder, say —
        // has nothing at depth 1, and erroring here made it unopenable even
        // though the sidebar exists precisely to browse such a tree. Fall
        // through instead: with no files collected this lands in list-only
        // mode, which opens an empty pane plus the browser. An explicit filter
        // that matched nothing is still a real miss and stays an error.
        if file_paths.is_empty() && (!is_dir || params.filter.is_some()) {
            let filter_msg = params.filter.as_deref().unwrap_or("any");
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "No {} files found in '{}'",
                filter_msg, path
            ))]));
        }

        // ── Pipeline-viewer routing (consent-gated) ──────────────────────
        // If this directory is a known pipeline's output and a matching pipeline
        // viewer is installed, don't open yet — ask the user first. On the second
        // call pipeline_viewer="yes" routes the whole folder to that viewer;
        // "no" falls through to ordinary per-extension routing.
        let mut pipeline_viewer_active: Option<String> = None;
        if is_dir && params.filter.is_none() {
            if let Some(pv_name) = self.match_pipeline_viewer(&path, &file_paths).await {
                // An installed pipeline viewer claims this directory — confirm first.
                match params.pipeline_viewer.as_deref() {
                    Some("yes") | Some("Yes") | Some("true") => {
                        pipeline_viewer_active = Some(pv_name);
                    }
                    Some("no") | Some("No") | Some("false") => {}
                    _ => {
                        return Ok(CallToolResult::success(vec![Content::text(format!(
                            "[Pipeline viewer] This directory looks like output that the '{pv}' viewer handles. \
                             Ask the user whether to open it with the dedicated {pv} viewer (one combined dashboard) \
                             instead of opening each file by type.\n\n\
                             Then call show_results AGAIN on the SAME path with pipeline_viewer=\"yes\" to use the {pv} \
                             viewer, or pipeline_viewer=\"no\" to open files by extension as usual.",
                            pv = pv_name
                        ))]));
                    }
                }
            } else if !matches!(params.pipeline_viewer.as_deref(), Some("no") | Some("No") | Some("false")) {
                // No matching viewer is installed. If the registry has a pipeline
                // viewer for this run's pipeline, tell the user to install it.
                if let Some(mname) = self.read_run_marker_pipeline(&path).await {
                    if let Some(reg_viewer) = self.registry_pipeline_viewer_for(&mname).await {
                        return Ok(CallToolResult::success(vec![Content::text(format!(
                            "[Pipeline viewer available] This is output from the '{pipe}' pipeline, which has a dedicated \
                             viewer ('{viewer}') in the AutoPipe registry — but it is NOT installed locally.\n\n\
                             Tell the user: to see this result as a combined dashboard, open the AutoPipe desktop app, \
                             click the Plugins button, search for '{viewer}', install it, then reopen this folder. \
                             Or call show_results again with pipeline_viewer=\"no\" to open the files by type for now.",
                            pipe = mname, viewer = reg_viewer
                        ))]));
                    }
                }
            }
        }

        // Separate genomics files (remote, server-side pagination) from other files (local transfer)
        let genomics_remote_exts = ["bam", "vcf", "bed", "gff", "gtf", "gff3", "cram", "bcf",
                                     "vcf.gz", "bed.gz", "gff.gz", "gff3.gz", "gtf.gz",
                                     "fasta.gz", "fa.gz", "fastq.gz", "fq.gz",
                                     "csv.gz", "tsv.gz", "txt.gz", "log.gz",
                                     "fasta", "fa", "fastq", "fq",
                                     "csv", "tsv", "tab",
                                     "txt", "log", "json", "yaml", "yml", "xml", "md",
                                     "sh", "py", "r", "nf", "smk", "cfg", "ini", "toml",
                                     "bai", "crai", "tbi", "csi", "fai", "idx"];
        // Detect genomics files early (extensions only, no SSH) so we can
        // decide whether to skip eager per-file loading.
        let genomics_exts = ["bam", "bed", "gff", "gtf", "gff3", "cram"];
        let has_genomics = file_paths.iter().any(|p| {
            let ext = p.rsplit('.').next().map(|e| e.to_lowercase()).unwrap_or_default();
            genomics_exts.contains(&ext.as_str())
        });
        // Directory list-only mode: a whole directory was opened and it has no
        // genomics files that need a reference. Skip eager per-file loading —
        // the viewer's browse sidebar lists the directory and loads each file
        // lazily on click, so we open with an empty snapshot and an empty pane.
        let list_only = is_dir && !has_genomics;

        let mut files: Vec<(String, Vec<u8>, String)> = Vec::new();
        let mut remote_files: Vec<(String, String, u64, String, bool)> = Vec::new(); // (filename, remote_path, size, mime, listed)
        let mut errors: Vec<String> = Vec::new();

        // Pipeline viewer: register every file in the directory as a streamable
        // remote file (served raw via /file/), so the viewer reads them itself.
        // This bypasses per-file extension handling and the has_viewer filter.
        if pipeline_viewer_active.is_some() {
            let dir_prefix = format!("{}/", path.trim_end_matches('/'));
            for p in &file_paths {
                // Register with a path RELATIVE to the opened dir so nested files
                // (e.g. meme_out/meme.xml) are served at /file/meme_out/meme.xml,
                // which is exactly what the pipeline viewer requests.
                let filename = p.strip_prefix(&dir_prefix).unwrap_or(p).to_string();
                let size_cmd = format!(
                    "stat -c%s '{}' 2>/dev/null || stat -f%z '{}' 2>/dev/null",
                    shell_escape(p), shell_escape(p)
                );
                let size: u64 = self.ssh_run(&size_cmd).await.ok()
                    .and_then(|(s, _)| clean_content(&s).trim().parse().ok())
                    .unwrap_or(0);
                // Serve with the right Content-Type so the browser renders
                // meme.html as a page (not raw source) and images inline.
                let ext = p.rsplit('.').next().map(|e| e.to_lowercase()).unwrap_or_default();
                let mime = match ext.as_str() {
                    "html" | "htm" => "text/html",
                    "xml" => "text/xml",
                    "svg" => "image/svg+xml",
                    "png" => "image/png",
                    "jpg" | "jpeg" => "image/jpeg",
                    "gif" => "image/gif",
                    "pdf" => "application/pdf",
                    "json" => "application/json",
                    "csv" | "tsv" => "text/csv",
                    "eps" | "ps" => "application/postscript",
                    _ => "text/plain",
                };
                remote_files.push((filename, p.clone(), size, mime.to_string(), true));
            }
        }

        if !list_only && pipeline_viewer_active.is_none() {
        for path in &file_paths {
            let ext = path
                .rsplit('.')
                .next()
                .map(|e| e.to_lowercase())
                .unwrap_or_default();
            let mime = match ext.as_str() {
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "gif" => "image/gif",
                "svg" => "image/svg+xml",
                "webp" => "image/webp",
                "bmp" => "image/bmp",
                "tiff" | "tif" => "image/tiff",
                "pdf" => "application/pdf",
                "txt" | "log" | "md" | "sh" | "py" | "r" | "nf" | "smk" | "cfg" | "ini" | "toml" => "text/plain",
                "csv" | "tsv" => "text/csv",
                "json" => "application/json",
                "yaml" | "yml" => "text/yaml",
                "xml" => "text/xml",
                "html" | "htm" => "text/html",
                "fastq" | "fq" | "fasta" | "fa" => "text/plain",
                _ => "application/octet-stream",
            };

            let filename = std::path::Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string();

            // Genomics files: register as remote (server-side pagination + Range proxy).
            // Match on the compound-aware extension so bgzipped text (.vcf.gz etc.)
            // is served through /file/ for the plugin to stream, not treated as an
            // opaque .gz.
            let route_ext = viewer_ext(path);
            if genomics_remote_exts.contains(&ext.as_str())
                || genomics_remote_exts.contains(&route_ext.as_str()) {
                let size_cmd = format!("stat -c%s '{}' 2>/dev/null || stat -f%z '{}' 2>/dev/null", shell_escape(path), shell_escape(path));
                let size: u64 = if let Ok((size_str, 0)) = self.ssh_run(&size_cmd).await {
                    clean_content(&size_str).trim().parse().unwrap_or(0)
                } else {
                    0
                };
                remote_files.push((filename, path.clone(), size, mime.to_string(), true));

                // Pull in the file's sidecar index (.bai/.crai/.tbi/.csi/.fai)
                // explicitly, the same way the sidebar-open path does. Relying
                // on the index merely happening to appear in the depth-1 scan
                // breaks when it lives in a subfolder or the listing is capped.
                for (idx_name, idx_path, idx_size) in
                    viewer::find_sidecar_indexes(&self.config(), path, &route_ext).await
                {
                    if remote_files.iter().any(|(n, _, _, _, _)| *n == idx_name) {
                        continue;
                    }
                    remote_files.push((
                        idx_name,
                        idx_path,
                        idx_size,
                        "application/octet-stream".to_string(),
                        false, // served but hidden from the file list
                    ));
                }
                continue;
            }

            // h5ad/h5/hdf5: check file size, skip if > 1GB (download only)
            if matches!(ext.as_str(), "h5ad" | "h5" | "hdf5") {
                let size_cmd = format!("stat -c%s '{}' 2>/dev/null || stat -f%z '{}' 2>/dev/null", shell_escape(path), shell_escape(path));
                if let Ok((size_str, 0)) = self.ssh_run(&size_cmd).await {
                    let size: u64 = clean_content(&size_str).trim().parse().unwrap_or(0);
                    if size > 1_073_741_824 {
                        errors.push(format!("{}: file too large ({:.1} GB) — download only", filename, size as f64 / 1_073_741_824.0));
                        continue;
                    }
                }
            }

            match self
                .ssh_run(&format!("base64 -w 0 '{}'", shell_escape(path)))
                .await
            {
                Ok((b64, 0)) => {
                    let trimmed = clean_content(&b64).trim().to_string();
                    match base64::Engine::decode(
                        &base64::engine::general_purpose::STANDARD,
                        &trimmed,
                    ) {
                        Ok(data) => files.push((filename, data, mime.to_string())),
                        Err(e) => errors.push(format!("{}: decode error: {}", filename, e)),
                    }
                }
                Ok((output, _)) => {
                    errors.push(format!("{}: {}", filename, clean_content(&output).trim()));
                }
                Err(e) => errors.push(format!("{}: {}", filename, e)),
            }
        }

        if files.is_empty() && remote_files.is_empty() {
            let msg = if errors.is_empty() {
                format!("No viewable files found in '{}'", path)
            } else {
                format!("Failed to load files:\n{}", errors.join("\n"))
            };
            return Ok(CallToolResult::error(vec![Content::text(msg)]));
        }
        } // end if !list_only

        // "none" means user explicitly said no reference → open viewer without IGV
        let user_declined_ref = matches!(
            params.reference.as_deref(),
            Some("none") | Some("None") | Some("no")
        );
        let reference = if user_declined_ref {
            None
        } else {
            // The reference may be a bare filename (e.g. "ref.fasta") or a
            // full path. windows_to_wsl is a no-op when there is no drive
            // letter prefix, so it is safe to apply unconditionally.
            params.reference.as_deref().map(windows_to_wsl)
        };

        // Check if any file requires a plugin that is not installed
        let plugins_dir_path = self.config().full_plugins_dir();
        let installed_plugin_exts: Vec<String> = {
            let mut exts = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&plugins_dir_path) {
                for entry in entries.flatten() {
                    let manifest_path = entry.path().join("manifest.json");
                    if let Ok(text) = std::fs::read_to_string(&manifest_path) {
                        if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&text) {
                            if let Some(arr) = manifest["extensions"].as_array() {
                                for e in arr {
                                    if let Some(s) = e.as_str() {
                                        exts.push(s.to_lowercase());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            exts
        };
        // Index file extensions — kept in remote_files for IGV/plugin access, but hidden from viewer list
        let index_exts = ["bai", "tbi", "csi", "crai", "fai", "idx"];
        let has_viewer = |name: &str| -> bool {
            let ext = viewer_ext(name);
            ext.is_empty() || installed_plugin_exts.contains(&ext)
        };
        let is_index_file = |name: &str| -> bool {
            let ext = name.rsplit('.').next().map(|e| e.to_lowercase()).unwrap_or_default();
            index_exts.contains(&ext.as_str())
        };
        {
            let mut skipped: Vec<String> = file_paths.iter().filter_map(|p| {
                if has_viewer(p) || is_index_file(p) { None } else {
                    Some(p.rsplit('.').next().unwrap_or("").to_lowercase())
                }
            }).collect();
            skipped.sort();
            skipped.dedup();
            if !skipped.is_empty() {
                let ext_list = skipped.iter().map(|e| format!(".{}", e)).collect::<Vec<_>>().join(", ");
                errors.push(format!("Skipped files with no viewer plugin: {}", ext_list));
            }
        }
        // A pipeline viewer consumes the whole directory, so keep every file it
        // registered instead of filtering by extension.
        if pipeline_viewer_active.is_none() {
            files.retain(|(name, _, _)| has_viewer(name));
            // Keep index files in remote_files (accessible via /file/ endpoint) but filter non-viewable, non-index files
            remote_files.retain(|(name, _, _, _, _)| has_viewer(name) || is_index_file(name));
        }
        // In list-only mode files/remote_files are intentionally empty (the
        // browse sidebar loads each file lazily on click), so this emptiness
        // guard must NOT fire — otherwise opening a directory fails with
        // "No viewable files found".
        if !list_only && files.is_empty() && remote_files.is_empty() {
            let msg = if errors.is_empty() {
                format!("No viewable files found in '{}'", path)
            } else {
                format!("No viewable files found:\n{}", errors.join("\n"))
            };
            return Ok(CallToolResult::error(vec![Content::text(msg)]));
        }

        // The reference question has to consider the whole tree, not just the
        // top level: opening a run's output directory lists nothing at depth 1,
        // yet the sidebar can open BAM/CRAM files sitting in align/ or similar.
        // Without this the viewer opens with no reference and the IGV tab never
        // appears. `list_only` deliberately still keys off the depth-1 scan so
        // eager loading behaviour is unchanged.
        let (deep_genomics, deep_fastas): (bool, Vec<String>) = if is_dir && !has_genomics {
            let cmd = format!(
                "find '{}' -maxdepth 3 -type f \\( -name '*.bam' -o -name '*.cram' -o -name '*.bed' \
                 -o -name '*.gff' -o -name '*.gtf' -o -name '*.gff3' -o -name '*.fa' \
                 -o -name '*.fasta' -o -name '*.fna' \\) 2>/dev/null | head -100",
                shell_escape(&path)
            );
            match self.ssh_run(&cmd).await {
                Ok((out, _)) => {
                    let paths: Vec<String> = clean_content(&out)
                        .lines()
                        .filter(|l| !l.trim().is_empty())
                        .map(|l| l.trim().to_string())
                        .collect();
                    let is_ref = |p: &String| {
                        let e = p.rsplit('.').next().map(|e| e.to_lowercase()).unwrap_or_default();
                        matches!(e.as_str(), "fa" | "fasta" | "fna")
                    };
                    let has_data = paths.iter().any(|p| !is_ref(p));
                    let fastas = paths.into_iter().filter(is_ref).collect();
                    (has_data, fastas)
                }
                Err(_) => (false, Vec::new()),
            }
        } else {
            (false, Vec::new())
        };

        // --- Genomics files exist but no reference decision yet → ask first, don't open viewer ---
        if (has_genomics || deep_genomics) && reference.is_none() && !user_declined_ref {
            let mut fasta_files: Vec<&String> = file_paths.iter().filter(|p| {
                let ext = p.rsplit('.').next().map(|e| e.to_lowercase()).unwrap_or_default();
                matches!(ext.as_str(), "fasta" | "fa" | "fna")
            }).collect();
            // Suggest references found deeper in the tree as well, so the user
            // can answer with e.g. ref/reference.fa.
            fasta_files.extend(deep_fastas.iter());

            let igv_only_files: Vec<String> = file_paths.iter().filter(|p| {
                let ext = p.rsplit('.').next().map(|e| e.to_lowercase()).unwrap_or_default();
                matches!(ext.as_str(), "cram")
            }).map(|p| {
                std::path::Path::new(p.as_str())
                    .file_name().and_then(|n| n.to_str()).unwrap_or("unknown").to_string()
            }).collect();

            let mut msg = String::from("[Reference check] Genomics files detected. The viewer is NOT opened yet.\n");

            if !fasta_files.is_empty() {
                // A FASTA from a subdirectory has to be offered by full path —
                // its bare filename would not identify it on the way back in.
                let fasta_names: Vec<String> = fasta_files.iter().map(|p| {
                    if deep_fastas.iter().any(|d| d == *p) {
                        (*p).clone()
                    } else {
                        std::path::Path::new(p.as_str())
                            .file_name().and_then(|n| n.to_str()).unwrap_or("unknown").to_string()
                    }
                }).collect();

                msg.push_str(&format!(
                    "\nFASTA file(s) found in the directory: {}\n\
                     Ask the user: 'Is [{}] the correct reference for your data? If not, please provide the correct reference file path.'\n\
                     - If yes: call show_results again with reference=\"{}\"\n\
                     - If user provides a different path: call show_results again with reference=<that_path>\n\
                     - If user has no reference: call show_results again with reference=\"none\"",
                    fasta_names.join(", "),
                    fasta_names.join(", "),
                    fasta_names[0],
                ));
            } else {
                msg.push_str(
                    "\nNo FASTA reference file found in the directory.\n\
                     Ask the user: 'Do you have a reference FASTA file for this data? If so, please provide the file path.'\n\
                     - If user provides a path: call show_results again with reference=<that_path>\n\
                     - If user has no reference: call show_results again with reference=\"none\""
                );
            }

            if !igv_only_files.is_empty() {
                msg.push_str(&format!(
                    "\n\nNote: {} require a reference and cannot be viewed without one.",
                    igv_only_files.join(", ")
                ));
            }

            return Ok(CallToolResult::success(vec![Content::text(msg)]));
        }

        // --- Reference confirmed / declined / no genomics → open viewer ---
        let total_files = files.len() + remote_files.len();

        // Compute the browse root — the directory the viewer may navigate
        // within — and the initial directory the sidebar opens at. This is a
        // pure string computation (NO SSH) so show_results stays fast; the
        // viewer canonicalizes the root lazily on the first browse request,
        // which is where symlink/`..` escapes are actually enforced.
        //
        // browse_root caps navigation at the run's output directory:
        //  - the whole configured output dir was opened → that dir (all runs)
        //  - a path under it → output_dir/<run_name>
        //  - a path outside it → "<...>/outputs/<run>" when the
        //    `outputs`/`output` convention is found (so a result file opened
        //    from elsewhere still gets its run dir as the cap), otherwise the
        //    directory itself or, for a single file, its parent.
        let output_dir = self.config().full_output_dir();
        let out_trimmed = output_dir.trim_end_matches('/').to_string();
        let browse_root: Option<String> = if path == out_trimmed {
            Some(out_trimmed.clone())
        } else if path.starts_with(&format!("{}/", out_trimmed)) {
            let rest = &path[out_trimmed.len() + 1..];
            let run_name = rest.split('/').next().unwrap_or("");
            if run_name.is_empty() {
                Some(out_trimmed.clone())
            } else {
                Some(format!("{}/{}", out_trimmed, run_name))
            }
        } else {
            // Locate an `outputs`/`output` segment and treat its child as the
            // run directory; fall back to the path itself / its parent.
            let segs: Vec<&str> = path.split('/').collect();
            let conv = segs
                .iter()
                .position(|s| *s == "outputs" || *s == "output")
                .filter(|&i| i + 1 < segs.len())
                .map(|i| segs[..=i + 1].join("/"));
            conv.or_else(|| {
                if is_dir {
                    Some(path.clone())
                } else {
                    std::path::Path::new(&path)
                        .parent()
                        .and_then(|p| p.to_str())
                        .map(|s| s.to_string())
                }
            })
        };

        // The sidebar opens at the opened file's own folder (single file) so
        // its siblings show immediately; for a directory it opens at the root.
        let initial_dir: Option<String> = if is_dir {
            None
        } else {
            std::path::Path::new(&path)
                .parent()
                .and_then(|p| p.to_str())
                .map(|s| s.to_string())
        };

        // The reference may be a genome ID, a filename among the collected
        // files, or a path anywhere on the server — the workflow above invites
        // the user to answer with one. Only the first two cases are servable so
        // far: /file/{name} resolves against the registered map, so a reference
        // living outside the opened directory has to be registered here or IGV
        // gets a 404. Its .fai comes along, without which igv.js falls back to
        // pulling the entire FASTA.
        if let Some(ref reference_value) = reference {
            const GENOME_IDS: &[&str] = &[
                "hg38", "hg19", "mm39", "mm10", "rn7", "rn6", "dm6", "ce11",
                "danRer11", "sacCer3", "tair10", "galGal6",
            ];
            let basename = std::path::Path::new(reference_value)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(reference_value.as_str())
                .to_string();

            let already_registered = remote_files.iter().any(|(n, _, _, _, _)| *n == basename)
                || files.iter().any(|(n, _, _)| *n == basename);

            if !GENOME_IDS.contains(&reference_value.as_str()) && !already_registered {
                // Resolve the answer against the opened directory so a relative
                // reply like "ref/reference.fa" works as well as a full path.
                let candidates = if reference_value.starts_with('/') {
                    vec![reference_value.clone()]
                } else {
                    let base = if is_dir { path.clone() } else {
                        std::path::Path::new(&path).parent()
                            .and_then(|p| p.to_str()).unwrap_or(".").to_string()
                    };
                    vec![
                        format!("{}/{}", base.trim_end_matches('/'), reference_value),
                        reference_value.clone(),
                    ]
                };

                for cand in candidates {
                    let esc = shell_escape(&cand);
                    let cmd = format!(
                        "for f in '{esc}' '{esc}.fai'; do [ -f \"$f\" ] && \
                         printf '%s\\t%s\\n' \"$f\" \
                         \"$(stat -c%s \"$f\" 2>/dev/null || stat -f%z \"$f\" 2>/dev/null)\"; done",
                        esc = esc
                    );
                    let out = match self.ssh_run(&cmd).await {
                        Ok((o, _)) => clean_content(&o),
                        Err(_) => continue,
                    };
                    let mut found_any = false;
                    for line in out.lines() {
                        if let Some((p, s)) = line.trim().split_once('\t') {
                            let name = std::path::Path::new(p)
                                .file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                            if name.is_empty() { continue; }
                            found_any = true;
                            // Served but kept out of the file list: it supports
                            // the alignment view rather than being a result.
                            remote_files.push((
                                name,
                                p.to_string(),
                                s.trim().parse().unwrap_or(0),
                                "text/plain".to_string(),
                                false,
                            ));
                        }
                    }
                    if found_any { break; }
                }
            }
        }

        // A pipeline-routed view is never list-only, even when the directory has
        // no genomics files: the viewer renders the whole folder as a dashboard.
        let list_only = list_only && pipeline_viewer_active.is_none();

        match viewer::show_files(
            files.clone(),
            remote_files.clone(),
            self.config().full_plugins_dir(),
            reference,
            Some(self.config()),
            browse_root,
            initial_dir,
            pipeline_viewer_active.clone(),
        ).await {
            Ok(url) => {
                if let Some(ref pv) = pipeline_viewer_active {
                    let msg = format!(
                        "Opened the '{}' pipeline viewer in your browser: {}\n\nThe whole result folder is shown as one dashboard. The sidebar still lists individual files if you want to open one directly.",
                        pv, url
                    );
                    return Ok(CallToolResult::success(vec![Content::text(msg)]));
                }
                if list_only {
                    // Directory opened in list-only mode: nothing is preloaded.
                    // The sidebar shows the directory; the user clicks a file to
                    // view it. Tell the user to pick a file from the browser.
                    let msg = format!(
                        "Opened the output directory browser in your browser: {}\n\nThe sidebar lists the files in this directory. The viewer pane is empty until you click a file — each file is loaded on demand when selected.",
                        url
                    );
                    return Ok(CallToolResult::success(vec![Content::text(msg)]));
                }
                let mut msg = format!(
                    "Opened results in browser: {}\n\nDisplaying {} file(s):",
                    url,
                    total_files
                );
                for (name, data, _) in &files {
                    msg.push_str(&format!("\n  {} ({} bytes)", name, data.len()));
                }
                for (name, _, size, _, listed) in &remote_files {
                    // Supporting files (indexes, an out-of-tree reference) are
                    // served but not shown, so do not announce them either.
                    if !listed {
                        continue;
                    }
                    msg.push_str(&format!("\n  {} ({} bytes, server-side)", name, size));
                }
                if !errors.is_empty() {
                    msg.push_str(&format!(
                        "\n\nSkipped {} file(s):\n{}",
                        errors.len(),
                        errors.join("\n")
                    ));
                }
                Ok(CallToolResult::success(vec![Content::text(msg)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to open browser: {}",
                e
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
             === ro-crate-metadata.json Template ===\n{}",
            templates::SNAKEFILE_TEMPLATE,
            templates::DOCKERFILE_TEMPLATE,
            templates::CONFIG_YAML_TEMPLATE,
            templates::RO_CRATE_METADATA_TEMPLATE,
        );
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(description = "Get the pipeline generation guide with rules for Snakefiles and Dockerfiles")]
    async fn get_generation_guide(&self) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::success(vec![Content::text(
            templates::GENERATION_GUIDE,
        )]))
    }

    // ── Plugin management tools ─────────────────────────────────

    #[tool(description = "List all locally installed viewer plugins. Shows each plugin's name, version, description, and supported file extensions.")]
    async fn list_installed_plugins(&self) -> Result<CallToolResult, ErrorData> {
        let plugins_dir = self.config().full_plugins_dir();
        let dir = std::path::Path::new(&plugins_dir);

        if !dir.is_dir() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "No plugins installed.\nPlugins directory: {}",
                plugins_dir
            ))]));
        }

        let mut plugins: Vec<serde_json::Value> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let manifest_path = path.join("manifest.json");
                if let Ok(content) = std::fs::read_to_string(&manifest_path) {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
                        plugins.push(v);
                    }
                }
            }
        }

        if plugins.is_empty() {
            Ok(CallToolResult::success(vec![Content::text(format!(
                "No plugins installed.\nPlugins directory: {}",
                plugins_dir
            ))]))
        } else {
            let mut msg = format!("Installed plugins ({}):\n", plugins.len());
            for p in &plugins {
                let name = p["name"].as_str().unwrap_or("unknown");
                let ver = p["version"].as_str().unwrap_or("?");
                let desc = p["description"].as_str().unwrap_or("");
                let exts: Vec<&str> = p["extensions"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
                    .unwrap_or_default();
                msg.push_str(&format!(
                    "\n  {} v{}\n    {}\n    Extensions: {}\n",
                    name, ver, desc, exts.join(", ")
                ));
            }
            msg.push_str(&format!("\nPlugins directory: {}", plugins_dir));
            Ok(CallToolResult::success(vec![Content::text(msg)]))
        }
    }

    #[tool(description = "Open the local plugins directory in the OS file explorer. Also shows a link to the plugin creation guide.")]
    async fn open_plugin_dir(&self) -> Result<CallToolResult, ErrorData> {
        let config = self.config();
        let plugins_dir = config.full_plugins_dir();

        // Create directory if it doesn't exist
        if let Err(e) = std::fs::create_dir_all(&plugins_dir) {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to create plugins directory '{}': {}",
                plugins_dir, e
            ))]));
        }

        match open::that(&plugins_dir) {
            Ok(_) => {
                let base = config.registry_url.trim_end_matches('/');
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Opened plugins directory: {}\n\n\
                     To create a new plugin, see the guide:\n\
                     {}/plugins/guide",
                    plugins_dir, base
                ))]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to open directory '{}': {}",
                plugins_dir, e
            ))])),
        }
    }
}

#[tool_handler]
impl ServerHandler for AutoPipeServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            instructions: Some(
                "AutoPipe MCP Server — Bioinformatics pipeline management.\n\
                 All file operations are on the remote SSH server.\n\
                 Use get_workspace_info first to see configured paths.\n\
                 Use create_symlink instead of cp for data files.\n\
                 Pipeline outputs are stored under the configured output directory.\n\
                 Use list_files and read_file to view results from the output path.\n\n\
                 DATA PRIVACY (IMPORTANT):\n\
                 NEVER load the user's raw input data into the conversation. To let the user view input or output data, use show_results — it streams to a local viewer in the browser, not into the chat. Files under the input directory are gated: read_file will not return their contents until the user explicitly consents (then re-call read_file with confirm_read=true); if the user declines, ask them for the specific values you need (column names, group labels, etc.) instead of reading the file. Stage user-provided data with prepare_input first — do not read raw data at its original external path directly. Large or binary files are never returned by read_file; view them with show_results.\n\n\
                 PATH HANDLING (IMPORTANT for Windows users):\n\
                 If the user provides a Windows-style path (e.g. C:\\Users\\me\\data\\input.fastq \
                 or C:/Users/me/data), pass it through to AutoPipe AS-IS. Do NOT ask the user to \
                 convert it to a remote/WSL path or to upload the file manually. AutoPipe \
                 automatically rewrites Windows paths to their WSL equivalent (C:\\Users\\me → \
                 /mnt/c/Users/me) before running any SSH command, which works whenever the SSH \
                 server is the user's local WSL distribution. Only ask for a different path if \
                 AutoPipe returns an explicit error that the translated path does not exist.\n\n\
                 PIPELINE CREATION (IMPORTANT):\n\
                 When the user wants to create a pipeline, NEVER write code directly.\n\
                 ALWAYS follow this workflow:\n\
                 1. Call get_generation_guide to learn the pipeline format rules.\n\
                 2. Call get_templates to get the Snakefile, Dockerfile, config.yaml, and ro-crate-metadata.json templates.\n\
                 3. Generate pipeline files based on the templates and guide.\n\
                 4. Use write_file to write each generated file to the pipeline directory on the SSH server.\n\
                 5. Use validate_pipeline to verify the structure.\n\
                 All pipelines MUST follow the AutoPipe format: Snakefile + Dockerfile + config.yaml + ro-crate-metadata.json.\n\
                 NEVER use Docker commands (docker run, docker pull) directly inside Snakefile rules. The only exception is nextflow run with -profile docker.\n\
                 If the user has an existing Dockerfile from their analysis environment, use it as the base.\n\
                 CRITICAL: NEVER call upload_pipeline or publish_pipeline unless the user EXPLICITLY says 'upload', 'publish', or 'register'. Pipeline creation, building, and execution do NOT require uploading or publishing. These are completely separate actions that require explicit user request.\n\n\
                 UPLOAD & PUBLISH RULES:\n\
                 When the user asks to upload OR publish, ALWAYS do these steps in order:\n\
                 1. Call validate_pipeline first. If it returns errors, fix the underlying files (e.g., fill in empty fields, create missing files, add `rule all` to the Snakefile) and re-run validate_pipeline until it passes. This step is REQUIRED even when the user says 'upload to GitHub only' — broken or empty pipeline files must not reach the public GitHub repository.\n\
                 2. Call upload_pipeline (version is auto-detected from GitHub, do NOT pass a version).\n\
                 3. If the user said 'upload to GitHub only', stop here. Otherwise, immediately call publish_pipeline with the returned github_url.\n\
                 Duplicate names and version upgrades are handled automatically by the tools."
                    .into(),
            ),
            ..Default::default()
        }
    }
}

// ── stdio transport ──────────────────────────────────────────────────────
//
// Used by clients whose JSON config only accepts stdio entries (notably
// Claude Desktop). The client spawns the autopipe binary as a child
// process and communicates over its stdin/stdout. Each spawn gets a fresh
// AutoPipeServer that loads config and SSH state independently — it does
// not share memory with the GUI tray app's HTTP server.

/// Run the MCP server in stdio mode. Returns when the client closes the
/// connection.
pub async fn run_mcp_stdio_server() -> Result<(), Box<dyn std::error::Error>> {
    let config = AppConfig::load();
    let server = AutoPipeServer::new(config);
    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}

// ── HTTP transport ───────────────────────────────────────────────────────
//
// The MCP server is exposed over local HTTP (Streamable HTTP transport),
// bound to 127.0.0.1 only. Clients (Claude Desktop, Claude Code, Cursor,
// VS Code, Gemini CLI, Codex CLI, ...) connect over loopback.
//
// Security:
//   - Bearer token check on every request (Authorization header)
//   - Origin header must be loopback (127.0.0.1 / localhost) or absent
//     (DNS-rebinding mitigation)
//
// Per-connection logging:
//   - Each request is tagged with a connection UUID in tracing spans so logs
//     from multiple AI clients can be separated by `grep conn=<uuid>`.

/// Try to bind 127.0.0.1:`preferred`, falling back to nearby ports
/// (`preferred`+1 .. `preferred`+`MCP_PORT_FALLBACK_RANGE`-1) if occupied.
/// As a last resort, bind to OS-assigned port (0).
///
/// Returns the bound listener and the actual port number.
pub async fn bind_with_fallback(
    preferred: u16,
) -> std::io::Result<(tokio::net::TcpListener, u16)> {
    let preferred = if preferred == 0 { DEFAULT_MCP_PORT } else { preferred };

    // Try preferred port first, then a small contiguous range.
    for offset in 0..MCP_PORT_FALLBACK_RANGE {
        let port = preferred.saturating_add(offset);
        if port == 0 {
            continue;
        }
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => {
                let actual = listener.local_addr()?.port();
                return Ok((listener, actual));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => continue,
            Err(e) => return Err(e),
        }
    }

    // Fallback: let the OS pick any free port.
    let listener =
        tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    let actual = listener.local_addr()?.port();
    Ok((listener, actual))
}

/// Authentication and Origin-header middleware.
///
/// - 401 if `Authorization: Bearer <token>` is missing or doesn't match.
/// - 403 if the `Origin` header is set to anything other than loopback.
async fn auth_middleware(
    axum::extract::State(token): axum::extract::State<std::sync::Arc<String>>,
    req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let headers = req.headers();

    // Origin check first — defends against DNS rebinding from local browsers.
    if let Some(origin) = headers.get(axum::http::header::ORIGIN) {
        let origin_str = origin.to_str().unwrap_or("");
        let is_loopback = origin_str.is_empty()
            || origin_str.starts_with("http://127.0.0.1")
            || origin_str.starts_with("http://localhost")
            || origin_str.starts_with("https://127.0.0.1")
            || origin_str.starts_with("https://localhost");
        if !is_loopback {
            tracing::warn!(origin = origin_str, "rejected non-loopback Origin");
            return axum::response::Response::builder()
                .status(axum::http::StatusCode::FORBIDDEN)
                .body(axum::body::Body::from("Forbidden: non-loopback Origin"))
                .expect("valid response");
        }
    }

    // Bearer token check.
    let auth_ok = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(|presented| presented.trim() == token.as_str())
        .unwrap_or(false);

    if !auth_ok {
        tracing::warn!("rejected request with missing or invalid Bearer token");
        return axum::response::Response::builder()
            .status(axum::http::StatusCode::UNAUTHORIZED)
            .body(axum::body::Body::from("Unauthorized"))
            .expect("valid response");
    }

    next.run(req).await
}

/// Logging middleware — assigns a per-request UUID and wraps the handler in
/// a tracing span so all log lines from one request share the same `conn`
/// tag. We can't easily extract `clientInfo.name` here without parsing the
/// body, so the span carries the connection UUID; the `clientInfo.name` is
/// logged when the `initialize` JSON-RPC message is processed by the server.
async fn logging_middleware(
    req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use tracing::Instrument;

    let conn_id = uuid::Uuid::new_v4();
    let conn_short = conn_id.simple().to_string();
    let conn_short = &conn_short[..8];
    let method = req.method().clone();
    let uri = req.uri().path().to_string();

    let span = tracing::info_span!("mcp_req", conn = %conn_short);
    async move {
        tracing::info!(method = %method, path = %uri, "request");
        let resp = next.run(req).await;
        tracing::info!(status = %resp.status(), "response");
        resp
    }
    .instrument(span)
    .await
}

/// Run the MCP server over Streamable HTTP on the given listener.
///
/// `token` is the Bearer token clients must present.
/// `shutdown_token` cancels the server when the host app exits.
pub async fn run_mcp_http_server(
    listener: tokio::net::TcpListener,
    token: String,
    shutdown_token: tokio_util::sync::CancellationToken,
) -> Result<(), Box<dyn std::error::Error>> {
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    };

    let config = AppConfig::load();
    let mcp_service: StreamableHttpService<AutoPipeServer, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(AutoPipeServer::new(config.clone())),
            std::sync::Arc::new(LocalSessionManager::default()),
            StreamableHttpServerConfig {
                stateful_mode: true,
                cancellation_token: shutdown_token.child_token(),
                ..Default::default()
            },
        );

    let token_state = std::sync::Arc::new(token);

    let router = axum::Router::new()
        .nest_service("/mcp", mcp_service)
        .route("/health", axum::routing::get(|| async { "ok" }))
        .layer(axum::middleware::from_fn_with_state(
            token_state,
            auth_middleware,
        ))
        .layer(axum::middleware::from_fn(logging_middleware));

    let local_addr = listener.local_addr()?;
    tracing::info!(addr = %local_addr, "MCP HTTP server listening");

    let shutdown = shutdown_token.clone();
    axum::serve(listener, router)
        .with_graceful_shutdown(async move { shutdown.cancelled_owned().await })
        .await?;

    tracing::info!("MCP HTTP server stopped");
    Ok(())
}
