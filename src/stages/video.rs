use anyhow::{Context, Result, anyhow, bail};
use serde_json::json;

use crate::pipeline::{Artifact, PipelineContext, Stage, StageParameters};
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

/// Placeholder for the upcoming proprietary video encoder stage.
pub struct VideoEncodeStage;

impl VideoEncodeStage {
    pub fn from_params(_params: StageParameters) -> Result<Self> {
        Ok(Self)
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
        _artifact: &mut Artifact,
        _ctx: &PipelineContext,
        _device: StageDevice,
    ) -> Result<()> {
        bail!(
            "video_encode stage is not implemented yet. This is a placeholder for the upcoming video pipeline"
        )
    }
}
