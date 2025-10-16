use anyhow::{Result, bail};

use crate::pipeline::{Artifact, PipelineContext, Stage, StageParameters};
use crate::scheduler::StageDevice;

/// Placeholder for the upcoming proprietary video decoder stage.
///
/// The stage is registered so that recipes can start referencing it, but it
/// currently returns an informative error until the full implementation lands.
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
        _artifact: &mut Artifact,
        _ctx: &PipelineContext,
        _device: StageDevice,
    ) -> Result<()> {
        bail!(
            "video_decode stage is not implemented yet. This is a placeholder for the upcoming video pipeline"
        )
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
