use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};

use crate::pipeline::{Artifact, OutputSpec, PipelineContext, Stage, StageParameters};
use crate::scheduler::StageDevice;
use crate::video::{self, MediaStreams};

pub struct VideoDecodeStage;

impl VideoDecodeStage {
    pub fn from_params(_params: StageParameters) -> Result<Self> {
        Ok(Self)
    }
}

impl Stage for VideoDecodeStage {
    fn name(&self) -> &'static str {
        "video_decode"
    }

    fn supports_device(&self, device: StageDevice) -> bool {
        matches!(device, StageDevice::Cpu)
    }

    fn run(
        &self,
        artifact: &mut Artifact,
        _ctx: &PipelineContext,
        _device: StageDevice,
    ) -> Result<()> {
        let mut media = match video::container::demux_media(&artifact.data) {
            Ok(streams) => streams,
            Err(_) => MediaStreams::default(),
        };
        if media.video.as_ref().map_or(true, |v| v.frames.is_empty()) {
            video::h264::decode_annex_b(&artifact.data, &mut media)
                .context("failed to decode H.264 Annex B stream")?;
        }

        let video_stream = media
            .video
            .as_ref()
            .ok_or_else(|| anyhow!("no decodable video stream found"))?;

        artifact.metadata.insert(
            "video.frame_count".to_string(),
            json!(video_stream.frames.len()),
        );
        if let Some(frame) = video_stream.frames.first() {
            artifact
                .metadata
                .insert("video.width".into(), json!(frame.width));
            artifact
                .metadata
                .insert("video.height".into(), json!(frame.height));
        }
        artifact.metadata.insert(
            "video.codec".into(),
            json!(format!("{:?}", video_stream.codec)),
        );
        artifact.media = media;
        Ok(())
    }
}

pub struct VideoEncodeStage {
    format: Option<String>,
    extension: Option<String>,
    _options: StageParameters,
}

impl VideoEncodeStage {
    pub fn from_params(mut params: StageParameters) -> Result<Self> {
        let format = take_string(&mut params, "format");
        let extension = take_string(&mut params, "extension");
        Ok(Self {
            format,
            extension,
            _options: params,
        })
    }
}

impl Stage for VideoEncodeStage {
    fn name(&self) -> &'static str {
        "video_encode"
    }

    fn supports_device(&self, device: StageDevice) -> bool {
        matches!(device, StageDevice::Cpu)
    }

    fn run(
        &self,
        artifact: &mut Artifact,
        ctx: &PipelineContext,
        _device: StageDevice,
    ) -> Result<()> {
        let video_stream = artifact
            .media()
            .video
            .as_ref()
            .ok_or_else(|| anyhow!("video_encode requires a decoded video stream"))?;

        let frame_count = video_stream.frames.len();

        let format = self.format.as_deref().unwrap_or("mp4").to_ascii_lowercase();
        let extension = self
            .extension
            .clone()
            .unwrap_or_else(|| default_extension(&format));

        let output_path = resolve_output_path(&ctx.output, artifact, &extension);
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create output directory: {}", parent.display())
            })?;
        }

        let buffer = artifact.data.clone();
        fs::write(&output_path, &buffer)
            .with_context(|| format!("failed to write encoded video: {}", output_path.display()))?;

        artifact.replace_data(buffer);
        artifact.metadata.insert(
            "video.output_path".into(),
            Value::String(output_path.to_string_lossy().to_string()),
        );
        artifact
            .metadata
            .insert("video.output.format".into(), Value::String(format.clone()));
        artifact
            .metadata
            .insert("video.output.size_bytes".into(), json!(artifact.data.len()));
        artifact
            .metadata
            .insert("video.output.frame_count".into(), json!(frame_count));
        Ok(())
    }
}

fn resolve_output_path(spec: &OutputSpec, artifact: &Artifact, extension: &str) -> PathBuf {
    let mut file_name = spec.structure.clone();
    file_name = file_name.replace("{stem}", &artifact.stem);
    file_name = file_name.replace("{ext}", extension);

    for (key, value) in artifact.metadata.iter() {
        if let Some(as_str) = value.as_str() {
            let placeholder = format!("{{{}}}", key);
            file_name = file_name.replace(&placeholder, as_str);
        }
    }

    let mut path = spec.directory.clone();
    path.push(file_name);
    path
}

fn default_extension(format: &str) -> String {
    match format {
        "mp4" => "mp4".to_string(),
        "annexb" | "h264" => "h264".to_string(),
        other => other.to_string(),
    }
}

fn take_string(params: &mut StageParameters, key: &str) -> Option<String> {
    params
        .remove(key)
        .and_then(|value| value.as_str().map(|s| s.to_string()))
}
