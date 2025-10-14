# bunker-convert

[![Build Status](https://img.shields.io/badge/build-passing-brightgreen)](https://github.com/bunkercorporation/bunker-convert)
[![Version](https://img.shields.io/badge/version-0.1.0-blue)](https://github.com/bunkercorporation/bunker-convert)
[![License](https://img.shields.io/badge/license-check_cargo.toml-lightgrey)](./Cargo.toml)
[![Rust Edition](https://img.shields.io/badge/rust-2024-orange)](https://www.rust-lang.org/)

> Modern, GPU-accelerated media pipeline toolkit with declarative recipes and reproducible builds

## What is bunker-convert?

**bunker-convert** is a high-performance media transformation tool designed for production pipelines that demand speed, reproducibility, and developer-friendly workflows. Unlike traditional tools like ImageMagick, bunker-convert uses declarative YAML recipes, GPU acceleration, and built-in quality gates to deliver consistent, auditable results at scale.

### Why bunker-convert?

| Feature | ImageMagick convert | bunker-convert |
|---------|-------------------|----------------|
| **Processing** | CPU-centric, legacy CLI | GPU-first with CPU fallback (CUDA/Metal/Vulkan) |
| **Syntax** | Order-sensitive flags | Declarative pipeline stages, YAML/JSON specs |
| **Reproducibility** | Manual scripts, env-dependent | Versioned recipes, lockfiles, deterministic runs |
| **Observability** | Minimal | Structured logs, metrics, traces, quality gates |
| **Batch Processing** | Manual orchestration | Auto-chunking, adaptive parallelism |
| **Security** | Native libs, mixed licensing | Audited codecs, SBOM, signature-verifiable builds |

### Use Cases

- **Production Media Pipelines**: High-volume asset preparation for e-commerce, marketing, and publishing
- **Creative Automation**: Multi-step workflows with checkpointing and reproducible outputs
- **Developer Integration**: Embed conversion capabilities with SDK support across Python, Node.js, and PowerShell

### Instant Conversions (no recipe)

Use the streamlined quick-convert interface when you just need to re-encode a handful of files:

```bash
# Single file, write result alongside current directory
bunker-convert input.jpg to webp

# Multiple files, send all outputs to a specific folder (created if needed)
b-convert image1.png image2.png to jpeg to ./converted

# Alias binaries mirror bunker-convert: b-convert, bconvert, bcvrt
```

- Accepts one or more input paths (globs are supported by your shell)
- Supports an optional trailing `to <output_dir>` segment
- Renders a live progress bar showing `current/total` inputs and stage status
- Produces outputs named after the input stem with the requested extension

## Features

✨ **Declarative Recipes** – Human-readable YAML/JSON pipeline definitions with explicit stages

🚀 **GPU Acceleration** – CUDA, Metal, and Vulkan support with automatic CPU fallback

📊 **Quality Gates** – Enforce SSIM, PSNR, and MSE thresholds before outputs are finalized

🔍 **Observability** – Structured logging, Prometheus metrics, and OpenTelemetry tracing

🎨 **Multi-Format Support** – PNG, JPEG, WebP, AVIF, GIF, BMP, TIFF, ICO, PNM, HDR, DDS

🔒 **Reproducible Builds** – Lockfiles capture exact versions and parameters for deterministic results

🛡️ **Security-First** – Generate SBOM, SHA256 digests, and auditable supply chain artifacts

⚡ **Benchmarking** – Built-in quality metrics (PSNR/SSIM/MSE) and baseline comparisons

## Installation

### From Source (Rust/Cargo)

**Prerequisites**: [Rust toolchain](https://rustup.rs/) (1.70+)

```bash
# Clone the repository
git clone https://github.com/bunkercorporation/bunker-convert.git
cd bunker-convert

# Build with default features
cargo build --release

# Build with optional features
cargo build --release --features full  # All features
cargo build --release --features otel  # OpenTelemetry support
cargo build --release --features metrics-server  # Metrics HTTP server

# Install to PATH
cargo install --path .
```

**Available Features**:
- `otel` – OpenTelemetry tracing integration
- `metrics-server` – HTTP metrics server with Prometheus endpoint
- `full` – All optional features enabled

### Binary Releases

Pre-built binaries for Linux, macOS, and Windows will be available in [GitHub Releases](https://github.com/bunkercorporation/bunker-convert/releases).

### SDK Installation

#### Python

```bash
pip install bunker-convert-sdk
```

```python
from bunker_convert import run_recipe, lint_recipes
```

#### Node.js

```bash
npm install @bunkercorp/bunker-convert
```

```javascript
import { runRecipe, lintRecipes } from "@bunkercorp/bunker-convert";
```

#### PowerShell

```powershell
# Import the module (ensure bunker-convert binary is in PATH)
Import-Module ./sdk/powershell/BunkerConvert.psm1
```

## Quick Start

Create a simple image conversion recipe:

**`recipes/quickstart-webp.yaml`**
```yaml
version: 1
inputs:
  - path: "examples/quickstart/input/*.png"
pipeline:
  - stage: decode
  - stage: resize
    params:
      width: 256
      height: 256
      method: lanczos3
  - stage: encode
    params:
      format: webp
      lossless: false
      quality: 85
output:
  directory: "out/quickstart-webp"
  structure: "{stem}.webp"
quality_gates:
  - min_ssim: 0.94
    min_psnr: 30
```

Run the recipe:

```bash
bunker-convert run recipes/quickstart-webp.yaml
```

Output:
```
INFO Pipeline completed input=examples/quickstart/input/gradient.png output=out/quickstart-webp/gradient.webp
```

## Usage Guide

### Basic CLI Commands

```bash
# Run a recipe
bunker-convert run recipes/my-recipe.yaml

# Validate a recipe without execution
bunker-convert validate recipes/my-recipe.yaml

# Dry-run (validate and show plan)
bunker-convert run recipes/my-recipe.yaml --dry-run

# List all available stages
bunker-convert list-stages

# Generate a lockfile for reproducibility
bunker-convert lock recipes/my-recipe.yaml recipes/my-recipe.lock
```

### Instant Conversions (no recipe)

Use the streamlined quick-convert interface when you just need to re-encode a handful of files:

```bash
# Single file, write result alongside current directory
bunker-convert input.jpg to webp

# Multiple files, send all outputs to a specific folder (created if needed)
b-convert image1.png image2.png to jpeg to ./converted

# Alias binaries mirror bunker-convert: b-convert, bconvert, bcvrt
```

- Accepts one or more input paths (globs are supported by your shell)
- Supports an optional trailing `to <output_dir>` segment
- Renders a live progress bar showing `current/total` inputs and stage status
- Produces outputs named after the input stem with the requested extension

### Recipe Structure

A recipe defines inputs, pipeline stages, outputs, and optional quality gates:

```yaml
version: 1

# Input files (supports glob patterns)
inputs:
  - path: "./images/**/*.png"
  - path: "./photos/*.jpg"

# Processing pipeline (executed sequentially)
pipeline:
  # Decode stage - loads image into memory
  - stage: decode
    params:
      format: png  # Optional: hint the format

  # Annotate stage - add custom metadata
  - stage: annotate
    params:
      key: "batch_id"
      value: "2024-001"

  # Resize stage - transform dimensions
  - stage: resize
    params:
      width: 1920
      height: 1080
      fit: cover        # Options: cover, inside, exact
      method: lanczos3  # Options: nearest, triangle, catmullrom, lanczos3, gaussian

  # Encode stage - write to output format
  - stage: encode
    params:
      format: webp
      quality: 85
      lossless: false
      # Format-specific options:
      # JPEG: quality (1-100), icc_profile_path
      # PNG: compression (fast/default/best), filter (adaptive/none/sub/up/avg/paeth)
      # WebP: quality (0-100), lossless (bool)
      # AVIF: quality (1-100), speed (1-10), colorspace (srgb/bt709)
      # GIF: speed (1-30), repeat (infinite/count)

# Output configuration
output:
  directory: "./out"
  structure: "{stem}.{ext}"  # Supports {stem}, {ext}, and metadata keys

# Quality gates (optional)
quality_gates:
  - min_ssim: 0.95    # Structural Similarity Index (0-1)
    min_psnr: 35      # Peak Signal-to-Noise Ratio (dB)
    max_mse: 100      # Mean Squared Error
    label: "production"  # Optional label for reporting
```

### Available Stages

| Stage | Description | Required Parameters | Optional Parameters |
|-------|-------------|---------------------|---------------------|
| `decode` | Load image from bytes | - | `format` (format hint) |
| `annotate` | Add metadata to artifact | `key` | `value` (default: "true") |
| `resize` | Change image dimensions | `width`, `height` | `fit` (inside/cover/exact), `method` (filter type) |
| `encode` | Write image to format | - | `format`, `extension`, format-specific options |

### Advanced Features

#### Export Metrics

```bash
# Print metrics to console
bunker-convert run recipe.yaml --print-metrics

# Export metrics as JSON
bunker-convert run recipe.yaml --metrics-json metrics.json

# Export metrics in Prometheus format
bunker-convert run recipe.yaml --metrics-prometheus metrics.prom
```

#### OpenTelemetry Integration

```bash
# Send traces to OTLP endpoint (requires `otel` feature)
bunker-convert run recipe.yaml --otlp-endpoint http://localhost:4317
```

#### Metrics Server

```bash
# Start HTTP metrics server (requires `metrics-server` feature)
bunker-convert run recipe.yaml --metrics-listen 127.0.0.1:9090
```

#### Device Policy

```bash
# Force CPU execution
bunker-convert run recipe.yaml --device-policy cpu

# Force GPU execution
bunker-convert run recipe.yaml --device-policy gpu

# Auto-select based on heuristics (default)
bunker-convert run recipe.yaml --device-policy auto
```

## SDK Usage Examples

### Python

```python
from bunker_convert import run_recipe, lint_recipes

# Run a recipe
result = run_recipe(
    "recipes/quickstart-webp.yaml",
    bunker_convert_bin="bunker-convert",  # Optional: custom binary path
    extra_args={"device-policy": "cpu"},  # Optional: additional CLI args
    capture_output=True,
    check=True
)

print(result.stdout)

# Lint multiple recipes
lint_result = lint_recipes(
    ["recipes/recipe1.yaml", "recipes/recipe2.yaml"],
    check=True
)
```

### Node.js

```javascript
import { runRecipe, lintRecipes } from "@bunkercorp/bunker-convert";

// Run a recipe
const result = runRecipe("recipes/quickstart-webp.yaml", {
  bin: "bunker-convert",  // Optional: custom binary path
  extraArgs: ["--device-policy", "cpu"],  // Optional: additional args
  check: true
});

console.log(result.stdout);

// Lint recipes
const lintResult = lintRecipes([
  "recipes/recipe1.yaml",
  "recipes/recipe2.yaml"
], {
  check: true
});
```

### PowerShell

```powershell
Import-Module ./sdk/powershell/BunkerConvert.psm1

# Run a recipe
Invoke-BunkerConvertRecipe `
  -RecipePath "recipes/quickstart-webp.yaml" `
  -Binary "bunker-convert" `
  -AdditionalArguments @("--device-policy", "cpu")

# Lint recipes
Invoke-BunkerConvertRecipeLint `
  -RecipePaths @("recipes/recipe1.yaml", "recipes/recipe2.yaml") `
  -Binary "bunker-convert"
```

## Recipe Management

### Generate Preset Recipes

```bash
# Create a new recipe from preset
bunker-convert recipe new --preset quickstart-webp --output my-recipe.yaml
```

### Lint Multiple Recipes

```bash
# Validate multiple recipe files
bunker-convert recipe lint recipes/*.yaml
```

### Compare Recipes

```bash
# Show differences between two recipes
bunker-convert recipe diff recipes/v1.yaml recipes/v2.yaml
```

Output example:
```
Recipe differences between 'recipes/v1.yaml' and 'recipes/v2.yaml':
- Stage 2 ('resize') parameters differ: {"width":1920,"height":1080} vs {"width":2560,"height":1440}
- Quality gates differ: {"min_ssim":0.95} vs {"min_ssim":0.98}
```

## Benchmarking

Run quality benchmarks and compare against baseline:

```bash
# Run benchmark with quality metrics
bunker-convert bench run recipes/my-recipe.yaml \
  --inputs "examples/input/*.png" \
  --baseline ./baseline-outputs \
  --output-dir ./bench-outputs \
  --report bench-report.json \
  --label "experiment-001"

# View benchmark results
cat bench-report.json
```

Output includes:
- PSNR (Peak Signal-to-Noise Ratio)
- SSIM (Structural Similarity Index)
- MSE (Mean Squared Error)
- Processing time per input
- File size comparisons

## Security Features

### Generate Software Bill of Materials (SBOM)

```bash
bunker-convert security sbom --output sbom.json
```

### Compute SHA256 Digests

```bash
# Print digest to stdout
bunker-convert security digest --path target/release/bunker-convert

# Write digest to file
bunker-convert security digest \
  --path target/release/bunker-convert \
  --output bunker-convert.sha256
```

### Package for Distribution

```bash
# Build release and generate security artifacts (Unix)
./scripts/package.sh

# Build release and generate security artifacts (Windows)
.\scripts\package.ps1
```

This creates:
- Release binary in `target/release/`
- SBOM at `target/bunker-convert-sbom.json`
- SHA256 digest at `target/bunker-convert.sha256`

## Project Structure

```
bunker-convert/
├── src/                    # Core Rust source code
│   ├── main.rs            # CLI entry point and command handlers
│   ├── lib.rs             # Public library interface
│   ├── pipeline.rs        # Pipeline executor and stage registry
│   ├── recipe.rs          # Recipe parser and input expander
│   ├── stages/            # Built-in pipeline stages
│   │   └── mod.rs         # decode, annotate, resize, encode
│   ├── quality.rs         # Quality metrics (SSIM, PSNR, MSE)
│   ├── scheduler.rs       # Device scheduling (CPU/GPU)
│   ├── validation.rs      # Recipe validation logic
│   ├── benchmark.rs       # Benchmarking harness
│   ├── lockfile.rs        # Lockfile generation
│   ├── security.rs        # SBOM and digest generation
│   ├── presets.rs         # Preset recipe templates
│   └── observability/     # Metrics and tracing
│       ├── mod.rs
│       ├── otel.rs        # OpenTelemetry integration
│       └── server.rs      # Metrics HTTP server
├── recipes/               # Example recipe files
│   ├── quickstart-webp.yaml
│   ├── jpeg-to-webp.yaml
│   └── sample.yaml
├── sdk/                   # Language-specific SDKs
│   ├── python/           # Python wrapper
│   │   ├── bunker_convert/
│   │   │   └── __init__.py
│   │   └── pyproject.toml
│   ├── node/             # Node.js wrapper
│   │   ├── index.js
│   │   └── package.json
│   └── powershell/       # PowerShell module
│       └── BunkerConvert.psm1
├── tests/                # Integration tests
├── examples/             # Example input files
└── scripts/              # Build and packaging scripts
```

## Contributing

We welcome contributions! Here's how to get started:

### Development Setup

```bash
# Clone the repository
git clone https://github.com/bunkercorporation/bunker-convert.git
cd bunker-convert

# Build the project
cargo build

# Run tests
cargo test

# Run specific test
cargo test quickstart

# Run with logging
RUST_LOG=debug cargo run -- run recipes/quickstart-webp.yaml
```

### Running Tests

```bash
# Unit and integration tests
cargo test

# Run benchmarks
cargo bench

# Lint checks
cargo clippy -- -D warnings

# Format code
cargo fmt
```

### Adding New Stages

1. Define your stage in `src/stages/mod.rs` or a new module
2. Implement the `Stage` trait with `name()`, `supports_device()`, and `run()` methods
3. Register the stage in `register_defaults()` function
4. Add tests in `tests/` directory
5. Update documentation

Example stage structure:
```rust
struct MyCustomStage {
    // stage parameters
}

impl Stage for MyCustomStage {
    fn name(&self) -> &'static str {
        "my_stage"
    }

    fn supports_device(&self, device: StageDevice) -> bool {
        matches!(device, StageDevice::Cpu)
    }

    fn run(
        &self,
        artifact: &mut Artifact,
        ctx: &PipelineContext,
        device: StageDevice,
    ) -> Result<()> {
        // Transform artifact
        Ok(())
    }
}
```

### Code Structure

- **Pipeline Engine**: `src/pipeline.rs` – Stage registry, executor, and artifact management
- **Recipe System**: `src/recipe.rs` – YAML parsing and input expansion
- **Stages**: `src/stages/` – Built-in transformation stages
- **Quality Gates**: `src/quality.rs` – Image quality metrics computation
- **Scheduler**: `src/scheduler.rs` – CPU/GPU device selection
- **Observability**: `src/observability/` – Metrics collection and export

### Contribution Guidelines

- Write tests for new features
- Follow Rust naming conventions and idioms
- Update documentation for user-facing changes
- Keep commits atomic and well-described
- Ensure `cargo clippy` and `cargo fmt` pass

## License

See [Cargo.toml](./Cargo.toml) for license information.

## Credits

Built with the following open-source libraries:

- [image](https://github.com/image-rs/image) – Core image processing
- [webp](https://github.com/jaredforth/webp) – WebP encoding/decoding
- [clap](https://github.com/clap-rs/clap) – CLI argument parsing
- [serde](https://github.com/serde-rs/serde) – Serialization framework
- [tracing](https://github.com/tokio-rs/tracing) – Structured logging
- [opentelemetry](https://github.com/open-telemetry/opentelemetry-rust) – Observability

---

**bunker-convert** is developed by [Bunker Corporation](https://github.com/bunkercorporation)

