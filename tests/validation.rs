use std::fs;
use std::path::PathBuf;

use bunker_convert::lockfile::generate_lock;
use bunker_convert::pipeline::{OutputSpec, StageParameters, StageRegistry, StageSpec};
use bunker_convert::recipe::{InputSpec, Recipe};
use bunker_convert::stages;
use bunker_convert::validation::validate_recipe;
use serde_json::json;
use tempfile::tempdir;

fn build_registry() -> StageRegistry {
    let mut registry = StageRegistry::new();
    stages::register_defaults(&mut registry);
    registry
}

fn base_recipe(output_dir: PathBuf) -> Recipe {
    Recipe {
        version: 1,
        inputs: vec![InputSpec {
            path: "./examples/input/*.png".to_string(),
        }],
        pipeline: Vec::new(),
        output: OutputSpec {
            directory: output_dir,
            structure: "{stem}.{ext}".to_string(),
        },
        quality_gates: Vec::new(),
    }
}

fn stage_spec(name: &str, params: &[(&str, serde_json::Value)]) -> StageSpec {
    let mut map = StageParameters::default();
    for (key, value) in params {
        map.insert((*key).to_string(), value.clone());
    }
    StageSpec {
        stage: name.to_string(),
        params: Some(map),
    }
}

#[test]
fn validation_catches_missing_params() {
    let temp = tempdir().unwrap();
    let mut recipe = base_recipe(temp.path().join("out"));
    recipe.pipeline.push(StageSpec {
        stage: "resize".to_string(),
        params: Some(StageParameters::default()),
    });

    let registry = build_registry();
    let report = validate_recipe(&recipe, &registry);

    assert!(!report.is_ok());
    assert!(
        !report.errors.is_empty(),
        "expected validation errors but found none"
    );
}

#[test]
fn lockfile_generates_expected_yaml() {
    let temp = tempdir().unwrap();
    let output_dir = temp.path().join("out");
    let mut recipe = base_recipe(output_dir.clone());
    recipe.pipeline = vec![
        stage_spec("decode", &[("format", json!("text"))]),
        stage_spec(
            "encode",
            &[("extension", json!("txt")), ("format", json!("text"))],
        ),
    ];

    let lock_path = temp.path().join("pipeline.lock");
    generate_lock(&recipe, &lock_path).unwrap();

    let content = fs::read_to_string(&lock_path).unwrap();
    assert!(content.contains("recipe_version: 1"));
    assert!(content.contains("stages"));
    assert!(content.contains("params_hash"));
}
