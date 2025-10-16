use std::env;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use bunker_convert::benchmark::{BenchmarkOptions, run_benchmark};
use bunker_convert::lockfile::generate_lock;
use bunker_convert::observability::log_snapshot;
#[cfg(feature = "metrics-server")]
use bunker_convert::observability::server::MetricsServer;
use bunker_convert::pipeline::{
    OutputSpec, StageParameters, StageProgress, StageRegistry, StageSpec, build_pipeline,
};
use bunker_convert::presets::generate_preset;
use bunker_convert::recipe::{QualityGateSpec, Recipe};
use bunker_convert::scheduler::DevicePolicy;
use bunker_convert::security::{compute_sha256, generate_sbom, write_sha256};
use bunker_convert::stages;
use bunker_convert::validation::validate_recipe;
use clap::error::ErrorKind;
use clap::{CommandFactory, Parser, Subcommand, ValueHint};
use serde_json::Value;
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
    let Cli {
        command,
        quick_args,
    } = cli;

    if command.is_some() && !quick_args.is_empty() {
        Cli::command()
            .error(
                ErrorKind::ArgumentConflict,
                "Quick convert arguments cannot be combined with subcommands",
            )
            .exit();
    }

    let otlp_endpoint_for_tracing = command.as_ref().and_then(|command| match command {
        Commands::Run { otlp_endpoint, .. } => otlp_endpoint.clone(),
        _ => None,
    });

    configure_tracing(otlp_endpoint_for_tracing.as_deref())?;

    let command_result: Result<()> = if let Some(command) = command {
        match command {
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
                )
            }
            Commands::ListStages => {
                list_stages();
                Ok(())
            }
            Commands::Validate { recipe } => validate_recipe_cmd(recipe),
            Commands::Lock { recipe, output } => lock_recipe(recipe, output),
            Commands::Recipe { action } => recipe_command(action),
            Commands::Bench { action } => bench_command(action),
            Commands::Security { action } => security_command(action),
        }
    } else if quick_args.is_empty() {
        Cli::command().print_help()?;
        println!();
        Ok(())
    } else {
        quick_convert_from_args(quick_args)
    };

    #[cfg(feature = "otel")]
    if otlp_endpoint_for_tracing.is_some() {
        opentelemetry::global::shutdown_tracer_provider();
    }

    command_result
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
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("Failed to create metrics directory: {}", parent.display())
                })?;
            }
            let file = File::create(&path)
                .with_context(|| format!("Failed to create metrics file: {}", path.display()))?;
            to_writer_pretty(file, &snapshot)
                .with_context(|| format!("Failed to write metrics JSON: {}", path.display()))?;
            info!(metrics = %path.display(), "Metrics JSON written");
        }
        if let Some(path) = metrics_prometheus {
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("Failed to create metrics directory: {}", parent.display())
                })?;
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

fn quick_convert_from_args(args: Vec<String>) -> Result<()> {
    if args.is_empty() {
        bail!("Quick convert usage: bunker-convert <input> to <format> [to <output_dir>]");
    }

    let to_positions: Vec<usize> = args
        .iter()
        .enumerate()
        .filter_map(|(idx, arg)| arg.eq_ignore_ascii_case("to").then_some(idx))
        .collect();

    let (input_tokens, format_token, output_token) = if to_positions.is_empty() {
        if args.len() < 2 {
            bail!("Quick convert usage: bunker-convert <input> to <format> [to <output_dir>]");
        }
        let (inputs, format) = args.split_at(args.len() - 1);
        (inputs.to_vec(), format[0].clone(), None)
    } else {
        let first_to = to_positions[0];
        if first_to == 0 {
            bail!("Quick convert usage: bunker-convert <input> to <format> [to <output_dir>]");
        }
        let last_to = *to_positions.last().unwrap();
        if first_to == last_to {
            let format_slice = &args[first_to + 1..];
            if format_slice.len() != 1 {
                bail!("Quick convert usage: bunker-convert <input> to <format> [to <output_dir>]");
            }
            (args[..first_to].to_vec(), format_slice[0].clone(), None)
        } else {
            let format_slice = &args[first_to + 1..last_to];
            if format_slice.len() != 1 {
                bail!("Quick convert usage: bunker-convert <input> to <format> [to <output_dir>]");
            }
            let output_slice = &args[last_to + 1..];
            if output_slice.is_empty() {
                bail!("Output directory must follow the final 'to'");
            }
            if output_slice.len() > 1 {
                bail!("Output directory must be a single argument. Quote paths containing spaces.");
            }
            (
                args[..first_to].to_vec(),
                format_slice[0].clone(),
                Some(output_slice[0].clone()),
            )
        }
    };

    if input_tokens.is_empty() {
        bail!("At least one input file must be specified");
    }

    let inputs: Vec<PathBuf> = input_tokens.into_iter().map(PathBuf::from).collect();
    let output_dir = output_token.map(PathBuf::from);
    quick_convert(inputs, format_token, output_dir)
}

fn quick_convert(
    inputs: Vec<PathBuf>,
    target_format: String,
    output_dir: Option<PathBuf>,
) -> Result<()> {
    if inputs.is_empty() {
        bail!("At least one input file is required");
    }

    for input in &inputs {
        if !input.exists() {
            bail!("Input file '{}' not found", input.display());
        }
    }

    let normalized_format = target_format.trim().trim_start_matches('.').to_lowercase();
    if normalized_format.is_empty() {
        bail!("Output format must be a non-empty value");
    }

    let mode = classify_inputs(&inputs)?;

    let mut stages = Vec::with_capacity(2);
    match mode {
        QuickConvertKind::Image => {
            stages.push(StageSpec {
                stage: "decode".to_string(),
                params: None,
            });
            let mut encode_params = StageParameters::new();
            encode_params.insert(
                "format".to_string(),
                Value::String(normalized_format.clone()),
            );
            stages.push(StageSpec {
                stage: "encode".to_string(),
                params: Some(encode_params),
            });
        }
        QuickConvertKind::Video => {
            stages.push(StageSpec {
                stage: "video_decode".to_string(),
                params: None,
            });
            let mut encode_params = StageParameters::new();
            encode_params.insert(
                "format".to_string(),
                Value::String(normalized_format.clone()),
            );
            stages.push(StageSpec {
                stage: "video_encode".to_string(),
                params: Some(encode_params),
            });
        }
    }

    let registry = build_registry();

    let mut directory = if let Some(dir) = output_dir {
        if dir.is_absolute() {
            dir
        } else {
            env::current_dir()
                .context("Failed to determine current directory")?
                .join(dir)
        }
    } else {
        env::current_dir().context("Failed to determine current directory")?
    };

    if directory.exists() {
        if !directory.is_dir() {
            bail!("Output path '{}' is not a directory", directory.display());
        }
    } else {
        fs::create_dir_all(&directory).with_context(|| {
            format!("Failed to create output directory: {}", directory.display())
        })?;
    }

    if let Ok(canonical) = directory.canonicalize() {
        directory = canonical;
    }

    let output_spec = OutputSpec {
        directory,
        structure: format!("{{stem}}.{}", normalized_format),
    };

    let executor = build_pipeline(
        &registry,
        &stages,
        output_spec,
        Vec::<QualityGateSpec>::new(),
        DevicePolicy::Auto,
    )?;

    let total_inputs = inputs.len();
    let bar_width = 30usize;

    let progress_render = move |progress: StageProgress<'_>| {
        let current_input = progress.input_index + 1;
        let total_inputs = progress.total_inputs.max(1);
        let total_stages = progress.total_stages.max(1);
        let total_steps = total_inputs * total_stages;
        let completed_steps = progress
            .input_index
            .saturating_mul(total_stages)
            .saturating_add(progress.stage_index);
        let fraction = (completed_steps as f64 / total_steps as f64).clamp(0.0, 1.0);
        let filled =
            ((fraction * bar_width as f64).round() as isize).clamp(0, bar_width as isize) as usize;
        let empty = bar_width.saturating_sub(filled);
        let percent = (fraction * 100.0).round().clamp(0.0, 100.0) as i32;
        let mut stage_label = progress.stage_name.to_string();
        if stage_label.len() > 12 {
            stage_label.truncate(12);
        }
        print!(
            "\r{:>3}/{:<3} [{}{}] {:>3}% {:<12}",
            current_input,
            total_inputs,
            "=".repeat(filled),
            " ".repeat(empty),
            percent,
            stage_label
        );
        let _ = io::stdout().flush();
    };

    let results = executor.execute_with_progress(&inputs, progress_render)?;

    if results.len() != total_inputs {
        bail!(
            "Expected {} output(s) but produced {}",
            total_inputs,
            results.len()
        );
    }

    println!();
    println!("\x1b[32mConversion completed\x1b[0m");

    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuickConvertKind {
    Image,
    Video,
}

fn classify_inputs(inputs: &[PathBuf]) -> Result<QuickConvertKind> {
    if inputs.is_empty() {
        return Ok(QuickConvertKind::Image);
    }

    let first_is_video = is_video_path(&inputs[0]);
    for path in inputs.iter().skip(1) {
        let is_video = is_video_path(path);
        if is_video != first_is_video {
            bail!("Mixed image and video inputs are not supported by quick convert");
        }
    }

    Ok(if first_is_video {
        QuickConvertKind::Video
    } else {
        QuickConvertKind::Image
    })
}

fn is_video_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(is_video_extension)
        .unwrap_or(false)
}

fn is_video_extension(ext: &str) -> bool {
    let normalized = ext.trim_start_matches('.').to_lowercase();
    matches!(normalized.as_str(), "h264" | "264" | "annexb" | "avc")
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

    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("Failed to create lockfile directory: {}", parent.display())
        })?;
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
                if let Some(parent) = path.parent()
                    && !parent.as_os_str().is_empty()
                {
                    fs::create_dir_all(parent).with_context(|| {
                        format!("Failed to create report directory: {}", parent.display())
                    })?;
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
    command: Option<Commands>,
    #[arg(
        value_name = "INPUT",
        help = "Quick convert syntax: <INPUT> to <FORMAT>",
        value_hint = ValueHint::Other,
        num_args = 0..
    )]
    quick_args: Vec<String>,
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
