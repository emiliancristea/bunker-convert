use std::path::PathBuf;

use bunker_convert::pipeline::{
    OutputSpec, StageParameters, StageRegistry, StageSpec, build_pipeline,
};
use bunker_convert::scheduler::DevicePolicy;
use bunker_convert::stages;
use image::{ImageBuffer, Rgba};
use serde_json::Value;
use tempfile::tempdir;

fn registry() -> StageRegistry {
    let mut registry = StageRegistry::new();
    stages::register_defaults(&mut registry);
    registry
}

fn stage(name: &str, params: &[(&str, Value)]) -> StageSpec {
    let mut map = StageParameters::default();
    for (key, value) in params {
        map.insert((*key).to_string(), value.clone());
    }
    StageSpec {
        stage: name.to_string(),
        params: Some(map),
    }
}

fn write_gradient(path: &PathBuf) {
    let mut image: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(24, 24);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let r = (x as u8).saturating_mul(10);
        let g = (y as u8).saturating_mul(10);
        let b = ((x + y) as u8).saturating_mul(5);
        *pixel = Rgba([r, g, b, 255]);
    }
    image.save(path).expect("failed to save gradient image");
}

#[test]
fn encode_jpeg_with_quality_metadata() {
    let temp = tempdir().unwrap();
    let input_path = temp.path().join("input.png");
    write_gradient(&input_path);

    let output_dir = temp.path().join("out");
    let output_spec = OutputSpec {
        directory: output_dir.clone(),
        structure: "{stem}.{ext}".into(),
    };

    let stages = vec![
        stage("decode", &[]),
        stage(
            "encode",
            &[
                ("format", Value::String("jpeg".into())),
                ("quality", Value::from(30)),
            ],
        ),
    ];

    let executor = build_pipeline(
        &registry(),
        &stages,
        output_spec,
        Vec::new(),
        DevicePolicy::CpuOnly,
    )
    .unwrap();
    let results = executor.execute(std::slice::from_ref(&input_path)).unwrap();
    let metadata = &results[0].metadata;
    assert_eq!(
        metadata.get("output.extension").and_then(Value::as_str),
        Some("jpg")
    );
    let quality = metadata
        .get("output.encoder.quality")
        .and_then(Value::as_f64)
        .expect("quality metadata");
    assert!((quality - 30.0).abs() < f64::EPSILON);
}

#[test]
fn encode_avif_records_speed_and_colorspace() {
    let temp = tempdir().unwrap();
    let input_path = temp.path().join("input.png");
    write_gradient(&input_path);

    let output_spec = OutputSpec {
        directory: temp.path().join("avif"),
        structure: "{stem}.{ext}".into(),
    };

    let stages = vec![
        stage("decode", &[]),
        stage(
            "encode",
            &[
                ("format", Value::String("avif".into())),
                ("quality", Value::from(60)),
                ("speed", Value::from(6)),
                ("colorspace", Value::String("bt709".into())),
            ],
        ),
    ];

    let executor = build_pipeline(
        &registry(),
        &stages,
        output_spec,
        Vec::new(),
        DevicePolicy::CpuOnly,
    )
    .unwrap();
    let results = executor.execute(std::slice::from_ref(&input_path)).unwrap();
    let metadata = &results[0].metadata;
    assert_eq!(
        metadata.get("output.extension").and_then(Value::as_str),
        Some("avif")
    );
    assert_eq!(
        metadata.get("output.encoder.speed").and_then(Value::as_u64),
        Some(6)
    );
    assert_eq!(
        metadata
            .get("output.encoder.colorspace")
            .and_then(Value::as_str),
        Some("bt709")
    );
}

#[test]
fn encode_webp_respects_lossless_flag() {
    let temp = tempdir().unwrap();
    let input_path = temp.path().join("input.png");
    write_gradient(&input_path);

    let output_spec = OutputSpec {
        directory: temp.path().join("webp"),
        structure: "{stem}.{ext}".into(),
    };

    let stages = vec![
        stage("decode", &[]),
        stage(
            "encode",
            &[
                ("format", Value::String("webp".into())),
                ("quality", Value::from(45)),
                ("lossless", Value::Bool(false)),
            ],
        ),
    ];

    let executor = build_pipeline(
        &registry(),
        &stages,
        output_spec,
        Vec::new(),
        DevicePolicy::CpuOnly,
    )
    .unwrap();
    let results = executor.execute(std::slice::from_ref(&input_path)).unwrap();
    let metadata = &results[0].metadata;
    assert_eq!(
        metadata.get("output.extension").and_then(Value::as_str),
        Some("webp")
    );
    assert_eq!(
        metadata
            .get("output.encoder.lossless")
            .and_then(Value::as_bool),
        Some(false)
    );
}

#[test]
fn encode_png_records_filter_and_compression() {
    let temp = tempdir().unwrap();
    let input_path = temp.path().join("input.png");
    write_gradient(&input_path);

    let output_spec = OutputSpec {
        directory: temp.path().join("png"),
        structure: "{stem}.{ext}".into(),
    };

    let stages = vec![
        stage("decode", &[]),
        stage(
            "encode",
            &[
                ("format", Value::String("png".into())),
                ("compression", Value::String("best".into())),
                ("filter", Value::String("paeth".into())),
            ],
        ),
    ];

    let executor = build_pipeline(
        &registry(),
        &stages,
        output_spec,
        Vec::new(),
        DevicePolicy::CpuOnly,
    )
    .unwrap();
    let results = executor.execute(std::slice::from_ref(&input_path)).unwrap();
    let metadata = &results[0].metadata;
    assert_eq!(
        metadata.get("output.extension").and_then(Value::as_str),
        Some("png")
    );
    assert_eq!(
        metadata
            .get("output.encoder.compression")
            .and_then(Value::as_str),
        Some("best")
    );
    assert_eq!(
        metadata
            .get("output.encoder.filter")
            .and_then(Value::as_str),
        Some("paeth")
    );
}
