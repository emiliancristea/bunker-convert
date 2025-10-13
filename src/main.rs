use std::fs::{self, File};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use bunker_convert::benchmark::{BenchmarkOptions, run_benchmark};
use bunker_convert::lockfile::generate_lock;
use bunker_convert::observability::log_snapshot;
#[cfg(feature = "metrics-server")]
use bunker_convert::observability::server::MetricsServer;
use bunker_convert::pipeline::{StageRegistry, build_pipeline};
use bunker_convert::presets::generate_preset;
use bunker_convert::recipe::Recipe;
use bunker_convert::scheduler::DevicePolicy;
use bunker_convert::security::{compute_sha256, generate_sbom, write_sha256};
use bunker_convert::stages;
use bunker_convert::validation::validate_recipe;
use clap::{Parser, Subcommand};
use serde_json::to_writer_pretty;
use tracing::{error, info, warn};
use tracing_subscriber::{EnvFilter, prelude::*};

#[cfg(feature = "otel")]
use opentelemetry::KeyValue;
#[cfg(feature = "otel")]
use opentelemetry_otlp::WithExportConfig;
#[cfg(feature = "otel")]
use opentelemetry_sdk::{resource::Resource, trace as sdktrace};
#[cfg(feature = "metrics-server")]
use std::net::SocketAddr;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let otlp_endpoint_for_tracing = match &cli.command {
        Commands::Run { otlp_endpoint, .. } => otlp_endpoint.clone(),
        _ => None,
    };

    configure_tracing(otlp_endpoint_for_tracing.as_deref())?;

    let result = match cli.command {
        Commands::Run {
            recipe,
            dry_run,
            print_metrics,
            metrics_json,
            metrics_prometheus,
            metrics_listen,
            otlp_endpoint,
            device_policy,
        } => {
            let _ = otlp_endpoint; // already handled in tracing configuration
            run_recipe(
                recipe,
                dry_run,
                print_metrics,
                metrics_json,
                metrics_prometheus,
                metrics_listen,
                device_policy,
            )?
        }
        Commands::ListStages => {
            list_stages();
            ()
        }
        Commands::Validate { recipe } => {
            validate_recipe_cmd(recipe)?;
            ()
        }
        Commands::Lock { recipe, output } => {
            lock_recipe(recipe, output)?;
            ()
        }
        Commands::Recipe { action } => {
            recipe_command(action)?;
            ()
        }
        Commands::Bench { action } => {
            bench_command(action)?;
            ()
        }
        Commands::Security { action } => {
            security_command(action)?;
            ()
        }
    };

    #[cfg(feature = "otel")]
    if otlp_endpoint_for_tracing.is_some() {
        opentelemetry::global::shutdown_tracer_provider();
    }

    Ok(result)
}

fn configure_tracing(otlp_endpoint: Option<&str>) -> Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    #[cfg(feature = "otel")]
    {
        if let Some(endpoint) = otlp_endpoint {
            let tracer =
                opentelemetry_otlp::new_pipeline()
                    .tracing()
                    .with_trace_config(sdktrace::Config::default().with_resource(Resource::new(
                        vec![KeyValue::new("service.name", "bunker-convert")],
                    )))
                    .with_exporter(
                        opentelemetry_otlp::new_exporter()
                            .tonic()
                            .with_endpoint(endpoint),
                    )
                    .install_simple()?;

            tracing_subscriber::registry()
                .with(filter.clone())
                .with(tracing_subscriber::fmt::layer())
                .with(tracing_opentelemetry::layer().with_tracer(tracer))
                .try_init()
                .map_err(|err| anyhow!(err.to_string()))?;
        } else {
            tracing_subscriber::registry()
                .with(filter.clone())
                .with(tracing_subscriber::fmt::layer())
                .try_init()
                .map_err(|err| anyhow!(err.to_string()))?;
        }
    }

    #[cfg(not(feature = "otel"))]
    {
        if let Some(endpoint) = otlp_endpoint {
            eprintln!(
                "warning: --otlp-endpoint '{}' requested but OpenTelemetry support is not enabled. Rebuild with --features otel.",
                endpoint
            );
        }

        tracing_subscriber::registry()
            .with(filter.clone())
            .with(tracing_subscriber::fmt::layer())
            .try_init()
            .map_err(|err| anyhow!(err.to_string()))?;
    }

    Ok(())
}

fn run_recipe(
    recipe_path: PathBuf,
    dry_run: bool,
    print_metrics: bool,
    metrics_json: Option<PathBuf>,
    metrics_prometheus: Option<PathBuf>,
    metrics_listen: Option<String>,
    device_policy: DevicePolicy,
) -> Result<()> {
    let recipe = Recipe::load(&recipe_path)?;
    let registry = build_registry();

    if dry_run {
        info!(
            "Loaded recipe with {} stage(s). Available inputs: {:?}",
            recipe.pipeline.len(),
            recipe.inputs.iter().map(|i| &i.path).collect::<Vec<_>>()
        );
        return Ok(());
    }

    let inputs = recipe.expand_inputs()?;
    if inputs.is_empty() {
        warn!("No inputs resolved for recipe. Nothing to process.");
        return Ok(());
    }

    let executor = build_pipeline(
        &registry,
        &recipe.pipeline,
        recipe.output.clone(),
        recipe.quality_gates.clone(),
        device_policy,
    )?;

    let metrics_handle = executor.metrics();

    #[cfg(feature = "metrics-server")]
    let metrics_server = if let Some(addr_str) = metrics_listen {
        let addr: SocketAddr = addr_str
            .parse()
            .with_context(|| format!("Invalid metrics listen address: {addr_str}"))?;
        Some(MetricsServer::start(addr, metrics_handle.clone())?)
    } else {
        None
    };

    #[cfg(not(feature = "metrics-server"))]
    if let Some(addr_str) = metrics_listen {
        warn!(
            "Metrics server feature not enabled; ignoring --metrics-listen={}.",
            addr_str
        );
    }

    let results = executor.execute(&inputs)?;

    for result in results {
        info!(
            input = %result.input.display(),
            output = %result.output.display(),
            "Pipeline completed"
        );
    }

    if print_metrics || metrics_json.is_some() || metrics_prometheus.is_some() {
        let snapshot = metrics_handle.snapshot();
        if print_metrics {
            log_snapshot(&snapshot);
        }
        if let Some(path) = metrics_json {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent).with_context(|| {
                        format!("Failed to create metrics directory: {}", parent.display())
                    })?;
                }
            }
            let file = File::create(&path)
                .with_context(|| format!("Failed to create metrics file: {}", path.display()))?;
            to_writer_pretty(file, &snapshot)
                .with_context(|| format!("Failed to write metrics JSON: {}", path.display()))?;
            info!(metrics = %path.display(), "Metrics JSON written");
        }
        if let Some(path) = metrics_prometheus {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent).with_context(|| {
                        format!("Failed to create metrics directory: {}", parent.display())
                    })?;
                }
            }
            let content = snapshot.to_prometheus();
            std::fs::write(&path, content).with_context(|| {
                format!("Failed to write Prometheus metrics: {}", path.display())
            })?;
            info!(metrics = %path.display(), "Prometheus metrics written");
        }
    }

    #[cfg(feature = "metrics-server")]
    if let Some(mut server) = metrics_server {
        server.stop();
    }

    Ok(())
}

fn list_stages() {
    let registry = build_registry();
    println!("Available stages:");
    for name in registry.known_stages() {
        println!("- {name}");
    }
}

fn validate_recipe_cmd(recipe_path: PathBuf) -> Result<()> {
    let recipe = Recipe::load(&recipe_path)?;
    let registry = build_registry();
    let report = validate_recipe(&recipe, &registry);

    for warning in &report.warnings {
        warn!(file = %recipe_path.display(), "{warning}");
    }

    if report.is_ok() {
        info!(file = %recipe_path.display(), "Recipe validation passed");
        Ok(())
    } else {
        for error_msg in &report.errors {
            error!(file = %recipe_path.display(), "{error_msg}");
        }
        Err(anyhow!(
            "Recipe validation failed with {} error(s)",
            report.errors.len()
        ))
    }
}

fn lock_recipe(recipe_path: PathBuf, output_path: PathBuf) -> Result<()> {
    let recipe = Recipe::load(&recipe_path)?;
    let registry = build_registry();
    let report = validate_recipe(&recipe, &registry);

    for warning in &report.warnings {
        warn!(file = %recipe_path.display(), "{warning}");
    }

    if !report.is_ok() {
        for error_msg in &report.errors {
            error!(file = %recipe_path.display(), "{error_msg}");
        }
        return Err(anyhow!(
            "Cannot generate lockfile due to {} validation error(s)",
            report.errors.len()
        ));
    }

    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create lockfile directory: {}", parent.display())
            })?;
        }
    }

    generate_lock(&recipe, &output_path)?;
    info!(
        lockfile = %output_path.display(),
        "Lockfile generated successfully"
    );

    Ok(())
}

fn recipe_command(command: RecipeCommands) -> Result<()> {
    match command {
        RecipeCommands::New { preset, output } => {
            let destination =
                output.unwrap_or_else(|| PathBuf::from(format!("recipes/{preset}.yaml")));
            let generated = generate_preset(&preset, &destination)?;
            info!(
                preset = %preset,
                path = %generated.display(),
                "Preset recipe generated"
            );
            Ok(())
        }
        RecipeCommands::Lint { recipes } => lint_recipes(&recipes),
        RecipeCommands::Diff { lhs, rhs } => diff_recipes(&lhs, &rhs),
    }
}

fn bench_command(command: BenchCommands) -> Result<()> {
    match command {
        BenchCommands::Run {
            recipe,
            inputs,
            baseline,
            device_policy,
            output_dir,
            report,
            label,
        } => {
            let options = BenchmarkOptions {
                recipe_path: recipe.clone(),
                inputs_override: inputs,
                output_dir,
                baseline_dir: baseline.clone(),
                device_policy,
                dataset_label: label,
            };

            let report_data = run_benchmark(options)?;

            println!(
                "Benchmark processed {}/{} inputs",
                report_data.summary.processed, report_data.summary.total_inputs
            );
            if let Some(psnr) = report_data.summary.average_psnr {
                println!("Average PSNR: {:.2} dB", psnr);
            }
            if let Some(ssim) = report_data.summary.average_ssim {
                println!("Average SSIM: {:.4}", ssim);
            }
            if let Some(mse) = report_data.summary.average_mse {
                println!("Average MSE: {:.6}", mse);
            }

            for entry in &report_data.entries {
                for note in &entry.notes {
                    warn!(
                        input = %entry.input.display(),
                        output = %entry.output.display(),
                        "{note}"
                    );
                }
            }

            if let Some(path) = report {
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent).with_context(|| {
                            format!("Failed to create report directory: {}", parent.display())
                        })?;
                    }
                }
                let file = File::create(&path)
                    .with_context(|| format!("Failed to create report file: {}", path.display()))?;
                to_writer_pretty(file, &report_data)
                    .with_context(|| format!("Failed to write report JSON: {}", path.display()))?;
                info!(report = %path.display(), "Benchmark report written");
            }

            Ok(())
        }
    }
}

fn lint_recipes(recipes: &[PathBuf]) -> Result<()> {
    if recipes.is_empty() {
        bail!("No recipe files supplied for linting");
    }

    let registry = build_registry();
    let mut failures = 0usize;

    for recipe_path in recipes {
        match Recipe::load(recipe_path) {
            Ok(recipe) => {
                let report = validate_recipe(&recipe, &registry);
                for warning in &report.warnings {
                    warn!(file = %recipe_path.display(), "{warning}");
                }
                if report.is_ok() {
                    info!(file = %recipe_path.display(), "Lint passed");
                } else {
                    failures += 1;
                    for error_msg in &report.errors {
                        error!(file = %recipe_path.display(), "{error_msg}");
                    }
                }
            }
            Err(err) => {
                failures += 1;
                error!(file = %recipe_path.display(), "Failed to load recipe: {err}");
            }
        }
    }

    if failures > 0 {
        bail!("Lint failed for {failures} recipe(s)");
    }

    info!("All recipe lint checks passed");
    Ok(())
}

fn diff_recipes(lhs: &Path, rhs: &Path) -> Result<()> {
    let left = Recipe::load(lhs)?;
    let right = Recipe::load(rhs)?;

    let mut differences = Vec::new();

    if left.version != right.version {
        differences.push(format!(
            "Version mismatch: {} vs {}",
            left.version, right.version
        ));
    }

    let left_inputs: Vec<_> = left
        .inputs
        .iter()
        .map(|i| i.path.trim().to_string())
        .collect();
    let right_inputs: Vec<_> = right
        .inputs
        .iter()
        .map(|i| i.path.trim().to_string())
        .collect();
    if left_inputs != right_inputs {
        differences.push(format!(
            "Input patterns differ: {:?} vs {:?}",
            left_inputs, right_inputs
        ));
    }

    let min_len = left.pipeline.len().min(right.pipeline.len());
    if left.pipeline.len() != right.pipeline.len() {
        differences.push(format!(
            "Pipeline stage count differs: {} vs {}",
            left.pipeline.len(),
            right.pipeline.len()
        ));
    }

    for (idx, (l_stage, r_stage)) in left
        .pipeline
        .iter()
        .take(min_len)
        .zip(right.pipeline.iter())
        .enumerate()
    {
        if l_stage.stage != r_stage.stage {
            differences.push(format!(
                "Stage {} name differs: '{}' vs '{}'",
                idx + 1,
                l_stage.stage,
                r_stage.stage
            ));
        }
        let l_params = l_stage.params.clone().unwrap_or_default();
        let r_params = r_stage.params.clone().unwrap_or_default();
        if l_params != r_params {
            differences.push(format!(
                "Stage {} ('{}') parameters differ: {} vs {}",
                idx + 1,
                l_stage.stage,
                serde_json::to_string(&l_params).unwrap_or_else(|_| "<invalid>".into()),
                serde_json::to_string(&r_params).unwrap_or_else(|_| "<invalid>".into())
            ));
        }
    }

    if left.pipeline.len() > min_len {
        for (extra_idx, stage) in left.pipeline[min_len..].iter().enumerate() {
            differences.push(format!(
                "Extra stage in left recipe at position {}: '{}'",
                min_len + extra_idx + 1,
                stage.stage
            ));
        }
    }

    if right.pipeline.len() > min_len {
        for (extra_idx, stage) in right.pipeline[min_len..].iter().enumerate() {
            differences.push(format!(
                "Extra stage in right recipe at position {}: '{}'",
                min_len + extra_idx + 1,
                stage.stage
            ));
        }
    }

    if left.output.directory != right.output.directory {
        differences.push(format!(
            "Output directory differs: '{}' vs '{}'",
            left.output.directory.display(),
            right.output.directory.display()
        ));
    }

    if left.output.structure != right.output.structure {
        differences.push(format!(
            "Output structure differs: '{}' vs '{}'",
            left.output.structure, right.output.structure
        ));
    }

    let left_quality = serde_json::to_value(&left.quality_gates)?;
    let right_quality = serde_json::to_value(&right.quality_gates)?;
    if left_quality != right_quality {
        differences.push(format!(
            "Quality gates differ: {} vs {}",
            serde_json::to_string(&left_quality).unwrap_or_else(|_| "<invalid>".into()),
            serde_json::to_string(&right_quality).unwrap_or_else(|_| "<invalid>".into())
        ));
    }

    if differences.is_empty() {
        info!(
            left = %lhs.display(),
            right = %rhs.display(),
            "Recipes are equivalent"
        );
        println!("Recipes match: {} == {}", lhs.display(), rhs.display());
        Ok(())
    } else {
        println!(
            "Recipe differences between '{}' and '{}':",
            lhs.display(),
            rhs.display()
        );
        for diff in &differences {
            println!("- {diff}");
        }
        bail!("Recipes differ ({} difference(s) found)", differences.len());
    }
}

fn security_command(command: SecurityCommands) -> Result<()> {
    match command {
        SecurityCommands::Sbom { output } => {
            generate_sbom(&output)?;
            info!(sbom = %output.display(), "SBOM generated");
            Ok(())
        }
        SecurityCommands::Digest { path, output } => {
            if let Some(out_path) = output {
                let digest = write_sha256(&path, &out_path)?;
                println!("{}  {}", digest, path.display());
                info!(
                    file = %path.display(),
                    digest_output = %out_path.display(),
                    "SHA256 digest written"
                );
            } else {
                let digest = compute_sha256(&path)?;
                println!("{}  {}", digest, path.display());
                info!(file = %path.display(), "SHA256 computed");
            }
            Ok(())
        }
    }
}

fn build_registry() -> StageRegistry {
    let mut registry = StageRegistry::new();
    stages::register_defaults(&mut registry);
    registry
}

#[derive(Parser)]
#[command(
    name = "bunker-convert",
    version,
    about = "GPU-ready media pipeline toolkit (MVP)"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Run {
        recipe: PathBuf,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        print_metrics: bool,
        #[arg(long = "metrics-json")]
        metrics_json: Option<PathBuf>,
        #[arg(long = "metrics-prometheus")]
        metrics_prometheus: Option<PathBuf>,
        #[arg(long = "metrics-listen")]
        metrics_listen: Option<String>,
        #[arg(long = "otlp-endpoint")]
        otlp_endpoint: Option<String>,
        #[arg(long = "device-policy", value_enum, default_value_t = DevicePolicy::Auto)]
        device_policy: DevicePolicy,
    },
    ListStages,
    Validate {
        recipe: PathBuf,
    },
    Lock {
        recipe: PathBuf,
        output: PathBuf,
    },
    Recipe {
        #[command(subcommand)]
        action: RecipeCommands,
    },
    Bench {
        #[command(subcommand)]
        action: BenchCommands,
    },
    Security {
        #[command(subcommand)]
        action: SecurityCommands,
    },
}

#[derive(Subcommand)]
enum RecipeCommands {
    New {
        #[arg(long)]
        preset: String,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Lint {
        #[arg(required = true)]
        recipes: Vec<PathBuf>,
    },
    Diff {
        lhs: PathBuf,
        rhs: PathBuf,
    },
}

#[derive(Subcommand)]
enum BenchCommands {
    Run {
        recipe: PathBuf,
        #[arg(long)]
        inputs: Option<String>,
        #[arg(long)]
        baseline: Option<PathBuf>,
        #[arg(long = "device-policy", value_enum, default_value_t = DevicePolicy::Auto)]
        device_policy: DevicePolicy,
        #[arg(long = "output-dir")]
        output_dir: Option<PathBuf>,
        #[arg(long)]
        report: Option<PathBuf>,
        #[arg(long)]
        label: Option<String>,
    },
}

#[derive(Subcommand)]
enum SecurityCommands {
    Sbom {
        #[arg(long)]
        output: PathBuf,
    },
    Digest {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
    },
}
