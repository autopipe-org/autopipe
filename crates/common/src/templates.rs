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
pub const DOCKERFILE_TEMPLATE: &str = r#"FROM condaforge/miniforge3:latest

# Install bioinformatics tools
RUN conda install -y -c bioconda -c conda-forge \
    snakemake-minimal \
    bash \
    # tool1=version \
    # tool2=version \
    && conda clean -afy

# Replace system bash with conda bash (prevents GLIBC mismatch)
RUN ln -sf /opt/conda/bin/bash /usr/bin/bash && \
    ln -sf /opt/conda/bin/bash /bin/sh

# Install uv for fast Python package installation
RUN pip install uv

# Python packages (use uv instead of pip for faster dependency resolution)
# RUN uv pip install --system package1 package2

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

/// RO-Crate metadata template for new pipelines (ro-crate-metadata.json).
pub const RO_CRATE_METADATA_TEMPLATE: &str = r##"{
  "@context": "https://w3id.org/ro/crate/1.1/context",
  "@graph": [
    {
      "@id": "ro-crate-metadata.json",
      "@type": "CreativeWork",
      "about": {"@id": "./"},
      "conformsTo": {"@id": "https://w3id.org/ro/crate/1.1"}
    },
    {
      "@id": "./",
      "@type": ["Dataset", "SoftwareSourceCode", "ComputationalWorkflow"],
      "name": "pipeline-name",
      "description": "One paragraph description of what this pipeline does.",
      "version": "1.0.0",
      "license": {"@id": "https://spdx.org/licenses/MIT"},
      "programmingLanguage": {"@id": "#snakemake"},
      "creator": [{"@id": "#author"}],
      "dateCreated": "",
      "sdPublisher": {"@id": "https://hub.autopipe.org"},
      "isBasedOn": {"@id": ""},
      "softwareRequirements": [
        {"@id": "#tool1"},
        {"@id": "#tool2"}
      ],
      "input": [
        {"@id": "#input-fastq"},
        {"@id": "#input-fastq-gz"}
      ],
      "output": [
        {"@id": "#output-bam"},
        {"@id": "#output-vcf"}
      ],
      "keywords": ["tag1", "tag2"]
    },
    {
      "@id": "#author",
      "@type": "Person",
      "name": ""
    },
    {
      "@id": "#snakemake",
      "@type": "ComputerLanguage",
      "name": "Snakemake",
      "url": "https://snakemake.readthedocs.io"
    },
    {
      "@id": "#tool1",
      "@type": "SoftwareApplication",
      "name": "tool1"
    },
    {
      "@id": "#tool2",
      "@type": "SoftwareApplication",
      "name": "tool2"
    },
    {
      "@id": "#input-fastq",
      "@type": "FormalParameter",
      "name": "fastq",
      "encodingFormat": "application/x-fastq"
    },
    {
      "@id": "#input-fastq-gz",
      "@type": "FormalParameter",
      "name": "fastq.gz",
      "encodingFormat": "application/gzip"
    },
    {
      "@id": "#output-bam",
      "@type": "FormalParameter",
      "name": "bam",
      "encodingFormat": "application/x-bam"
    },
    {
      "@id": "#output-vcf",
      "@type": "FormalParameter",
      "name": "vcf",
      "encodingFormat": "text/x-vcf"
    }
  ]
}
"##;

/// Pipeline generation guidelines for Claude (MCP resource).
pub const GENERATION_GUIDE: &str = r#"# AutoPipe Pipeline Generation Guide

## Pipeline Structure
Every pipeline is a directory with 5 required files:
- Snakefile: Snakemake workflow
- Dockerfile: Execution environment
- config.yaml: Parameters
- ro-crate-metadata.json: Name, description, tools, I/O, tags
- README.md: Usage instructions

## Snakefile Rules
- Use `configfile: "config.yaml"` for all parameters
- Define `rule all` with final expected outputs
- Each rule = one logical analysis step
- Use `threads` directive for parallelizable steps
- Use `log` directive for capturing tool output
- Use `expand()` for sample-level parallelism

## Dockerfile Rules
- Base image: `condaforge/miniforge3:latest`
- Install bioconda/conda-forge tools via `conda install -c bioconda -c conda-forge`
- Always install `snakemake-minimal` and `bash` via conda
- After installing, replace system bash with conda bash to prevent GLIBC mismatch:
  `RUN ln -sf /opt/conda/bin/bash /usr/bin/bash && ln -sf /opt/conda/bin/bash /bin/sh`
- Pin tool versions for reproducibility (e.g., `bwa=0.7.18`)
- For Python (PyPI) packages, use `uv pip install --system` instead of `pip install`
  - Install uv first: `RUN pip install uv`
  - The `--system` flag is required to install into the conda environment
  - uv resolves dependencies much faster than pip
- Copy Snakefile and config.yaml into `/pipeline`
- Clean up: `conda clean -afy`
- Set `WORKDIR /pipeline`
- Each pipeline must have exactly ONE Dockerfile. Do NOT use Docker commands (docker run, docker pull) inside Snakefile rules.
- If converting from Nextflow or other container-based workflows, install all required tools from every container into the single Dockerfile.
- If the user already has a working Dockerfile from their analysis environment, use it as the base instead of writing one from scratch.

## config.yaml Rules
- ALL configurable parameters go here, not in Snakefile
- Include comments explaining each parameter
- Provide sensible defaults
- Mark required parameters with comments
- IMPORTANT: Use `/input` and `/output` as paths (Docker mount points)
  - Input data is mounted at `/input` (read-only) at runtime
  - Output directory is mounted at `/output` at runtime
  - Do NOT use absolute host paths like `/home/user/data/...`

## ro-crate-metadata.json (RO-Crate Format)
- Must follow RO-Crate 1.1 specification (JSON-LD)
- Dataset node requires: `name`, `description`, `version`, `license`
- `creator`: Person objects with `name` field
- `softwareRequirements`: SoftwareApplication objects with `name` field
- `input` / `output`: FormalParameter objects with `name` and `encodingFormat`
- `keywords`: array of search tags
- `programmingLanguage`: always reference Snakemake

## README.md Content
- What the pipeline does (1-2 sentences)
- Required inputs with format description
- Expected outputs
- How to run (docker build + docker run commands)
- Configuration options from config.yaml

## Path Convention
- Pipelines always use Docker mount points for data paths:
  - `/input` — input data (mounted read-only at runtime)
  - `/output` — output directory (mounted at runtime)
  - `/pipeline` — pipeline files (Snakefile, config.yaml, etc.)
- Actual host paths are provided at execution time, not in pipeline files.
- Example in Snakefile: `"input/{sample}.fastq.gz"` (relative to Docker workdir)
- Example in config.yaml: `reference: "/input/reference.fa"`

## Safety Rules
1. All pipelines use Snakemake format only.
2. Every pipeline must have a Dockerfile.
3. NEVER modify or delete user input data. Mount as read-only (:ro).
4. NEVER run destructive commands on user-provided paths.
5. NEVER hardcode absolute host paths in pipeline files.
"#;
