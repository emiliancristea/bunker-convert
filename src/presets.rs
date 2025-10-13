use anyhow::{Context, Result};
use serde::Serialize;
use serde_yaml::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
struct PresetRecipe {
    version: u32,
    inputs: Vec<InputPattern>,
    pipeline: Vec<StageEntry>,
    output: OutputPreset,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    quality_gates: Vec<BTreeMap<String, Value>>,
}

#[derive(Debug, Clone, Serialize)]
struct StageEntry {
    stage: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<BTreeMap<String, Value>>,
}

#[derive(Debug, Clone, Serialize)]
struct InputPattern {
    path: String,
}

#[derive(Debug, Clone, Serialize)]
struct OutputPreset {
    directory: PathBuf,
    structure: String,
}

pub fn generate_preset(name: &str, destination: &Path) -> Result<PathBuf> {
    let preset = match name {
        "web" => web_preset(),
        "print" => print_preset(),
        "social" => social_preset(),
        other => anyhow::bail!("Unknown preset '{other}'"),
    };

    let rendered = serde_yaml::to_string(&preset)?;
    if let Some(parent) = destination.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }
    fs::write(destination, rendered)
        .with_context(|| format!("Failed to write preset recipe: {}", destination.display()))?;

    Ok(destination.to_path_buf())
}

fn web_preset() -> PresetRecipe {
    PresetRecipe {
        version: 1,
        inputs: vec![InputPattern {
            path: "./assets/**/*.png".into(),
        }],
        pipeline: vec![
            stage("decode", None),
            stage(
                "resize",
                Some(resize_params(1920, 1080, "inside", "lanczos3")),
            ),
            stage("encode", Some(encode_params("webp", Some("90")))),
        ],
        output: OutputPreset {
            directory: PathBuf::from("./out/web"),
            structure: "{stem}.webp".into(),
        },
        quality_gates: vec![gate(vec![
            ("label", val_str("web-quality")),
            ("min_ssim", val_f64(0.98)),
        ])],
    }
}

fn print_preset() -> PresetRecipe {
    PresetRecipe {
        version: 1,
        inputs: vec![InputPattern {
            path: "./assets/**/*.tif".into(),
        }],
        pipeline: vec![
            stage("decode", None),
            stage(
                "resize",
                Some(resize_params(4961, 3508, "cover", "lanczos3")),
            ),
            stage("encode", Some(encode_params("tiff", None))),
        ],
        output: OutputPreset {
            directory: PathBuf::from("./out/print"),
            structure: "{stem}.tiff".into(),
        },
        quality_gates: vec![gate(vec![
            ("label", val_str("print-quality")),
            ("min_ssim", val_f64(0.995)),
        ])],
    }
}

fn social_preset() -> PresetRecipe {
    PresetRecipe {
        version: 1,
        inputs: vec![InputPattern {
            path: "./assets/**/*.jpg".into(),
        }],
        pipeline: vec![
            stage("decode", None),
            stage(
                "resize",
                Some(resize_params(1080, 1080, "cover", "lanczos3")),
            ),
            stage("annotate", Some(text_overlay_params())),
            stage("encode", Some(encode_params("jpeg", Some("85")))),
        ],
        output: OutputPreset {
            directory: PathBuf::from("./out/social"),
            structure: "{stem}_social.jpg".into(),
        },
        quality_gates: vec![gate(vec![
            ("label", val_str("social-quality")),
            ("min_ssim", val_f64(0.97)),
        ])],
    }
}

fn stage(name: &str, params: Option<BTreeMap<String, Value>>) -> StageEntry {
    StageEntry {
        stage: name.into(),
        params,
    }
}

fn resize_params(width: u32, height: u32, fit: &str, method: &str) -> BTreeMap<String, Value> {
    let mut params = BTreeMap::new();
    params.insert("width".into(), val_u64(width as u64));
    params.insert("height".into(), val_u64(height as u64));
    params.insert("fit".into(), val_str(fit));
    params.insert("method".into(), val_str(method));
    params
}

fn encode_params(format: &str, quality: Option<&str>) -> BTreeMap<String, Value> {
    let mut params = BTreeMap::new();
    params.insert("format".into(), val_str(format));
    if let Some(q) = quality {
        params.insert("quality".into(), val_str(q));
    }
    params
}

fn text_overlay_params() -> BTreeMap<String, Value> {
    let mut params = BTreeMap::new();
    params.insert("key".into(), val_str("watermark"));
    params.insert("value".into(), val_str("#BUNKER"));
    params
}

fn gate(entries: Vec<(&str, Value)>) -> BTreeMap<String, Value> {
    entries.into_iter().map(|(k, v)| (k.into(), v)).collect()
}

fn val_str(value: &str) -> Value {
    Value::String(value.to_string())
}

fn val_f64(value: f64) -> Value {
    Value::from(value)
}

fn val_u64(value: u64) -> Value {
    Value::from(value)
}
