use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use image::DynamicImage;
use serde::Serialize;

use crate::observability::MetricsSnapshot;
use crate::pipeline::{PipelineExecutor, PipelineResult, StageRegistry, build_pipeline};
use crate::quality::{QualityMetrics, compute_metrics};
use crate::recipe::{InputSpec, Recipe};
use crate::scheduler::DevicePolicy;
use crate::stages;

#[derive(Debug)]
pub struct BenchmarkOptions {
    pub recipe_path: PathBuf,
    pub inputs_override: Option<String>,
    pub output_dir: Option<PathBuf>,
    pub baseline_dir: Option<PathBuf>,
    pub device_policy: DevicePolicy,
    pub dataset_label: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BenchmarkEntry {
    pub input: PathBuf,
    pub output: PathBuf,
    pub baseline: Option<PathBuf>,
    pub metrics: Option<QualityMetrics>,
    pub notes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct BenchmarkSummary {
    pub total_inputs: usize,
    pub processed: usize,
    pub compared: usize,
    pub average_psnr: Option<f64>,
    pub average_ssim: Option<f64>,
    pub average_mse: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct BenchmarkReport {
    pub recipe: PathBuf,
    pub dataset_label: Option<String>,
    pub baseline_dir: Option<PathBuf>,
    pub metrics: MetricsSnapshot,
    pub entries: Vec<BenchmarkEntry>,
    pub summary: BenchmarkSummary,
}

pub fn run_benchmark(options: BenchmarkOptions) -> Result<BenchmarkReport> {
    let mut recipe = Recipe::load(&options.recipe_path)?;

    if let Some(glob) = options.inputs_override {
        recipe.inputs = vec![InputSpec { path: glob }];
    }

    if let Some(dir) = &options.output_dir {
        recipe.output.directory = dir.clone();
    }

    let inputs = recipe.expand_inputs()?;
    if inputs.is_empty() {
        return Err(anyhow!("No inputs resolved for benchmark"));
    }

    let registry = build_registry();
    let executor = build_benchmark_executor(&registry, &recipe, options.device_policy.clone())?;

    let bench_start = Instant::now();
    let results = executor.execute(&inputs)?;
    let duration = bench_start.elapsed();
    let metrics_snapshot = executor.metrics().snapshot();

    let (entries, metrics_samples) = collect_entries(&results, options.baseline_dir.as_ref())?;

    let summary = summarize(&inputs, &results, &metrics_samples);

    let mut report = BenchmarkReport {
        recipe: options.recipe_path.clone(),
        dataset_label: options.dataset_label,
        baseline_dir: options.baseline_dir,
        metrics: metrics_snapshot,
        entries,
        summary,
    };

    // Attach total duration to metrics if not already set
    if report.metrics.total_duration_ms == 0.0 {
        report.metrics.total_duration_ms = duration.as_secs_f64() * 1_000.0;
    }

    Ok(report)
}

fn build_benchmark_executor(
    registry: &StageRegistry,
    recipe: &Recipe,
    device_policy: DevicePolicy,
) -> Result<PipelineExecutor> {
    build_pipeline(
        registry,
        &recipe.pipeline,
        recipe.output.clone(),
        recipe.quality_gates.clone(),
        device_policy,
    )
}

fn collect_entries(
    results: &[PipelineResult],
    baseline_dir: Option<&PathBuf>,
) -> Result<(Vec<BenchmarkEntry>, Vec<QualityMetrics>)> {
    let mut entries = Vec::with_capacity(results.len());
    let mut metrics_samples = Vec::new();

    for result in results {
        let mut notes = Vec::new();
        let baseline_path = match (baseline_dir, result.output.file_name()) {
            (Some(dir), Some(file_name)) => {
                let path = dir.join(file_name);
                Some(path)
            }
            _ => None,
        };

        let metrics = if let Some(path) = baseline_path.clone() {
            if path.exists() {
                let reference = load_image(&path)?;
                let candidate = load_image(&result.output)?;
                let metrics = compute_metrics(&reference, &candidate)?;
                metrics_samples.push(metrics.clone());
                Some(metrics)
            } else {
                notes.push(format!("Baseline missing: {}", path.display()));
                None
            }
        } else {
            None
        };

        entries.push(BenchmarkEntry {
            input: result.input.clone(),
            output: result.output.clone(),
            baseline: baseline_path,
            metrics,
            notes,
        });
    }

    Ok((entries, metrics_samples))
}

fn summarize(
    inputs: &[PathBuf],
    results: &[PipelineResult],
    samples: &[QualityMetrics],
) -> BenchmarkSummary {
    let total_inputs = inputs.len();
    let processed = results.len();
    let compared = samples.len();

    let (avg_psnr, avg_ssim, avg_mse) = if compared > 0 {
        let totals = samples.iter().fold((0.0, 0.0, 0.0), |acc, m| {
            (acc.0 + m.psnr, acc.1 + m.ssim, acc.2 + m.mse)
        });
        (
            Some(totals.0 / compared as f64),
            Some(totals.1 / compared as f64),
            Some(totals.2 / compared as f64),
        )
    } else {
        (None, None, None)
    };

    BenchmarkSummary {
        total_inputs,
        processed,
        compared,
        average_psnr: avg_psnr,
        average_ssim: avg_ssim,
        average_mse: avg_mse,
    }
}

fn build_registry() -> StageRegistry {
    let mut registry = StageRegistry::new();
    stages::register_defaults(&mut registry);
    registry
}

fn load_image(path: &Path) -> Result<DynamicImage> {
    let data =
        fs::read(path).with_context(|| format!("Failed to read image file: {}", path.display()))?;
    let format = image::guess_format(&data)
        .with_context(|| format!("Failed to detect format: {}", path.display()))?;
    image::load_from_memory_with_format(&data, format)
        .with_context(|| format!("Failed to decode image: {}", path.display()))
}
