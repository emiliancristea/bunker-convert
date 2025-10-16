use std::io::Write;
use std::path::Path;

use anyhow::Result;

use bunker_convert::pipeline::{
    Artifact, OutputSpec, PipelineContext, StageParameters, StageRegistry,
};
use bunker_convert::scheduler::StageDevice;
use bunker_convert::stages;

const ANNEX_B_SAMPLE: &[u8] = &[
    0x00, 0x00, 0x01, 0x67, 0x42, 0xE0, 0x1E, 0x8D, 0x68, 0x50, 0x1E, 0xD8, 0x08, 0x80, 0x00, 0x00,
    0x01, 0x68, 0xCE, 0x06, 0xE2, 0x00, 0x00, 0x01, 0x65, 0x88, 0x84, 0x21, 0xA0,
];

#[test]
fn video_decode_stage_produces_frames_from_annex_b() -> Result<()> {
    let tempdir = tempfile::tempdir()?;
    let mut temp_file = tempfile::NamedTempFile::new()?;
    temp_file.write_all(ANNEX_B_SAMPLE)?;

    let mut artifact = Artifact::load(temp_file.path())?;

    let mut registry = StageRegistry::new();
    stages::register_defaults(&mut registry);
    let stage = registry.create("video_decode", StageParameters::new())?;

    let ctx = PipelineContext {
        output: OutputSpec {
            directory: tempdir.path().to_path_buf(),
            structure: "{stem}.bin".to_string(),
        },
    };

    stage.run(&mut artifact, &ctx, StageDevice::Cpu)?;

    let video = artifact
        .media()
        .video
        .as_ref()
        .expect("video stream present");
    assert!(!video.frames.is_empty());
    assert_eq!(
        artifact.metadata.get("video.codec").unwrap().as_str(),
        Some("H264")
    );

    Ok(())
}

#[test]
fn video_encode_stage_writes_output_file() -> Result<()> {
    let tempdir = tempfile::tempdir()?;
    let mut temp_file = tempfile::NamedTempFile::new()?;
    temp_file.write_all(ANNEX_B_SAMPLE)?;

    let mut artifact = Artifact::load(temp_file.path())?;

    let mut registry = StageRegistry::new();
    stages::register_defaults(&mut registry);
    let decode = registry.create("video_decode", StageParameters::new())?;
    let encode = registry.create("video_encode", StageParameters::new())?;

    let ctx = PipelineContext {
        output: OutputSpec {
            directory: tempdir.path().to_path_buf(),
            structure: "{stem}.{ext}".to_string(),
        },
    };

    decode.run(&mut artifact, &ctx, StageDevice::Cpu)?;
    encode.run(&mut artifact, &ctx, StageDevice::Cpu)?;

    let output_path = artifact
        .metadata
        .get("video.output_path")
        .and_then(|value| value.as_str())
        .expect("output path recorded");
    assert!(Path::new(output_path).exists());
    Ok(())
}
