use bunker_convert::pipeline::{
    OutputSpec, StageParameters, StageRegistry, StageSpec, build_pipeline,
};
use bunker_convert::recipe::QualityGateSpec;
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

fn save_test_image(path: &std::path::Path) {
    let mut img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(16, 16);
    for (x, y, pixel) in img.enumerate_pixels_mut() {
        let r = (x as u8).saturating_mul(16);
        let g = (y as u8).saturating_mul(16);
        let b = ((x + y) as u8).saturating_mul(8);
        *pixel = Rgba([r, g, b, 255]);
    }
    img.save(path).expect("failed to save fixture image");
}

#[test]
fn quality_gate_passes_for_lossless_pipeline() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("input.png");
    save_test_image(&input);

    let output_dir = temp.path().join("out");
    let output = OutputSpec {
        directory: output_dir.clone(),
        structure: "{stem}.{ext}".to_string(),
    };

    let gates = vec![QualityGateSpec {
        label: Some("baseline".into()),
        min_ssim: Some(0.999),
        min_psnr: Some(60.0),
        max_mse: Some(1e-6),
    }];

    let stages = vec![
        stage("decode", &[("format", Value::String("png".into()))]),
        stage("encode", &[("format", Value::String("png".into()))]),
    ];

    let executor =
        build_pipeline(&registry(), &stages, output, gates, DevicePolicy::CpuOnly).unwrap();
    let results = executor
        .execute(&[input])
        .expect("quality gate should pass");
    let metadata = &results[0].metadata;
    assert!(metadata.get("quality.ssim").is_some());
    assert!(metadata.get("quality.psnr").is_some());
    assert!(metadata.get("quality.mse").is_some());
    let snapshot = executor.metrics().snapshot();
    assert_eq!(snapshot.quality_passes, 1);
    assert_eq!(snapshot.quality_failures, 0);
}

#[test]
fn quality_gate_fails_for_lossy_output() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("input.png");
    save_test_image(&input);

    let output = OutputSpec {
        directory: temp.path().join("out"),
        structure: "{stem}.{ext}".to_string(),
    };

    let gates = vec![QualityGateSpec {
        label: Some("ssim-strict".into()),
        min_ssim: Some(0.9999999),
        min_psnr: Some(50.0),
        max_mse: None,
    }];

    let stages = vec![
        stage("decode", &[("format", Value::String("png".into()))]),
        stage(
            "resize",
            &[
                ("width", Value::from(32)),
                ("height", Value::from(32)),
                ("fit", Value::String("exact".into())),
                ("method", Value::String("nearest".into())),
            ],
        ),
        stage(
            "resize",
            &[
                ("width", Value::from(16)),
                ("height", Value::from(16)),
                ("fit", Value::String("exact".into())),
                ("method", Value::String("lanczos3".into())),
            ],
        ),
        stage("encode", &[("format", Value::String("jpeg".into()))]),
    ];

    let executor =
        build_pipeline(&registry(), &stages, output, gates, DevicePolicy::CpuOnly).unwrap();
    let err = executor
        .execute(&[input])
        .expect_err("quality gate should fail");
    assert!(err.to_string().contains("Quality gate"));
    let snapshot = executor.metrics().snapshot();
    assert_eq!(snapshot.quality_passes, 0);
    assert_eq!(snapshot.quality_failures, 1);
}
