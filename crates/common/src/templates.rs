/// Snakefile template for new pipelines.
pub const SNAKEFILE_TEMPLATE: &str = r#"configfile: "config.yaml"

SAMPLES = config["samples"]

rule all:
    input:
        expand("output/{sample}.final_output", sample=SAMPLES)

rule step_1:
    """First analysis step."""
    input:
        "input/{sample}.fastq.gz"
    output:
        "output/{sample}.step1_output"
    threads: config.get("threads", 4)
    log:
        "logs/{sample}_step1.log"
    shell:
        "tool_name -t {threads} -i {input} -o {output} 2> {log}"

rule step_2:
    """Second analysis step."""
    input:
        rules.step_1.output
    output:
        "output/{sample}.final_output"
    params:
        extra=config.get("extra_params", "")
    log:
        "logs/{sample}_step2.log"
    shell:
        "tool_name {params.extra} -i {input} -o {output} 2> {log}"
"#;

/// Dockerfile template for new pipelines.
pub const DOCKERFILE_TEMPLATE: &str = r#"FROM condaforge/mambaforge:latest

# Install bioinformatics tools
RUN mamba install -y -c bioconda -c conda-forge \
    snakemake-minimal \
    # tool1=version \
    # tool2=version \
    && mamba clean -afy

# Setup pipeline
WORKDIR /pipeline
COPY Snakefile .
COPY config.yaml .

CMD ["snakemake", "--help"]
"#;

/// config.yaml template for new pipelines.
pub const CONFIG_YAML_TEMPLATE: &str = r#"# Required: list of sample names (without extension)
samples:
  - sample1
  - sample2

# Required: path to reference genome (mounted at runtime)
# reference: "/input/reference.fa"

# Optional: number of threads per rule (default: 4)
threads: 4

# Optional: additional parameters
# extra_params: ""
"#;

/// metadata.json template for new pipelines.
pub const METADATA_JSON_TEMPLATE: &str = r#"{
  "name": "pipeline-name",
  "description": "One paragraph description of what this pipeline does.",
  "version": "1.0.0",
  "author": "",
  "tools": ["tool1", "tool2"],
  "input_formats": ["fastq", "fastq.gz"],
  "output_formats": ["bam", "vcf"],
  "tags": ["tag1", "tag2"],
  "parameters": {
    "threads": "Number of CPU threads",
    "reference": "Path to reference genome"
  },
  "verified": false,
  "created_at": ""
}
"#;

/// Pipeline generation guidelines for Claude (MCP resource).
pub const GENERATION_GUIDE: &str = r#"# AutoPipe Pipeline Generation Guide

## Pipeline Structure
Every pipeline is a directory with 5 required files:
- Snakefile: Snakemake workflow
- Dockerfile: Execution environment
- config.yaml: Parameters
- metadata.json: Name, description, tools, I/O, tags
- README.md: Usage instructions

## Snakefile Rules
- Use `configfile: "config.yaml"` for all parameters
- Define `rule all` with final expected outputs
- Each rule = one logical analysis step
- Use `threads` directive for parallelizable steps
- Use `log` directive for capturing tool output
- Use `expand()` for sample-level parallelism

## Dockerfile Rules
- Base image: `condaforge/mambaforge:latest`
- Install tools via `mamba install -c bioconda -c conda-forge`
- Always install `snakemake-minimal`
- Pin tool versions for reproducibility (e.g., `bwa=0.7.18`)
- Copy Snakefile and config.yaml into `/pipeline`
- Clean up: `mamba clean -afy`
- Set `WORKDIR /pipeline`

## config.yaml Rules
- ALL configurable parameters go here, not in Snakefile
- Include comments explaining each parameter
- Provide sensible defaults
- Mark required parameters with comments

## metadata.json Required Fields
- `name`: pipeline name (lowercase, hyphens)
- `description`: one paragraph
- `version`: semver
- `tools`: array of tool names
- `input_formats`: array of input file types
- `output_formats`: array of output file types
- `tags`: array of keywords for search
- `verified`: boolean (false until tested)

## README.md Content
- What the pipeline does (1-2 sentences)
- Required inputs with format description
- Expected outputs
- How to run (docker build + docker run commands)
- Configuration options from config.yaml

## Safety Rules
1. All pipelines use Snakemake format only.
2. Every pipeline must have a Dockerfile.
3. NEVER modify or delete user input data. Mount as read-only (:ro).
4. NEVER run destructive commands on user-provided paths.
"#;
