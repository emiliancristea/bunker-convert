use std::fs;
use std::path::PathBuf;

use bunker_convert::benchmark::{BenchmarkOptions, run_benchmark};
use bunker_convert::scheduler::DevicePolicy;
use image::{ImageBuffer, Rgba};
use tempfile::tempdir;

fn create_sample_images(dir: &PathBuf, count: usize) {
    fs::create_dir_all(dir).expect("create inputs dir");
    for idx in 0..count {
        let path = dir.join(format!("img{idx}.png"));
        let mut image: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(16, 16);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            let base = (idx as u8).saturating_mul(20);
            *pixel = Rgba([
                base.wrapping_add(x as u8 * 8),
                base.wrapping_add(y as u8 * 8),
                base.wrapping_add(((x + y) as u8).saturating_mul(4)),
                255,
            ]);
        }
        image.save(&path).expect("write sample image");
    }
}

#[test]
fn run_benchmark_produces_metrics() {
    let temp = tempdir().unwrap();
    let root = temp.path();

    let inputs_dir = root.join("inputs");
    create_sample_images(&inputs_dir, 2);

    let baseline_dir = root.join("baseline");
    fs::create_dir_all(&baseline_dir).unwrap();
    for entry in fs::read_dir(&inputs_dir).unwrap() {
        let entry = entry.unwrap();
        let file_name = entry.file_name();
        fs::copy(entry.path(), baseline_dir.join(file_name)).unwrap();
    }

    let recipe_path = root.join("recipe.yaml");
    let output_dir = root.join("outputs");
    let inputs_str = inputs_dir.to_string_lossy().replace('\\', "/");
    let outputs_str = output_dir.to_string_lossy().replace('\\', "/");
    let recipe_yaml = format!(
        r#"version: 1
inputs:
  - path: "{}/*.png"
pipeline:
  - stage: decode
  - stage: encode
    params:
      format: "png"
output:
  directory: "{}"
  structure: "{{stem}}.png"
"#,
        inputs_str, outputs_str
    );
    fs::write(&recipe_path, recipe_yaml).unwrap();

    let mut glob_path = inputs_dir.to_string_lossy().replace('\\', "/");
    glob_path.push_str("/*.png");

    let options = BenchmarkOptions {
        recipe_path: recipe_path.clone(),
        inputs_override: Some(glob_path),
        output_dir: Some(output_dir.clone()),
        baseline_dir: Some(baseline_dir.clone()),
        device_policy: DevicePolicy::CpuOnly,
        dataset_label: Some("unit-test".into()),
    };

    let report = run_benchmark(options).expect("benchmark run");
    assert_eq!(report.summary.total_inputs, 2);
    assert_eq!(report.summary.processed, 2);
    assert_eq!(report.summary.compared, 2);
    assert!(report.summary.average_ssim.unwrap() > 0.99);
    assert!(report.summary.average_psnr.unwrap() > 40.0);
    assert!(report.metrics.total_duration_ms >= 0.0);
    assert!(report.entries.iter().all(|entry| entry.metrics.is_some()));
    assert!(
        report
            .entries
            .iter()
            .flat_map(|entry| &entry.notes)
            .collect::<Vec<_>>()
            .is_empty()
    );
}
