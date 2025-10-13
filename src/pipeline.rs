use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use image::DynamicImage;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tracing::{instrument, warn};

use crate::observability::MetricsCollector;
use crate::quality::{QualityMetrics, compute_metrics};
use crate::recipe::QualityGateSpec;
use crate::scheduler::{DevicePolicy, StageDevice, TaskScheduler};

#[derive(Debug, Clone, Deserialize)]
pub struct OutputSpec {
    pub directory: PathBuf,
    #[serde(default = "default_output_structure")]
    pub structure: String,
}

fn default_output_structure() -> String {
    "{stem}.{ext}".to_string()
}

#[derive(Debug)]
pub struct Artifact {
    pub input_path: PathBuf,
    pub stem: String,
    pub data: Vec<u8>,
    pub format: Option<String>,
    pub original_image: Option<DynamicImage>,
    pub image: Option<DynamicImage>,
    pub metadata: Map<String, Value>,
}

impl Artifact {
    pub fn load(input: &Path) -> Result<Self> {
        let data = fs::read(input)
            .with_context(|| format!("Failed to read input file: {}", input.display()))?;
        let stem = input
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "artifact".to_string());

        let mut metadata = Map::new();
        metadata.insert(
            "input_path".to_string(),
            Value::String(input.to_string_lossy().to_string()),
        );
        metadata.insert("stem".to_string(), Value::String(stem.clone()));

        Ok(Self {
            input_path: input.to_path_buf(),
            stem,
            data,
            format: None,
            original_image: None,
            image: None,
            metadata,
        })
    }

    pub fn set_format(&mut self, fmt: impl Into<String>) {
        self.format = Some(fmt.into());
    }

    pub fn replace_data(&mut self, data: Vec<u8>) {
        self.data = data;
    }

    pub fn set_image(&mut self, image: DynamicImage) {
        self.image = Some(image);
    }

    pub fn set_original_image(&mut self, image: DynamicImage) {
        self.original_image = Some(image);
    }
}

#[derive(Debug, Clone)]
pub struct PipelineContext {
    pub output: OutputSpec,
}

pub type StageParameters = Map<String, Value>;

pub trait Stage: Send + Sync {
    fn name(&self) -> &'static str;
    fn supports_device(&self, device: StageDevice) -> bool;
    fn run(
        &self,
        artifact: &mut Artifact,
        ctx: &PipelineContext,
        device: StageDevice,
    ) -> Result<()>;
}

type StageConstructor = Arc<dyn Fn(StageParameters) -> Result<Box<dyn Stage>> + Send + Sync>;

pub struct StageRegistry {
    factories: HashMap<String, StageConstructor>,
}

impl Default for StageRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl StageRegistry {
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    pub fn register<F>(&mut self, name: impl Into<String>, constructor: F)
    where
        F: Fn(StageParameters) -> Result<Box<dyn Stage>> + Send + Sync + 'static,
    {
        self.factories.insert(name.into(), Arc::new(constructor));
    }

    pub fn create(&self, name: &str, params: StageParameters) -> Result<Box<dyn Stage>> {
        let factory = self.factories.get(name).ok_or_else(|| {
            anyhow!(
                "Unknown stage '{}'. Available stages: {}",
                name,
                self.known_stages().join(", ")
            )
        })?;
        factory(params)
    }

    pub fn known_stages(&self) -> Vec<String> {
        let mut names: Vec<_> = self.factories.keys().cloned().collect();
        names.sort();
        names
    }
}

pub struct PipelineExecutor {
    stages: Vec<Box<dyn Stage>>,
    ctx: PipelineContext,
    metrics: MetricsCollector,
    quality_gates: Vec<QualityGateSpec>,
    scheduler: TaskScheduler,
}

impl PipelineExecutor {
    pub fn new(
        stages: Vec<Box<dyn Stage>>,
        output: OutputSpec,
        quality_gates: Vec<QualityGateSpec>,
        scheduler: TaskScheduler,
    ) -> Self {
        Self {
            stages,
            ctx: PipelineContext { output },
            metrics: MetricsCollector::new(),
            quality_gates,
            scheduler,
        }
    }

    #[instrument(skip(self, artifact))]
    pub fn process(&self, artifact: &mut Artifact) -> Result<()> {
        for stage in &self.stages {
            let span = tracing::span!(tracing::Level::DEBUG, "stage", stage = stage.name());
            let _span_guard = span.enter();
            let _timer = self.metrics.start_stage(stage.name());
            let requested = self.scheduler.select_device(stage.name());
            let device = if stage.supports_device(requested) {
                requested
            } else if requested == StageDevice::Gpu && stage.supports_device(StageDevice::Cpu) {
                tracing::debug!("Falling back to CPU device");
                StageDevice::Cpu
            } else if requested == StageDevice::Cpu
                && self.scheduler.gpu_available()
                && stage.supports_device(StageDevice::Gpu)
            {
                tracing::debug!("Promoting stage to GPU device");
                StageDevice::Gpu
            } else {
                bail!(
                    "Stage '{}' does not support requested device {:?}",
                    stage.name(),
                    requested
                );
            };
            tracing::debug!(?requested, ?device, "Dispatching stage");
            stage.run(artifact, &self.ctx, device)?;
        }
        Ok(())
    }

    pub fn execute(&self, inputs: &[PathBuf]) -> Result<Vec<PipelineResult>> {
        self.metrics.reset();
        let total_start = Instant::now();
        let mut results = Vec::new();
        for input in inputs {
            let mut artifact = Artifact::load(input)?;
            let artifact_span =
                tracing::span!(tracing::Level::DEBUG, "artifact", input = %input.display());
            let _artifact_guard = artifact_span.enter();
            self.process(&mut artifact)?;
            if let Some(metrics) = self.evaluate_quality_gates(&mut artifact)? {
                artifact
                    .metadata
                    .insert("quality.mse".to_string(), value_from_metric(metrics.mse));
                artifact
                    .metadata
                    .insert("quality.psnr".to_string(), value_from_metric(metrics.psnr));
                artifact
                    .metadata
                    .insert("quality.ssim".to_string(), value_from_metric(metrics.ssim));
            }
            let output_path = artifact
                .metadata
                .get("output_path")
                .and_then(|v| v.as_str())
                .map(PathBuf::from)
                .unwrap_or_else(|| self.ctx.output.directory.join(&artifact.stem));
            results.push(PipelineResult {
                input: input.clone(),
                output: output_path,
                metadata: artifact.metadata.clone(),
            });
        }

        self.metrics.record_total_duration(total_start.elapsed());

        Ok(results)
    }

    pub fn metrics(&self) -> MetricsCollector {
        self.metrics.clone()
    }

    fn evaluate_quality_gates(&self, artifact: &mut Artifact) -> Result<Option<QualityMetrics>> {
        if self.quality_gates.is_empty() {
            return Ok(None);
        }

        let reference = artifact
            .original_image
            .as_ref()
            .ok_or_else(|| anyhow!("Quality gates require an original decoded image"))?;
        if matches!(
            artifact
                .metadata
                .get("output.decode_supported")
                .and_then(Value::as_bool),
            Some(false)
        ) {
            warn!("Skipping quality gates: encoded output decoder unavailable");
            artifact
                .metadata
                .insert("quality.status".into(), Value::String("skipped".into()));
            return Ok(None);
        }
        let Some(candidate) = artifact.image.as_ref() else {
            warn!("Skipping quality gates: artifact image unavailable");
            artifact
                .metadata
                .insert("quality.status".into(), Value::String("skipped".into()));
            return Ok(None);
        };

        let metrics = compute_metrics(reference, candidate)?;
        let mut failure: Option<String> = None;

        for gate in &self.quality_gates {
            if let Some(min_ssim) = gate.min_ssim
                && metrics.ssim < min_ssim
            {
                failure = Some(format!(
                    "Quality gate '{}' failed: SSIM {:.5} < {:.5}",
                    gate.label.as_deref().unwrap_or("ssim"),
                    metrics.ssim,
                    min_ssim
                ));
                break;
            }
            if let Some(min_psnr) = gate.min_psnr
                && metrics.psnr < min_psnr
            {
                failure = Some(format!(
                    "Quality gate '{}' failed: PSNR {:.2} < {:.2}",
                    gate.label.as_deref().unwrap_or("psnr"),
                    metrics.psnr,
                    min_psnr
                ));
                break;
            }
            if let Some(max_mse) = gate.max_mse
                && metrics.mse > max_mse
            {
                failure = Some(format!(
                    "Quality gate '{}' failed: MSE {:.4} > {:.4}",
                    gate.label.as_deref().unwrap_or("mse"),
                    metrics.mse,
                    max_mse
                ));
                break;
            }
        }

        if let Some(reason) = failure {
            self.metrics.record_quality_failure();
            bail!(reason);
        } else {
            self.metrics.record_quality_pass();
        }

        Ok(Some(metrics))
    }
}

#[derive(Debug, Clone)]
pub struct PipelineResult {
    pub input: PathBuf,
    pub output: PathBuf,
    pub metadata: Map<String, Value>,
}

pub fn build_pipeline(
    stage_registry: &StageRegistry,
    stage_specs: &[StageSpec],
    output_spec: OutputSpec,
    quality_gates: Vec<QualityGateSpec>,
    device_policy: DevicePolicy,
) -> Result<PipelineExecutor> {
    let mut stages = Vec::with_capacity(stage_specs.len());
    for spec in stage_specs {
        let params = spec.params.clone().unwrap_or_default();
        let stage = stage_registry.create(&spec.stage, params)?;
        stages.push(stage);
    }

    let scheduler = TaskScheduler::new(device_policy);
    Ok(PipelineExecutor::new(
        stages,
        output_spec,
        quality_gates,
        scheduler,
    ))
}

fn value_from_metric(value: f64) -> Value {
    if value.is_finite() {
        json!(value)
    } else {
        Value::String(value.to_string())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct StageSpec {
    pub stage: String,
    #[serde(default)]
    pub params: Option<StageParameters>,
}
