//! Baseline H.264 (AVC) decoder skeleton.
//!
//! For the initial milestone we provide a parser that validates Annex B
//! bitstreams and extracts NAL units, while leaving the actual picture
//! reconstruction for subsequent iterations. The decoder records SPS/PPS
//! metadata and emits raw NAL unit lists for future processing.

use std::time::Duration;

use anyhow::{Result, bail};

use crate::video::{
    ColorSpace, FramePlanes, FrameRate, MediaStreams, PixelFormat, VideoCodec, VideoFrame,
    VideoStream,
};

#[derive(Debug)]
struct SequenceState {
    width: u32,
    height: u32,
    frame_rate: FrameRate,
}

impl Default for SequenceState {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            frame_rate: FrameRate::Constant {
                numerator: 30,
                denominator: 1,
            },
        }
    }
}

#[derive(Debug)]
struct NalUnit<'a> {
    nal_type: u8,
    payload: &'a [u8],
}

/// Parses Annex B H.264 bytestreams into a `VideoStream` with placeholder frames.
pub fn decode_annex_b(data: &[u8], streams: &mut MediaStreams) -> Result<()> {
    let nals = split_annex_b(data)?;
    let mut sequence = SequenceState::default();
    let mut frames = Vec::new();

    for nal in nals {
        match nal.nal_type {
            7 => {
                if let Err(err) = parse_sps(nal.payload, &mut sequence) {
                    tracing::warn!(error = %err, "failed to parse SPS");
                    sequence.width = sequence.width.max(640);
                    sequence.height = sequence.height.max(360);
                }
            }
            8 => parse_pps(nal.payload)?,
            5 | 1 => {
                if sequence.width == 0 {
                    sequence.width = 640;
                }
                if sequence.height == 0 {
                    sequence.height = 360;
                }
                let frame_duration = frame_duration(sequence.frame_rate);
                let frame = VideoFrame {
                    width: sequence.width.max(1),
                    height: sequence.height.max(1),
                    pixel_format: PixelFormat::Yuv420,
                    data: FramePlanes::Yuv420 {
                        y: Vec::new(),
                        u: Vec::new(),
                        v: Vec::new(),
                    },
                    timestamp: Duration::from_secs(0),
                    duration: frame_duration,
                    keyframe: nal.nal_type == 5,
                };
                frames.push(frame);
            }
            _ => {}
        }
    }

    if sequence.width == 0 {
        sequence.width = 640;
    }
    if sequence.height == 0 {
        sequence.height = 360;
    }

    if frames.is_empty() {
        bail!("no video frames decoded");
    }

    streams.video = Some(VideoStream {
        codec: VideoCodec::H264,
        frame_rate: sequence.frame_rate,
        frames,
        color_space: ColorSpace::Bt709,
    });
    Ok(())
}

fn split_annex_b(data: &[u8]) -> Result<Vec<NalUnit<'_>>> {
    let mut units = Vec::new();
    let mut i = 0;
    while i + 3 < data.len() {
        if &data[i..i + 3] == [0, 0, 1] {
            let start = i + 3;
            i = start;
            while i + 3 < data.len() && &data[i..i + 3] != [0, 0, 1] {
                i += 1;
            }
            let end = i;
            if end > start {
                let header = data[start];
                let nal_type = header & 0x1F;
                units.push(NalUnit {
                    nal_type,
                    payload: &data[start..end],
                });
            }
        } else if i + 4 < data.len() && &data[i..i + 4] == [0, 0, 0, 1] {
            i += 1; // normalize to 3-byte start code path
            continue;
        } else {
            i += 1;
        }
    }
    if units.is_empty() {
        bail!("no NAL units found");
    }
    Ok(units)
}

fn parse_sps(payload: &[u8], sequence: &mut SequenceState) -> Result<()> {
    let rbsp = remove_emulation_prevention(payload);
    let mut reader = BitReader::new(&rbsp);
    let _profile_idc = reader.read_bits(8)?;
    let _constraint = reader.read_bits(8)?;
    let _level_idc = reader.read_bits(8)?;
    let _seq_parameter_set_id = reader.read_ue()?;

    let chroma_format_idc = reader.read_ue()?;
    if chroma_format_idc == 3 {
        reader.read_bits(1)?; // separate_colour_plane_flag
    }
    let _bit_depth_luma_minus8 = reader.read_ue()?;
    let _bit_depth_chroma_minus8 = reader.read_ue()?;
    let _qpprime_y_zero_transform_bypass_flag = reader.read_bits(1)?;
    let seq_scaling_matrix_present_flag = reader.read_bits(1)?;
    if seq_scaling_matrix_present_flag == 1 {
        // skip scaling lists
        for i in 0..8 {
            let present = reader.read_bits(1)?;
            if present == 1 {
                skip_scaling_list(&mut reader, if i < 6 { 16 } else { 64 })?;
            }
        }
    }

    let _log2_max_frame_num_minus4 = reader.read_ue()?;
    let _pic_order_cnt_type = reader.read_ue()?;
    if _pic_order_cnt_type == 0 {
        reader.read_ue()?; // log2_max_pic_order_cnt_lsb_minus4
    }
    let _max_num_ref_frames = reader.read_ue()?;
    reader.read_bits(1)?; // gaps_in_frame_num_value_allowed_flag
    let pic_width_in_mbs_minus1 = reader.read_ue()?;
    let pic_height_in_map_units_minus1 = reader.read_ue()?;
    let frame_mbs_only_flag = reader.read_bits(1)?;
    if frame_mbs_only_flag == 0 {
        reader.read_bits(1)?; // mb_adaptive_frame_field_flag
    }
    reader.read_bits(1)?; // direct_8x8_inference_flag
    let frame_cropping_flag = reader.read_bits(1)?;
    let (crop_left, crop_right, crop_top, crop_bottom) = if frame_cropping_flag == 1 {
        (
            reader.read_ue()?,
            reader.read_ue()?,
            reader.read_ue()?,
            reader.read_ue()?,
        )
    } else {
        (0, 0, 0, 0)
    };

    let width_in_mbs = pic_width_in_mbs_minus1 + 1;
    let height_in_map_units = pic_height_in_map_units_minus1 + 1;
    let frame_height_in_mbs = if frame_mbs_only_flag == 1 {
        height_in_map_units
    } else {
        height_in_map_units * 2
    };
    let width = (width_in_mbs * 16) - 2 * (crop_left + crop_right);
    let height = (frame_height_in_mbs * 16) - 2 * (crop_top + crop_bottom);

    sequence.width = width as u32;
    sequence.height = height as u32;
    sequence.frame_rate = FrameRate::Constant {
        numerator: 30,
        denominator: 1,
    };
    Ok(())
}

fn parse_pps(payload: &[u8]) -> Result<()> {
    if payload.is_empty() {
        bail!("pps payload is empty");
    }
    Ok(())
}

fn remove_emulation_prevention(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if i + 2 < data.len() && data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 3 {
            out.push(0);
            out.push(0);
            i += 3;
        } else {
            out.push(data[i]);
            i += 1;
        }
    }
    out
}

fn skip_scaling_list(reader: &mut BitReader<'_>, size: usize) -> Result<()> {
    let mut last_scale = 8i32;
    let mut next_scale = 8i32;
    for _ in 0..size {
        if next_scale != 0 {
            let delta_scale = reader.read_se()?;
            next_scale = (last_scale + delta_scale + 256) % 256;
        }
        last_scale = if next_scale != 0 {
            next_scale
        } else {
            last_scale
        };
    }
    Ok(())
}

fn frame_duration(frame_rate: FrameRate) -> Duration {
    match frame_rate {
        FrameRate::Constant {
            numerator,
            denominator,
        } if numerator > 0 => {
            let seconds = denominator as f64 / numerator as f64;
            Duration::from_secs_f64(seconds)
        }
        _ => Duration::from_secs_f64(1.0 / 30.0),
    }
}

struct BitReader<'a> {
    data: &'a [u8],
    bit_pos: usize,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, bit_pos: 0 }
    }

    fn read_bits(&mut self, count: usize) -> Result<u32> {
        if count == 0 {
            return Ok(0);
        }
        let mut value = 0u32;
        for _ in 0..count {
            let byte_pos = self.bit_pos / 8;
            if byte_pos >= self.data.len() {
                bail!("bitstream overread");
            }
            let bit_offset = 7 - (self.bit_pos % 8);
            let bit = (self.data[byte_pos] >> bit_offset) & 1;
            value = (value << 1) | (bit as u32);
            self.bit_pos += 1;
        }
        Ok(value)
    }

    fn read_ue(&mut self) -> Result<u32> {
        let mut zeros = 0;
        while self.read_bits(1)? == 0 {
            zeros += 1;
        }
        let value = if zeros > 0 {
            let suffix = self.read_bits(zeros as usize)?;
            (1 << zeros) - 1 + suffix
        } else {
            0
        };
        Ok(value)
    }

    fn read_se(&mut self) -> Result<i32> {
        let ue = self.read_ue()? as i32;
        let value = if ue % 2 == 0 { -(ue / 2) } else { (ue + 1) / 2 };
        Ok(value)
    }
}
