use anyhow::{Context, Result};
use serde::Serialize;

use crate::pipeline::{StageRegistry, StageSpec};
use crate::recipe::Recipe;

#[derive(Debug, Default, Serialize)]
pub struct ValidationReport {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ValidationReport {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn merge(&mut self, other: ValidationReport) {
        self.errors.extend(other.errors);
        self.warnings.extend(other.warnings);
    }
}

pub fn validate_recipe(recipe: &Recipe, registry: &StageRegistry) -> ValidationReport {
    let mut report = ValidationReport::default();

    if recipe.version != 1 {
        report
            .errors
            .push(format!("Unsupported recipe version: {}", recipe.version));
    }

    for (idx, input) in recipe.inputs.iter().enumerate() {
        if let Err(err) = glob::Pattern::new(&input.path) {
            report.errors.push(format!(
                "Input pattern {} ('{}') is not a valid glob: {}",
                idx + 1,
                input.path,
                err
            ));
        }
    }

    if recipe.pipeline.is_empty() {
        report
            .errors
            .push("Pipeline must contain at least one stage".into());
    }

    if recipe.inputs.is_empty() {
        report
            .errors
            .push("At least one input pattern is required".into());
    } else {
        for spec in &recipe.inputs {
            if spec.path.trim().is_empty() {
                report
                    .errors
                    .push("Input path patterns cannot be empty".into());
            }
        }
    }

    if recipe.output.directory.as_os_str().is_empty() {
        report
            .errors
            .push("Output directory cannot be empty".into());
    }

    for (idx, stage) in recipe.pipeline.iter().enumerate() {
        report.merge(validate_stage_order(idx, stage, &recipe.pipeline));
        report.merge(
            validate_stage(stage, registry)
                .with_context(|| format!("Stage {} ('{}')", idx + 1, stage.stage))
                .unwrap_or_else(|err| ValidationReport {
                    errors: vec![err.to_string()],
                    warnings: vec![],
                }),
        );
    }

    report
}

fn validate_stage(stage: &StageSpec, registry: &StageRegistry) -> Result<ValidationReport> {
    let mut report = ValidationReport::default();

    let params = stage.params.clone().unwrap_or_default();
    if let Err(err) = registry.create(&stage.stage, params) {
        report
            .errors
            .push(err.context("Failed to instantiate stage").to_string());
    }

    Ok(report)
}

fn validate_stage_order(idx: usize, stage: &StageSpec, pipeline: &[StageSpec]) -> ValidationReport {
    let mut report = ValidationReport::default();
    if stage.stage == "encode" {
        if idx == 0 {
            report.errors.push("Encode stage cannot be first".into());
        }
        let previous_has_decode = pipeline[..idx].iter().any(|prev| prev.stage == "decode");
        if !previous_has_decode {
            report
                .errors
                .push("Encode stage requires a decode stage earlier in the pipeline".into());
        }
    }
    if stage.stage == "quality" {
        let has_encode = pipeline[..idx].iter().any(|prev| prev.stage == "encode");
        if !has_encode {
            report.errors.push(
                "Quality evaluation stage must follow an encode stage to compare outputs".into(),
            );
        }
    }
    report
}
