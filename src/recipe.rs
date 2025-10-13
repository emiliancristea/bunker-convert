use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use glob::glob;
use serde::{Deserialize, Serialize};

use crate::pipeline::{OutputSpec, StageSpec};

#[derive(Debug, Deserialize)]
pub struct Recipe {
    pub version: u32,
    pub inputs: Vec<InputSpec>,
    pub pipeline: Vec<StageSpec>,
    pub output: OutputSpec,
    #[serde(default)]
    pub quality_gates: Vec<QualityGateSpec>,
}

impl Recipe {
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read recipe file: {}", path.display()))?;
        let recipe: Recipe = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse recipe YAML: {}", path.display()))?;
        Ok(recipe)
    }

    pub fn expand_inputs(&self) -> Result<Vec<PathBuf>> {
        let mut resolved = Vec::new();
        for input in &self.inputs {
            let matches = glob(&input.path)
                .with_context(|| format!("Invalid glob pattern: {}", input.path))?;
            let mut found = false;
            for entry in matches {
                let path = entry?;
                if path.is_file() {
                    resolved.push(path);
                    found = true;
                }
            }
            if !found {
                anyhow::bail!("No inputs matched pattern: {}", input.path);
            }
        }
        Ok(resolved)
    }
}

#[derive(Debug, Deserialize)]
pub struct InputSpec {
    pub path: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct QualityGateSpec {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub min_ssim: Option<f64>,
    #[serde(default)]
    pub min_psnr: Option<f64>,
    #[serde(default)]
    pub max_mse: Option<f64>,
}
