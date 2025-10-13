use std::path::PathBuf;

use bunker_convert::pipeline::{
    OutputSpec, StageParameters, StageRegistry, StageSpec, build_pipeline,
};
use bunker_convert::scheduler::DevicePolicy;
use bunker_convert::stages;
use image::{ImageBuffer, Rgba};
use serde_json::Value;
use tempfile::tempdir;

fn build_registry() -> StageRegistry {
    let mut registry = StageRegistry::new();
    stages::register_defaults(&mut registry);
    registry
}

fn build_stage_spec(name: &str, params: &[(&str, Value)]) -> StageSpec {
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
fn pipeline_executes_and_writes_output() {
    let temp = tempdir().unwrap();
    let input_path = temp.path().join("input.png");
    let image: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(8, 4, Rgba([255, 0, 0, 255]));
    image.save(&input_path).expect("failed to save test image");

    let output_dir = temp.path().join("out");
    let output_spec = OutputSpec {
        directory: output_dir.clone(),
        structure: "{stem}.{ext}".to_string(),
    };

    let registry = build_registry();
    let stages = vec![
        build_stage_spec("decode", &[("format", Value::String("png".to_string()))]),
        build_stage_spec(
            "resize",
            &[("width", Value::from(4)), ("height", Value::from(2))],
        ),
        build_stage_spec("encode", &[("format", Value::String("png".to_string()))]),
    ];

    let executor = build_pipeline(
        &registry,
        &stages,
        output_spec,
        Vec::new(),
        DevicePolicy::CpuOnly,
    )
    .unwrap();
    let results = executor.execute(&[input_path.clone()]).unwrap();

    assert_eq!(results.len(), 1);
    let result = &results[0];
    let expected_output = output_dir.join("input.png");
    assert_eq!(PathBuf::from(&result.output), expected_output);
    assert_eq!(
        result.metadata.get("image.width").and_then(Value::as_u64),
        Some(4)
    );
    assert_eq!(
        result.metadata.get("image.height").and_then(Value::as_u64),
        Some(2)
    );
    assert_eq!(
        result.metadata.get("output.format").and_then(Value::as_str),
        Some("png")
    );

    let snapshot = executor.metrics().snapshot();
    assert!(snapshot.total_duration_ms >= 0.0);
    let decode_metrics = snapshot.stages.get("decode").unwrap();
    assert_eq!(decode_metrics.calls, 1);
    let resize_metrics = snapshot.stages.get("resize").unwrap();
    assert_eq!(resize_metrics.calls, 1);
    let encode_metrics = snapshot.stages.get("encode").unwrap();
    assert_eq!(encode_metrics.calls, 1);
    let prom = snapshot.to_prometheus();
    assert!(prom.contains("bunker_stage_calls_total{stage=\"decode\"}"));
    assert!(prom.contains("bunker_quality_passes_total"));
}
