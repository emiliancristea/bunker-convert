//! Video processing primitives for bunker-convert.
//!
//! This module currently defines the core data structures that will power the
//! upcoming proprietary video pipeline. The initial implementation focuses on
//! data modelling so that pipeline stages can start exchanging rich video and
//! audio representations while the heavy lifting codecs are developed in
//! subsequent milestones.

pub mod container;
pub mod h264;

use std::time::Duration;

use serde::Serialize;

/// A decoded video frame.
#[derive(Debug, Clone, Serialize)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub pixel_format: PixelFormat,
    pub data: FramePlanes,
    pub timestamp: Duration,
    pub duration: Duration,
    pub keyframe: bool,
}

/// Supported planar buffer layouts.
#[derive(Debug, Clone, Serialize)]
pub enum FramePlanes {
    /// Packed RGB data (width * height * 3 bytes).
    Rgb(Vec<u8>),
    /// Packed RGBA data (width * height * 4 bytes).
    Rgba(Vec<u8>),
    /// YUV420 planar layout (Y plane followed by U and V planes).
    Yuv420 { y: Vec<u8>, u: Vec<u8>, v: Vec<u8> },
    /// YUV444 planar layout.
    Yuv444 { y: Vec<u8>, u: Vec<u8>, v: Vec<u8> },
    /// Placeholder for hardware backed surfaces (future work).
    ExternalHandle,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum PixelFormat {
    Rgb,
    Rgba,
    Yuv420,
    Yuv444,
    Unknown,
}

/// Audio PCM buffer.
#[derive(Debug, Clone, Serialize)]
pub struct AudioBuffer {
    pub sample_rate: u32,
    pub channel_layout: ChannelLayout,
    pub samples: Vec<f32>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum ChannelLayout {
    Mono,
    Stereo,
    Surround51,
    Surround71,
    Custom(u8),
}

/// A full set of media streams extracted from an input asset.
#[derive(Debug, Clone, Default, Serialize)]
pub struct MediaStreams {
    pub video: Option<VideoStream>,
    pub audio: Option<AudioStream>,
    pub subtitles: Vec<SubtitleStream>,
    pub duration: Option<Duration>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VideoStream {
    pub codec: VideoCodec,
    pub frame_rate: FrameRate,
    pub frames: Vec<VideoFrame>,
    pub color_space: ColorSpace,
}

#[derive(Debug, Clone, Serialize)]
pub struct AudioStream {
    pub codec: AudioCodec,
    pub buffers: Vec<AudioBuffer>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubtitleStream {
    pub codec: SubtitleCodec,
    pub cues: Vec<SubtitleCue>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubtitleCue {
    pub start: Duration,
    pub end: Duration,
    pub text: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum FrameRate {
    Constant { numerator: u32, denominator: u32 },
    Variable,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum ColorSpace {
    Bt601,
    Bt709,
    Bt2020,
    Srgb,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum VideoCodec {
    Raw,
    H264,
    H265,
    Vp9,
    Av1,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum AudioCodec {
    PcmF32,
    PcmS16,
    Aac,
    Opus,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum SubtitleCodec {
    Srt,
    WebVtt,
    Ass,
    Unknown,
}
