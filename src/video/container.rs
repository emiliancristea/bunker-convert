//! Minimal ISO-BMFF (MP4) container parser.
//!
//! The goal is to have a proprietary demuxer that can extract H.264 video and
//! PCM audio tracks for the first milestone. The implementation favours
//! clarity and correctness over absolute performance; optimization will follow
//! later.

use std::convert::TryInto;
use std::io::{Cursor, Read};

use anyhow::{Context, Result, anyhow, bail};

use crate::video::{
    AudioCodec, AudioStream, ColorSpace, FrameRate, MediaStreams, VideoCodec, VideoStream,
};

#[derive(Debug)]
pub struct Mp4Demuxer<'a> {
    cursor: Cursor<&'a [u8]>,
}

#[derive(Debug, Default)]
struct TrackCollector {
    video: Option<VideoTrack>,
    audio: Option<AudioTrack>,
}

#[derive(Debug)]
#[allow(dead_code)]
struct VideoTrack {
    codec: VideoCodec,
    width: u32,
    height: u32,
    timescale: u32,
    duration: u32,
    frame_count: u32,
}

#[derive(Debug)]
#[allow(dead_code)]
struct AudioTrack {
    codec: AudioCodec,
    sample_rate: u32,
    channels: u16,
    timescale: u32,
    duration: u32,
}

impl<'a> Mp4Demuxer<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            cursor: Cursor::new(data),
        }
    }

    pub fn demux(mut self) -> Result<MediaStreams> {
        let mut collector = TrackCollector::default();
        while let Some(atom) = read_atom(&mut self.cursor)? {
            match atom.kind.as_str() {
                "moov" => collect_moov(&atom.data, &mut collector)?,
                _ => {}
            }
        }

        let mut streams = MediaStreams::default();
        if let Some(video) = collector.video {
            streams.video = Some(VideoStream {
                codec: video.codec,
                frame_rate: FrameRate::Constant {
                    numerator: video.frame_count,
                    denominator: video.duration.max(1),
                },
                frames: Vec::new(),
                color_space: ColorSpace::Bt709,
            });
        }
        if let Some(audio) = collector.audio {
            streams.audio = Some(AudioStream {
                codec: audio.codec,
                buffers: Vec::new(),
            });
        }
        Ok(streams)
    }
}

#[derive(Debug)]
struct Atom<'a> {
    kind: String,
    data: &'a [u8],
}

fn read_atom<'a>(cursor: &mut Cursor<&'a [u8]>) -> Result<Option<Atom<'a>>> {
    if cursor.position() as usize >= cursor.get_ref().len() {
        return Ok(None);
    }

    let mut size_buf = [0u8; 4];
    cursor.read_exact(&mut size_buf).context("atom size")?;
    let size = u32::from_be_bytes(size_buf);
    if size < 8 {
        bail!("invalid atom size {size}");
    }

    let mut kind_buf = [0u8; 4];
    cursor.read_exact(&mut kind_buf).context("atom kind")?;
    let kind = std::str::from_utf8(&kind_buf).unwrap_or("????").to_string();

    let payload_len = (size as usize).saturating_sub(8);
    let start = cursor.position() as usize;
    let end = start
        .checked_add(payload_len)
        .ok_or_else(|| anyhow!("atom length overflow"))?;
    if end > cursor.get_ref().len() {
        bail!("atom payload exceeds buffer bounds");
    }

    let data = &cursor.get_ref()[start..end];
    cursor.set_position(end as u64);
    Ok(Some(Atom { kind, data }))
}

fn collect_moov(data: &[u8], collector: &mut TrackCollector) -> Result<()> {
    let mut cursor = Cursor::new(data);
    while let Some(atom) = read_atom(&mut cursor)? {
        if atom.kind == "trak" {
            collect_trak(atom.data, collector)?;
        }
    }
    Ok(())
}

fn collect_trak(data: &[u8], collector: &mut TrackCollector) -> Result<()> {
    let mut cursor = Cursor::new(data);
    let mut tkhd_timescale = None;
    let mut tkhd_duration = None;
    let mut mdia_data = None;

    while let Some(atom) = read_atom(&mut cursor)? {
        match atom.kind.as_str() {
            "tkhd" => {
                let version = atom
                    .data
                    .first()
                    .copied()
                    .ok_or_else(|| anyhow!("tkhd missing version"))?;
                let (duration_offset, timescale_offset) = if version == 1 {
                    (28usize, 20usize)
                } else {
                    (24, 12)
                };
                tkhd_timescale = Some(read_u32(&atom.data[timescale_offset..timescale_offset + 4]));
                tkhd_duration = Some(read_u32(&atom.data[duration_offset..duration_offset + 4]));
            }
            "mdia" => mdia_data = Some(atom.data),
            _ => {}
        }
    }

    let mdia = mdia_data.ok_or_else(|| anyhow!("trak missing mdia"))?;
    let track = parse_media(mdia, tkhd_timescale, tkhd_duration)?;
    match track {
        ParsedTrack::Video(track) => collector.video = Some(track),
        ParsedTrack::Audio(track) => collector.audio = Some(track),
        ParsedTrack::Unknown => {}
    }
    Ok(())
}

enum ParsedTrack {
    Video(VideoTrack),
    Audio(AudioTrack),
    Unknown,
}

fn parse_media(
    data: &[u8],
    tk_timescale: Option<u32>,
    tk_duration: Option<u32>,
) -> Result<ParsedTrack> {
    let mut cursor = Cursor::new(data);
    let mut hdlr_type = None;
    let mut mdhd_timescale = None;
    let mut mdhd_duration = None;
    let mut stsd_data = None;

    while let Some(atom) = read_atom(&mut cursor)? {
        match atom.kind.as_str() {
            "hdlr" => {
                if atom.data.len() >= 12 {
                    hdlr_type = Some(atom.data[8..12].try_into().unwrap());
                }
            }
            "mdhd" => {
                let version = atom
                    .data
                    .first()
                    .copied()
                    .ok_or_else(|| anyhow!("mdhd missing version"))?;
                let (timescale_offset, duration_offset) = if version == 1 {
                    (20usize, 24usize)
                } else {
                    (12, 16)
                };
                mdhd_timescale = Some(read_u32(&atom.data[timescale_offset..timescale_offset + 4]));
                mdhd_duration = Some(read_u32(&atom.data[duration_offset..duration_offset + 4]));
            }
            "minf" => {
                let mut minf_cursor = Cursor::new(atom.data);
                while let Some(child) = read_atom(&mut minf_cursor)? {
                    if child.kind == "stbl" {
                        let mut stbl_cursor = Cursor::new(child.data);
                        while let Some(grandchild) = read_atom(&mut stbl_cursor)? {
                            if grandchild.kind == "stsd" {
                                stsd_data = Some(grandchild.data);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let handler: [u8; 4] = match hdlr_type {
        Some(v) => v,
        None => return Ok(ParsedTrack::Unknown),
    };
    let timescale = mdhd_timescale.or(tk_timescale).unwrap_or(1);
    let duration = mdhd_duration.or(tk_duration).unwrap_or(0);

    let stsd = stsd_data.ok_or_else(|| anyhow!("stsd not found"))?;
    if stsd.len() < 16 {
        bail!("invalid stsd atom");
    }
    let entry_count = read_u32(&stsd[4..8]);
    if entry_count == 0 {
        return Ok(ParsedTrack::Unknown);
    }

    let entry_size = read_u32(&stsd[8..12]) as usize;
    if entry_size + 8 > stsd.len() {
        bail!("stsd entry exceeds buffer");
    }
    let entry_data = &stsd[12..12 + entry_size];
    let codec_fourcc = &entry_data[4..8];

    match &handler {
        b"vide" => {
            let width = u16::from_be_bytes(entry_data[32..34].try_into()?);
            let height = u16::from_be_bytes(entry_data[34..36].try_into()?);
            let codec = match codec_fourcc {
                b"avc1" => VideoCodec::H264,
                b"hvc1" => VideoCodec::H265,
                b"vp09" => VideoCodec::Vp9,
                b"av01" => VideoCodec::Av1,
                _ => VideoCodec::Unknown,
            };
            Ok(ParsedTrack::Video(VideoTrack {
                codec,
                width: width as u32,
                height: height as u32,
                timescale,
                duration,
                frame_count: 0,
            }))
        }
        b"soun" => {
            let channels = u16::from_be_bytes(entry_data[16..18].try_into()?);
            let sample_rate_fixed = read_u32(&entry_data[24..28]);
            let sample_rate = (sample_rate_fixed >> 16) as u32;
            let codec = match codec_fourcc {
                b"lpcm" => AudioCodec::PcmS16,
                b"f32 " => AudioCodec::PcmF32,
                b"aac " => AudioCodec::Aac,
                b"Opus" => AudioCodec::Opus,
                _ => AudioCodec::Unknown,
            };
            Ok(ParsedTrack::Audio(AudioTrack {
                codec,
                sample_rate,
                channels,
                timescale,
                duration,
            }))
        }
        _ => Ok(ParsedTrack::Unknown),
    }
}

fn read_u32(buf: &[u8]) -> u32 {
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&buf[..4]);
    u32::from_be_bytes(bytes)
}

pub fn demux_media(data: &[u8]) -> Result<MediaStreams> {
    Mp4Demuxer::new(data).demux()
}
