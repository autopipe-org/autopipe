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

# Install bioinformatics tools with the fastest available solver (micromamba >
# mamba > conda; all resolve the same conda packages). This uses the base env,
# matching most existing pipelines. ALTERNATIVE: if a base-env build hits the
# "Cannot find a valid extracted directory cache" linking error on an
# overlayfs/rootless build host, or the env is large, install into a dedicated
# env instead — `conda create -y -n pipeline ...` + `ENV PATH=/opt/conda/envs/pipeline/bin:$PATH`
# and symlink /opt/conda/envs/pipeline/bin/bash below.
RUN INSTALL="conda install -y"; \
    command -v mamba >/dev/null 2>&1 && INSTALL="mamba install -y"; \
    command -v micromamba >/dev/null 2>&1 && INSTALL="micromamba install -y -n base"; \
    $INSTALL -c bioconda -c conda-forge \
    snakemake-minimal \
    bash \
    curl \
    # tool1=version \
    # tool2=version \
    && conda clean -afy

# Replace system bash with conda bash (prevents GLIBC mismatch) — only when a
# conda bash exists, so a non-conda image is not left with a dangling /bin/sh
# (which makes every shell command fail).
RUN if [ -x /opt/conda/bin/bash ]; then \
        ln -sf /opt/conda/bin/bash /usr/bin/bash && \
        ln -sf /opt/conda/bin/bash /bin/sh; \
    fi

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
pub const CONFIG_YAML_TEMPLATE: &str = r#"# Required: paired-end R1 FASTQ (input file)
r1: "/input/sample_R1.fastq.gz"
# Required: paired-end R2 FASTQ (input file)
r2: "/input/sample_R2.fastq.gz"

# Required: reference genome, mounted at runtime (input file)
reference: "/input/reference.fa"

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
      "license": {"@id": "https://spdx.org/licenses/<SPDX_ID — ask the user before filling this in; recommend MIT if they are unsure>"},
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
A pipeline whose input is set up in chat also has SETUP.md (see "Input Setup: the Input page or chat").
Code copied in from elsewhere lives only under vendor/<name>/, recorded in vendor/VENDORED.json (see "Vendored Code").

## Clarify Before Generating (ask first — do NOT guess)
If the user's request is ambiguous, or is missing details needed to write correct
code, do NOT invent them. Ask the user targeted questions FIRST, then generate.
Batch every question into one message to minimize back-and-forth. Ask about
anything that changes correctness:
- Input data: format, path, number of samples, single-end vs paired-end
- Reference: whether a genome/annotation is needed, and which one
- Tool choice: when several tools fit the same step (e.g. bwa vs bowtie2)
- Key parameters: values that materially change results (cutoffs, k-mer size, etc.)
- Final outputs and scope: how far the workflow goes (QC only? alignment? variant calling?)
- Compute: when the user asks for GPU/CUDA or a step has a GPU implementation, whether a GPU is
  REQUIRED or a CPU fallback is acceptable (see "GPU Pipelines")
For trivial or conventional details, pick a sensible default and STATE that
assumption in the review summary instead of asking.

## Config Variables: default value vs blank (ask)
ALL tunable parameters go in config.yaml — never hardcode them in the Snakefile.
For each variable, ask the user whether to (a) pre-fill a sensible default value,
or (b) leave it blank so the user fills it in themselves. Batch this with the
clarifying questions above: present the list of variables and let the user choose
all-defaults / all-blank / per-variable. Blank values are surfaced as must-fill
items by review_pipeline and are enforced again at run time.

## Review After Generating (MANDATORY)
After writing all files and running validate_pipeline, ALWAYS call review_pipeline
on the pipeline directory, present its natural-language summary report to the user,
and get their confirmation (or apply requested fixes) BEFORE calling build_image.
Re-run review_pipeline after any edit. Never skip this for a pipeline you just
generated, and never rely on a remembered earlier approval.

## Snakefile Rules
- Use `configfile: "config.yaml"` for all parameters
- Define `rule all` with final expected outputs
- Each rule = one logical analysis step
- Use `threads` directive for parallelizable steps
- Use `log` directive for capturing tool output
- Use `expand()` for sample-level parallelism

## Dockerfile Rules
- Base image: `condaforge/miniforge3:latest`
- Install bioconda/conda-forge tools with the fastest available solver, preferring
  `micromamba` > `mamba` > `conda` (all resolve the same packages). Pick the first
  that exists at build time, e.g.:
  `RUN INSTALL="conda install -y"; command -v mamba >/dev/null 2>&1 && INSTALL="mamba install -y"; command -v micromamba >/dev/null 2>&1 && INSTALL="micromamba install -y -n base"; $INSTALL -c bioconda -c conda-forge ...`
- Always install `snakemake-minimal` and `bash` via conda
- After installing, replace system bash with conda bash to prevent GLIBC mismatch, GUARDED so it only runs when that bash exists (otherwise a non-conda image is left with a dangling /bin/sh and every shell command fails):
  `RUN if [ -x /opt/conda/bin/bash ]; then ln -sf /opt/conda/bin/bash /usr/bin/bash && ln -sf /opt/conda/bin/bash /bin/sh; fi`
- ALTERNATIVE (dedicated env) — both patterns are valid and already used across the hub. Prefer this when a base-env build fails with "Cannot find a valid extracted directory cache" (an overlayfs/rootless-Docker linking bug that base-env re-links can trigger), or when the environment is large/heavy: install into a fresh env with `conda create -y -n pipeline ...` (same solver preference), add `ENV PATH=/opt/conda/envs/pipeline/bin:$PATH`, and guard-symlink `/opt/conda/envs/pipeline/bin/bash` instead of the base one.
- Pin tool versions for reproducibility (e.g., `bwa=0.7.18`)
- For Python (PyPI) packages, use `uv pip install --system` instead of `pip install`
  - Install uv first: `RUN pip install uv`
  - The `--system` flag is required to install into the conda environment
  - uv resolves dependencies much faster than pip
- Copy Snakefile and config.yaml into `/pipeline`
- Clean up: `conda clean -afy`
- Set `WORKDIR /pipeline`
- Do NOT use Docker commands (docker run, docker pull) directly inside Snakefile rules. The only exception is `nextflow run` with `-profile docker`, which manages its own containers through the mounted Docker socket.
- If converting from Nextflow or other container-based workflows, install all required tools from every container into the single Dockerfile.
- If the user already has a working Dockerfile from their analysis environment, use it as the base instead of writing one from scratch.
- GPU pipelines keep this base image: install CUDA-enabled wheels (see "GPU Pipelines"). Never install an NVIDIA driver in the image.

## config.yaml Rules
- WRITE EVERYTHING IN ENGLISH — config.yaml comments, README.md and
  ro-crate-metadata.json. These are published to the Hub and read by people who
  may not share the user's language. Talk to the user in their own language, but
  keep the pipeline's own files English-only.
- ALL configurable parameters go here, not in Snakefile
- REQUIRED: every parameter MUST have a one-line explanatory comment describing what it is — either inline (`key: value  # what it is`) or on the line directly above the key. The AutoPipe Input page shows these comments as each field's description, so a variable with no comment appears with no help text.
- Mark REQUIRED parameters by including the word `Required` in that parameter's comment (e.g. `# Required: paired-end R1 FASTQ`). The Input page reads this keyword to show a red `*` next to required fields.
- Fill a default value or leave it blank per the user's choice (see "Config Variables: default value vs blank")
- IMPORTANT: Use `/input` and `/output` as paths (Docker mount points)
  - Input data is mounted at `/input` (read-only) at runtime
  - Output directory is mounted at `/output` at runtime
  - Do NOT use absolute host paths like `/home/user/data/...`
- PREFER TOP-LEVEL scalar keys for input files. The Input page renders such a key
  as a file BROWSE field (a native file picker) rather than a plain text box. A key
  becomes a browsable input file when its name is one of `r1, r2, reads, input,
  fastq, fq, reference, genome, fasta, fa, bam` (or ends/starts with one, e.g.
  `tumor_bam`), OR its value starts with `"/input/"`.
  - BEST (each input its own top-level key → browsable, and easiest to fill in):
    `r1: "/input/sample_R1.fastq.gz"  # Required: paired-end R1 FASTQ`
    `r2: "/input/sample_R2.fastq.gz"  # Required: paired-end R2 FASTQ`
  - Nested mappings and lists ARE supported: the Input page shows a nested leaf as
    `parent.child` and a list as one editable field (comma-separated on a single
    line, or one item per line when an item itself contains a comma), and writes
    both back in place, keeping the YAML structure and type. So a config that
    groups values under `qc:` or keeps a `samples:` list still works.
  - Still prefer flat keys for INPUT FILES specifically: only a top-level key whose
    NAME matches the list above gets the browse button, so a nested
    `wild_type.h5_file` is editable but must be typed rather than picked. Give each
    input file its own top-level key (`r1`, `r2`, `reference`, ...) when you can.
- SAY WHICH KEYS ARE FILES. The page decides whether to show a file picker from
  the comment first, and only guesses when the comment is silent (a file-ish key
  NAME plus a value that looks like a path). A blank value has nothing to guess
  from, so be explicit whenever you ship a key blank:
  - end the comment with `(input file)` for a key the user supplies a file for;
  - end it with `(not a file)` for a key whose name looks like a file but whose
    value is not a path (e.g. `genome: "mm10"`, a genome build name).
  Both markers are stripped from the help text shown on the page, so write the
  comment as a normal sentence and just append the marker:
    `r1: ""  # Required: paired-end R1 FASTQ (input file)`
    `genome: "mm10"  # Genome build, mm10 or hg38 (not a file)`
- A key that is an input file is treated as REQUIRED unless its comment says
  `Optional`, so there is no need to also write `Required` on it.
- A key must not be BOTH a value and a parent: write either `key: value` or an
  indented block under `key:`, never a scalar on a key that also has children.
- Keep a path relative to `/input` consistent with how the Snakefile reads it. If a
  rule prefixes the value itself (e.g. `$HOST_INPUT_DIR/{params.fasta}`), accept an
  absolute `/input/...` value too — the Input page writes that form when a file is
  picked — by stripping the `/input/` prefix before use.

## Getting Input From the User: the Input page first, chat when they ask
- NEVER hardcode host paths. Every input file and tunable parameter comes from the
  user, normally through the AutoPipe Input page ("Configure Input"), which reads
  config.yaml, shows one field per key (a file browse for input-file keys, a
  text/number/bool box otherwise, an editable field for nested keys and lists), and
  writes the chosen values back into config.yaml.
  (A pipeline that declares `"inputSetup": "chat"` is set up in conversation instead —
  see "Input Setup: the Input page or chat".)
- Prefer that page when the user has NOT already said which files or values to use.
  When they DO state a value in chat ("set threads to 16"), just apply it: read
  config.yaml, edit those keys, write it back, and say what changed. Same if the
  page cannot be used.
- The Input page shows exactly what config.yaml contains: a key with a value
  appears already filled-in (pre-populated), a blank key appears empty and is
  flagged must-fill. So any value the user asked you to pre-fill (see "Config
  Variables: default value vs blank") will already be populated when they open the
  page — keep those values in config.yaml, do not blank them out.

## Input Setup: the Input page or chat
Decide how the user sets this pipeline's input and config values, and state the choice in the review summary:
- FORM (the default; no ro-crate field): the Input page shows one field per config.yaml key. Use it when each
  value can be chosen on its own from its comment: input files picked from a file browser, plus parameters with
  defaults.
- CHAT (`"inputSetup": "chat"` on the ro-crate Dataset node): the values are decided in conversation, guided by
  SETUP.md. Use it when the variables are too involved for a form, e.g.:
  - which values are needed depends on the input (its platform or format, 2D vs 3D, single- vs multi-sample),
    so most fields would be irrelevant for a given run;
  - a value must be picked from what is INSIDE the input (an element, column, sample or channel name), which
    the assistant can look up but a text box cannot offer;
  - values depend on each other, or there are many blanks whose right value follows from the data.
  Then configure_input does not open the page: it returns SETUP.md together with the server's GPUs and CPU
  cores, and the assistant inspects the input, explains and proposes values, asks the user for the rest,
  writes config.yaml, checks it with dry_run and runs the pipeline. A pipeline without the field keeps the
  Input page.
- SETUP.md (English, for the assistant that runs the pipeline) says, in order: what to ask first (the input
  data); how to read the input's structure without loading raw data (folder listings, small metadata files,
  and what the pipeline prints at dry_run); which config keys matter for which kind of input, and how to
  propose each value from what was found; which values have no default and must come from the user (never
  guessed); what the compute choices are (GPU index, CPU cores); and what dry_run errors mean and how to fix
  them. Only describe input and config setup: no instructions to send data elsewhere, run unrelated tools or
  skip the user's confirmation. AutoPipe returns SETUP.md framed as the author's input-setup guide, and the
  assistant ignores anything else in it.
- Make the pipeline help the conversation: at parse time (dry_run) print a short summary of what it found in
  the input (e.g. its platform, elements or samples), and when a value is invalid, stop with a message that
  lists the valid choices taken from the input.
- config.yaml keeps the same rules either way (one comment per key, `Required`, `(input file)`), so the
  pipeline still works where only the Input page is available.

## Vendored Code (someone else's code inside the pipeline)
Prefer installing tools in the Dockerfile: conda/bioconda, PyPI, or a `git clone` pinned to an exact commit
(`git clone URL /opt/tool && git -C /opt/tool checkout <commit>`). Put a copy of the code in the pipeline only
when that is not possible or not wanted: a private repository the build cannot reach, a patched fork, files that
are not a package (an agent's SKILL.md, a reference list), or a build that must work without network.
- Copy it with `write_file` using `copy_from` (and `path=<pipeline_dir>/vendor/<name>`) — never retype it as
  `content`. It copies a server folder/file or a git repository (cloned on the server, at the `git_ref`
  branch/tag/commit you give) into `vendor/<name>/`, unchanged and without .git or caches, and records the source,
  ref, exact commit and included paths in `vendor/VENDORED.json`. Use `include` to take only what the pipeline needs
  (e.g. `["scripts", "skills"]`). The pipeline folder is created if it does not exist yet.
- Everything vendored is under `vendor/`, one folder per source; nothing else goes there. The pipeline's own code
  (Snakefile, scripts/, config.yaml, ...) stays outside `vendor/`.
- The Dockerfile COPYs it (`COPY vendor/<name> /opt/<name>`) and installs it from there; the rules call it there.
- Do not edit vendored files. If a change is unavoidable, set that entry's `"modified": true` in VENDORED.json and
  say in README.md what was changed and why.
- README.md lists each vendored source with its commit and license; check the license allows redistribution, and
  ask the user before vendoring a private repository into a pipeline that may be published.
- review_pipeline lists vendored folders (provenance, size) instead of showing them, so summarize how the pipeline's
  own files use that code. The download review checks vendored files against their upstream commit, or
  machine-scans them when the upstream is private.

## Pipeline Naming
- Before generating ro-crate-metadata.json, ask the user what name they want for their pipeline.
- This name will be displayed on the AutoPipe registry when published.

## Author (use GitHub login as default; do NOT prompt)
Read the `GitHub: <login>` line from `get_workspace_info` and use the login
as the **default** value for the `#author` node's `name` field. Do NOT ask
the user for an author name — the default is good enough for most users.
If the user explicitly says they want a different author (e.g., "publish
as 'Smith Lab'"), use what they specified instead.

If `get_workspace_info` shows `GitHub: (not connected)`: generate the
pipeline anyway with `#author.name` left empty, and tell the user once, in
chat, that they will need to complete the GitHub connection step in the
AutoPipe app before publishing. Do not block generation on this — when the
user later runs `validate_pipeline`, the validator will automatically fill
`#author.name` from the GitHub login once it is available, and will return
a clear error pointing back to the AutoPipe app if GitHub is still not
connected at that point.

## License Selection (ASK USER before generating ro-crate-metadata.json)
Before writing the metadata file, ask the user which open-source license to apply.
You MUST present ALL FIVE options below — do not collapse, omit, or summarise any
of them. Option 5 is REQUIRED so users with a non-listed license still know what to
do; never drop it because you assume the user will pick one of the first four.

Render the choices exactly like this (translate the prose into the user's chat
language but keep the SPDX identifiers and the URL verbatim):

  1. MIT — recommended default. The most permissive and widely-used license; pick
     this if you are unsure or unfamiliar with software licensing.
  2. BSD-3-Clause — permissive, similar to MIT with a no-endorsement clause.
  3. Apache-2.0 — permissive with an explicit patent grant.
  4. GPL-3.0 — copyleft; downstream forks must also be GPL-licensed.
  5. Other — look up the SPDX identifier of your preferred license at
     https://opensource.org/licenses and tell me which one to use.

If the user is unsure or asks for guidance, suggest MIT explicitly and explain
that it is the most permissive and widely-used license for open scientific
pipelines. Only proceed when the user has explicitly confirmed a choice.

Use the chosen SPDX identifier in the metadata's `license` field:
  "license": {"@id": "https://spdx.org/licenses/<SPDX_ID>"}

Examples:
  https://spdx.org/licenses/MIT
  https://spdx.org/licenses/BSD-3-Clause
  https://spdx.org/licenses/Apache-2.0
  https://spdx.org/licenses/GPL-3.0

## ro-crate-metadata.json (RO-Crate Format)
- Must follow RO-Crate 1.1 specification (JSON-LD)
- Dataset node requires: `name`, `description`, `version`, `license`
- `creator`: Person objects with `name` field
- `softwareRequirements`: SoftwareApplication objects with `name` field.
  ONLY include bioinformatics tools that directly analyze or process data in the Snakefile rules.
  Do NOT include programming languages, package managers, workflow engines, system libraries,
  or any infrastructure dependencies (Python, R, snakemake, conda, pip, etc.).
- `input` / `output`: FormalParameter objects with `name` and `encodingFormat`
- `keywords`: array of search tags
- `programmingLanguage`: always reference Snakemake
- `gpu` (GPU pipelines only): `"required"` or `"optional"` on the Dataset node — see "GPU Pipelines"
- `inputSetup` (only when the input is set up in chat): `"chat"` on the Dataset node — see "Input Setup: the
  Input page or chat"; leave it out for the Input page
- `llmCli` (pipelines that run an AI CLI inside a rule only): `"claude"`, `"codex"` or a list of both on the
  Dataset node — see "AI CLI Steps"

## README.md Content
- What the pipeline does (1-2 sentences)
- Required inputs with format description
- Expected outputs
- How to run (docker build + docker run commands). For a GPU pipeline say instead that it is run
  through AutoPipe, which attaches the GPUs — do not give a hand-written `docker run --gpus` command
- Configuration options from config.yaml

## Path Convention
- Pipelines always use Docker mount points for data paths:
  - `/input` — input data (mounted read-only at runtime)
  - `/output` — output directory (mounted at runtime)
  - `/pipeline` — pipeline files (Snakefile, config.yaml, etc.)
- Actual host paths are provided at execution time, not in pipeline files.
- Example in Snakefile: `"input/{sample}.fastq.gz"` (relative to Docker workdir)
- Example in config.yaml: `reference: "/input/reference.fa"`

## Nextflow / nf-core / Docker-in-Docker Support
When the pipeline uses nextflow, nf-core, or any command that runs Docker inside the container:
- BEFORE calling build_image, you MUST ask the user for confirmation:
  "This pipeline uses external containers (e.g., nf-core images) that are not inspected by AutoPipe.
   Running it requires mounting the host Docker socket. Proceed with build and execution?"
- If the user APPROVES: call build_image, then execute_pipeline with needs_docker_socket=true.
- If the user DECLINES: stop. Do NOT build or execute. Tell the user this pipeline cannot run
  without Docker socket access and ask if they want a different approach.
- Always use `-profile docker` for nextflow when approved.
- Install nextflow in the Dockerfile: `RUN curl -s https://get.nextflow.io | bash && mv nextflow /usr/local/bin/`
- CRITICAL: When needs_docker_socket=true, the environment variables HOST_INPUT_DIR, HOST_OUTPUT_DIR, and HOST_PIPELINE_DIR are automatically set inside the container with the actual host paths. In Snakefile rules that call nextflow or docker, ALWAYS use these variables instead of /input, /output, /pipeline. Also set NXF_HOME to $HOST_OUTPUT_DIR/.nextflow. Example:
  ```
  shell:
      """
      export NXF_HOME=$HOST_OUTPUT_DIR/.nextflow
      nextflow run nf-core/rnaseq -profile docker \
          --input $HOST_INPUT_DIR/samplesheet.csv \
          --outdir $HOST_OUTPUT_DIR/nfcore \
          -w $HOST_OUTPUT_DIR/.nextflow/work
      """
  ```
  For non-nextflow rules in the same Snakefile, keep using /input, /output, /pipeline as usual.
- Keep everything in a single Snakefile + single Dockerfile as usual.

## GPU Pipelines
Use this section when the user asks for GPU / CUDA acceleration, or the analysis uses a GPU
implementation (rapids-singlecell, cuML/cuGraph, CuPy, PyTorch/TensorFlow/JAX on CUDA, Parabricks, ...).
How it works: the code is ordinary CUDA code. At run time execute_pipeline attaches the server's GPUs
to the container (the host's NVIDIA driver is injected then), so the same image runs on any GPU server.
- Ask with the clarifying questions: is a GPU REQUIRED, or may the pipeline fall back to CPU when the
  server has none? If the user explicitly asked for GPU, default to REQUIRED. Note that GPU and CPU
  implementations usually give slightly different results (e.g. Leiden clusters), so a fallback run is
  not a reproduction of a GPU run.
- Declare it in ro-crate-metadata.json on the Dataset (`@id: "./"`) node: `"gpu": "required"` or
  `"gpu": "optional"`. execute_pipeline reads this and attaches all GPUs automatically; "required"
  refuses to start on a server that cannot attach a GPU. Say the same in README.md.
- config.yaml: `device: "cuda"  # Compute device: cuda (GPU) or cpu (not a file)` for a required GPU, or
  `device: "auto"  # Compute device: auto (GPU when available, else CPU), cuda or cpu (not a file)`
  for optional. Pass the resolved device to every GPU-capable step.
- First rule `check_gpu` (every other rule depends on its output): write `nvidia-smi` output (or
  "no GPU") to `gpu_info.txt`, resolve `auto`, and FAIL with a clear message when the device is `cuda`
  but no GPU is visible. Never fall back to CPU silently: with `auto`, write the device actually used
  into the outputs. Detecting a GPU: `nvidia-smi -L` succeeds only when a GPU was attached (the tool is
  injected with the driver), or ask the framework (`torch.cuda.is_available()`,
  `cupy.cuda.runtime.getDeviceCount()`).
- Code always uses the first visible device (`cuda` / `cuda:0`): the attached GPUs are renumbered from
  0 inside the container. Never hardcode server GPU indices; the user picks them with execute_pipeline's
  `gpus` ("1", "0,1").
- Dockerfile: keep the miniforge base and install CUDA-enabled builds with uv, pinned to one CUDA major
  version (e.g. `rapids-singlecell[rapids12]` / `cupy-cuda12x` / `cuml-cu12` with
  `--extra-index-url https://pypi.nvidia.com`; PyTorch from `https://download.pytorch.org/whl/cu128`).
  The CUDA runtime libraries come with these wheels; the server's driver must support that CUDA version.
  Also install the CPU implementation when the pipeline is "optional".
- Run it ONLY through execute_pipeline. Never put `docker run --gpus` in the Snakefile or tell the user to
  run docker by hand — AutoPipe picks the attach method that works on the server (CDI devices for
  rootless Docker, `--gpus` otherwise).

## AI CLI Steps (Claude Code / Codex run headless inside a rule)
Use this when a step is done by an AI agent end to end and the run must not stop for it — e.g. an existing
agent skill (instructions in Markdown) that the pipeline runs as it is. The rule calls `claude -p` (Claude Code)
or `codex exec` (Codex CLI) and waits for it to finish. If the AI judgment should come from the user's own
assistant between runs instead, use "Pausing for Input".
- The login is the one the user already has ON THE SERVER (a Claude or ChatGPT subscription works; no API key).
  Declare the CLI in ro-crate-metadata.json on the Dataset node: `"llmCli": "claude"` (or `"codex"`, or a list).
  execute_pipeline then checks the server user's login (`~/.claude/.credentials.json`, `~/.codex/auth.json`),
  mounts that folder into the container and sets `CLAUDE_CONFIG_DIR` / `CODEX_HOME`, so the CLI in the image is
  logged in. The user's own setup in that folder (skills, agents, plugins, settings, CLAUDE.md/AGENTS.md, MCP
  servers, codex config.toml) is hidden in the run and claude.ai connectors are off, so put everything the step
  needs (skills, agent files, model choice) in the pipeline itself. When none of the declared CLIs is logged in, the run refuses to start; with several declared
  (config.yaml picks one), the check rule must confirm the chosen one is logged in. NEVER put a token, API key or credential file in the pipeline,
  config.yaml or the input folder, and never ask the user for one.
- Dockerfile: install the CLI at a pinned version and turn its self-update off, e.g.
  `ENV DISABLE_AUTOUPDATER=1` + `RUN curl -fsSL https://claude.ai/install.sh | bash -s <version>` (then link
  `/root/.local/bin/claude` into `/usr/local/bin`), or Codex with npm in its own Node env
  (`npm install -g @openai/codex@<version>`, with that env's bin on PATH).
- config.yaml: which CLI (`judge: "claude"  # AI CLI for this step: claude or codex (not a file)`) and the model
  (blank = the CLI's default). The rule runs only the chosen CLI.
- First rule (the check rule): confirm the CLI is installed and answers one short prompt
  (`claude -p "Reply OK" --max-turns 1`), so a login or network problem fails before the heavy steps.
- The rule: give the agent its instructions as files in a working directory under the run's output
  (Claude Code: `.claude/skills/<name>/SKILL.md`, `.claude/agents/<name>.md`; Codex: `AGENTS.md`,
  `.agents/skills/`), keep permissions narrow (`--allowedTools` / `--permission-mode` for claude, `-s
  workspace-write` for codex), save the session's event stream (`--output-format stream-json --verbose` /
  `--json`) next to the outputs, and check the files the agent must produce — fail with its last message if
  they are missing. Record the CLI, its version and the model in the outputs.
- README.md: say that the step sends its input data to the AI provider, uses the server user's subscription
  quota, and that its results are not exactly reproducible between runs.

## Pausing for Input (user decisions and AI judgment mid-pipeline)
Some workflows need a decision in the middle: the user must confirm something (e.g. which reference or
module to use), or an AI assistant must judge intermediate results (e.g. an agentic annotation step)
before later steps can run. Snakemake cannot wait for a person, so the pipeline STOPS at that point and
is re-run after the answer is written. Keep everything in ONE Snakefile:
- The decision is a FILE in the run's output directory (e.g. `decisions/round1.json`), never a
  config.yaml edit: config.yaml is shared by every run of the pipeline, and a changed parameter makes
  Snakemake re-run the upstream steps. Snakemake runs with `/output` (the run's output directory) as
  its working directory, so relative paths like `decisions/round1.json` land there.
- Choose `rule all` targets from what exists: when the answer file is missing, target the outputs up to
  the decision point plus the pause rule's output; when it exists, target the final outputs. Do NOT list a
  missing answer file as a rule input — Snakemake then refuses to run anything.
  ```python
  import os
  ANSWER = "decisions/round1.json"
  if os.path.exists(ANSWER):
      FINAL = ["results/final.tsv"]
  else:
      FINAL = ["results/profiles.json", "awaiting/round1.done"]
  rule all:
      input: FINAL

  rule await_round1:
      input: "results/profiles.json"
      output: touch("awaiting/round1.done")
      run:
          import json
          json.dump({"decided_by": "user", "question": "...", "answer_file": ANSWER,
                     "format": "..."}, open("AWAITING_INPUT.json", "w"), indent=1)
          print("AWAITING_INPUT: write " + ANSWER + " (see AWAITING_INPUT.json), then re-run", flush=True)
  ```
- Each pause is its own rule with a per-round output (`awaiting/round{k}.done`) that (re)writes
  `AWAITING_INPUT.json` as a side effect, so a file left over from an earlier round can never stop a
  later pause from firing. `AWAITING_INPUT.json` describes, in plain English, what is needed: the
  question (or the judgment task and the files to read for it), who decides (user or AI), the exact
  path of the answer file and its format.
- The pause rule PRINTS a line starting with `AWAITING_INPUT:` to its standard output — do not give it
  a `log:` directive or redirect that line to a file: AutoPipe reads `pipeline.log` (Snakemake's own
  output) to show the run as paused rather than completed.
- Steps after the decision take the answer file as an input, so re-running with the SAME run_name skips
  everything already done and continues from the decision point. The step that consumes the answer
  deletes `AWAITING_INPUT.json`, so a finished run does not look paused to someone reading the folder.
- Repeated rounds (decide → recompute → decide again) use one answer file per round
  (`decisions/round{k}.json`), so every round is a new target.
- Validate the answer file before using it and fail with a clear message if it is malformed, so a wrong
  answer is caught at the decision point, not three steps later.

## Upload Rules
- When uploading a pipeline, include EVERY file the pipeline needs to build and run: Snakefile, Dockerfile, config.yaml, ro-crate-metadata.json, README.md, SETUP.md (chat input setup), scripts/ and other files you created, and the whole vendor/ folder with vendor/VENDORED.json (vendored code is part of the pipeline; leaving it out breaks the build).
- NEVER include files that are not part of the pipeline: run outputs, caches (__pycache__, .snakemake), test data, notes.
- Before calling upload_pipeline, verify the file list by checking what exists in the pipeline directory. Do NOT recursively include unrelated subdirectories.
- A folder may be listed as one entry (e.g. `vendor`, `scripts`); every file in it is uploaded. upload_pipeline checks the new commit against the server files before moving main and returns `upload_commit`; pass it to publish_pipeline, which tags exactly the uploaded files plus the version change. If either step reports a mismatch, tell the user and stop.

## Safety Rules
1. Pipelines use Snakemake format. Nextflow is allowed only inside Snakefile rules via `nextflow run`.
2. Every pipeline must have a Dockerfile.
3. NEVER modify or delete user input data. Mount as read-only (:ro).
4. NEVER run destructive commands on user-provided paths.
5. NEVER hardcode absolute host paths in pipeline files.
6. NEVER use `docker run` or `docker pull` directly inside Snakefile rules.
"#;
