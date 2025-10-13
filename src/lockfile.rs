use std::fs::File;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::pipeline::StageSpec;
use crate::recipe::Recipe;

#[derive(Debug, Serialize)]
pub struct PipelineLock {
    pub recipe_version: u32,
    pub generated_at: DateTime<Utc>,
    pub inputs: Vec<String>,
    pub output: OutputLock,
    pub stages: Vec<StageLock>,
}

#[derive(Debug, Serialize)]
pub struct OutputLock {
    pub directory: String,
    pub structure: String,
}

#[derive(Debug, Serialize)]
pub struct StageLock {
    pub name: String,
    pub params_hash: String,
}

pub fn generate_lock(recipe: &Recipe, path: &Path) -> Result<()> {
    let stages = recipe
        .pipeline
        .iter()
        .map(|spec| StageLock {
            name: spec.stage.clone(),
            params_hash: hash_params(spec),
        })
        .collect();

    let lock = PipelineLock {
        recipe_version: recipe.version,
        generated_at: Utc::now(),
        inputs: recipe.inputs.iter().map(|i| i.path.clone()).collect(),
        output: OutputLock {
            directory: recipe.output.directory.to_string_lossy().to_string(),
            structure: recipe.output.structure.clone(),
        },
        stages,
    };

    let file = File::create(path)
        .with_context(|| format!("Failed to create lockfile: {}", path.display()))?;
    serde_yaml::to_writer(file, &lock)
        .with_context(|| format!("Failed to write lockfile: {}", path.display()))?;

    Ok(())
}

fn hash_params(spec: &StageSpec) -> String {
    let mut hasher = Sha256::new();
    let value = serde_json::to_value(spec.params.clone().unwrap_or_default()).unwrap_or_default();
    let serialized = serde_json::to_vec(&value).unwrap_or_default();
    hasher.update(spec.stage.as_bytes());
    hasher.update(serialized);
    format!("{:x}", hasher.finalize())
}
