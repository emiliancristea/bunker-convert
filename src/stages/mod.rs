use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use image::codecs::avif::{AvifEncoder, ColorSpace as AvifColorSpace};
use image::codecs::gif::{GifEncoder, Repeat as GifRepeat};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{
    CompressionType as PngCompressionType, FilterType as PngFilterType, PngEncoder,
};
use image::imageops::FilterType as ResizeFilter;
use image::{DynamicImage, ExtendedColorType, ImageEncoder, ImageFormat};
use serde_json::{Value, json};
use tracing::warn;
use webp::Encoder as WebpEncoder;

use crate::pipeline::{
    Artifact, OutputSpec, PipelineContext, Stage, StageParameters, StageRegistry,
};
use crate::scheduler::StageDevice;

pub fn register_defaults(registry: &mut StageRegistry) {
    registry.register("decode", |params| {
        Ok(Box::new(DecodeStage::from_params(params)?))
    });
    registry.register("annotate", |params| {
        Ok(Box::new(AnnotateStage::from_params(params)?))
    });
    registry.register("resize", |params| {
        Ok(Box::new(ResizeStage::from_params(params)?))
    });
    registry.register("encode", |params| {
        Ok(Box::new(EncodeStage::from_params(params)?))
    });
}

struct DecodeStage {
    format_hint: Option<String>,
}

impl DecodeStage {
    fn from_params(mut params: StageParameters) -> Result<Self> {
        let format_hint = take_string(&mut params, "format");
        Ok(Self { format_hint })
    }
}

impl Stage for DecodeStage {
    fn name(&self) -> &'static str {
        "decode"
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
        let (image_format, label) = infer_format(self.format_hint.as_deref(), artifact)?;
        let decoded = image::load_from_memory_with_format(&artifact.data, image_format)
            .with_context(|| format!("Failed to decode image as {:?}", image_format))?;

        let width = decoded.width();
        let height = decoded.height();
        artifact.set_original_image(decoded.clone());
        artifact.set_image(decoded);
        artifact.set_format(label.clone());
        artifact
            .metadata
            .insert("image.width".to_string(), json!(width));
        artifact
            .metadata
            .insert("image.height".to_string(), json!(height));
        Ok(())
    }
}

struct AnnotateStage {
    key: String,
    value: Value,
}

impl AnnotateStage {
    fn from_params(mut params: StageParameters) -> Result<Self> {
        let key = take_string(&mut params, "key")
            .ok_or_else(|| anyhow!("annotate stage requires 'key' parameter"))?;
        let value = params
            .remove("value")
            .unwrap_or(Value::String("true".to_string()));
        Ok(Self { key, value })
    }
}

impl Stage for AnnotateStage {
    fn name(&self) -> &'static str {
        "annotate"
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
        artifact
            .metadata
            .insert(self.key.clone(), self.value.clone());
        Ok(())
    }
}

struct ResizeStage {
    width: u32,
    height: u32,
    fit: ResizeMode,
    filter: ResizeFilter,
}

impl ResizeStage {
    fn from_params(mut params: StageParameters) -> Result<Self> {
        let width = take_u32(&mut params, "width")
            .ok_or_else(|| anyhow!("resize stage requires 'width' parameter"))?;
        let height = take_u32(&mut params, "height")
            .ok_or_else(|| anyhow!("resize stage requires 'height' parameter"))?;
        let fit = take_string(&mut params, "fit");
        let filter = take_string(&mut params, "method")
            .and_then(map_filter)
            .unwrap_or(ResizeFilter::CatmullRom);
        Ok(Self {
            width,
            height,
            fit: fit
                .as_deref()
                .and_then(ResizeMode::from_str)
                .unwrap_or(ResizeMode::Inside),
            filter,
        })
    }
}

impl Stage for ResizeStage {
    fn name(&self) -> &'static str {
        "resize"
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
        let image = artifact
            .image
            .as_ref()
            .ok_or_else(|| anyhow!("resize stage requires a decoded image"))?;

        let resized = match self.fit {
            ResizeMode::Cover => image.resize_to_fill(self.width, self.height, self.filter),
            ResizeMode::Exact => image.resize_exact(self.width, self.height, self.filter),
            ResizeMode::Inside => image.resize(self.width, self.height, self.filter),
        };

        artifact.set_image(resized.clone());
        artifact
            .metadata
            .insert("resize.width".to_string(), json!(self.width));
        artifact
            .metadata
            .insert("resize.height".to_string(), json!(self.height));
        artifact.metadata.insert(
            "resize.filter".to_string(),
            Value::String(format_filter(self.filter)),
        );
        artifact.metadata.insert(
            "resize.mode".to_string(),
            Value::String(self.fit.as_str().to_string()),
        );
        record_dimensions(artifact, "image", &resized);
        Ok(())
    }
}

struct EncodeStage {
    format: Option<String>,
    extension: Option<String>,
    options: StageParameters,
}

impl EncodeStage {
    fn from_params(mut params: StageParameters) -> Result<Self> {
        let format = take_string(&mut params, "format");
        let extension = take_string(&mut params, "extension");
        Ok(Self {
            format,
            extension,
            options: params,
        })
    }
}

impl Stage for EncodeStage {
    fn name(&self) -> &'static str {
        "encode"
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
        let (image_format, label) = infer_format(self.format.as_deref(), artifact)?;
        artifact.set_format(label.clone());
        let extension = self
            .extension
            .clone()
            .unwrap_or_else(|| format_extension(image_format).to_string());

        let image = artifact
            .image
            .as_ref()
            .ok_or_else(|| anyhow!("encode stage requires a decoded image"))?;

        let buffer = encode_with_options(image, image_format, &self.options)
            .with_context(|| format!("Failed to encode image as {:?}", image_format))?;

        let resolved = resolve_output_path(&ctx.output, artifact, &extension);
        if let Some(parent) = resolved.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create output directory: {}", parent.display())
            })?;
        }
        fs::write(&resolved, &buffer)
            .with_context(|| format!("Failed to write output file: {}", resolved.display()))?;

        match image::load_from_memory_with_format(&buffer, image_format) {
            Ok(decoded) => {
                artifact
                    .metadata
                    .insert("output.decode_supported".into(), Value::Bool(true));
                artifact.set_image(decoded.clone());
                record_dimensions(artifact, "image", &decoded);
            }
            Err(err) => {
                artifact
                    .metadata
                    .insert("output.decode_supported".into(), Value::Bool(false));
                artifact.metadata.insert(
                    "output.decode_warning".into(),
                    Value::String(err.to_string()),
                );
                warn!(
                    format = ?image_format,
                    error = %err,
                    "Post-encode decode skipped; decoder unavailable"
                );
                artifact.image = None;
            }
        }
        artifact.replace_data(buffer.clone());
        artifact.metadata.insert(
            "output_path".to_string(),
            Value::String(resolved.to_string_lossy().to_string()),
        );
        artifact.metadata.insert(
            "output.extension".to_string(),
            Value::String(extension.clone()),
        );
        artifact
            .metadata
            .insert("output.format".to_string(), Value::String(label));
        artifact
            .metadata
            .insert("output.size_bytes".to_string(), json!(buffer.len()));
        record_encoder_metadata(artifact, &self.options);
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

fn encode_with_options(
    image: &DynamicImage,
    format: ImageFormat,
    options: &StageParameters,
) -> Result<Vec<u8>> {
    match format {
        ImageFormat::Jpeg => encode_jpeg(image, options),
        ImageFormat::Png => encode_png(image, options),
        ImageFormat::WebP => encode_webp(image, options),
        ImageFormat::Avif => encode_avif(image, options),
        ImageFormat::Gif => encode_gif(image, options),
        _ => encode_generic(image, format),
    }
}

fn encode_jpeg(image: &DynamicImage, options: &StageParameters) -> Result<Vec<u8>> {
    let (data, width, height) = to_rgb8(image);
    let mut cursor = Cursor::new(Vec::new());
    let quality = param_u8(options, "quality").unwrap_or(90).clamp(1, 100);
    {
        let mut encoder = JpegEncoder::new_with_quality(&mut cursor, quality);
        if let Some((icc, path)) = load_icc_profile(options)? {
            encoder.set_icc_profile(icc).map_err(|err| {
                anyhow!("Failed to apply ICC profile '{path}' for JPEG encoder: {err}")
            })?;
        }
        encoder
            .write_image(&data, width, height, ExtendedColorType::Rgb8)
            .context("JPEG encode failed")?;
    }
    Ok(cursor.into_inner())
}

fn encode_png(image: &DynamicImage, options: &StageParameters) -> Result<Vec<u8>> {
    let (data, width, height) = to_rgba8(image);
    let compression = parse_png_compression(options)?;
    let filter = parse_png_filter(options)?;
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut encoder = PngEncoder::new_with_quality(&mut cursor, compression, filter);
        if let Some((icc, path)) = load_icc_profile(options)? {
            encoder.set_icc_profile(icc).map_err(|err| {
                anyhow!("Failed to apply ICC profile '{path}' for PNG encoder: {err}")
            })?;
        }
        encoder
            .write_image(&data, width, height, ExtendedColorType::Rgba8)
            .context("PNG encode failed")?;
    }
    Ok(cursor.into_inner())
}

fn encode_webp(image: &DynamicImage, options: &StageParameters) -> Result<Vec<u8>> {
    let lossless = param_bool(options, "lossless").unwrap_or(false);
    let quality = param_f64(options, "quality")
        .unwrap_or(75.0)
        .clamp(0.0, 100.0) as f32;
    let encoder = WebpEncoder::from_image(image)
        .map_err(|err| anyhow!("Failed to prepare WebP encoder: {err}"))?;
    let encoded = if lossless {
        encoder.encode_lossless()
    } else {
        encoder.encode(quality)
    };
    Ok(encoded.to_vec())
}

fn encode_avif(image: &DynamicImage, options: &StageParameters) -> Result<Vec<u8>> {
    let (data, width, height) = to_rgba8(image);
    let quality = param_u8(options, "quality").unwrap_or(80).clamp(1, 100);
    let speed = param_u8(options, "speed").unwrap_or(4).clamp(1, 10);
    let mut cursor = Cursor::new(Vec::new());
    let encoder = AvifEncoder::new_with_speed_quality(&mut cursor, speed, quality);
    let encoder = match parse_avif_colorspace(options)? {
        Some(space) => encoder.with_colorspace(space),
        None => encoder,
    };
    encoder
        .write_image(&data, width, height, ExtendedColorType::Rgba8)
        .context("AVIF encode failed")?;
    Ok(cursor.into_inner())
}

fn encode_gif(image: &DynamicImage, options: &StageParameters) -> Result<Vec<u8>> {
    let (data, width, height) = to_rgba8(image);
    let speed = param_u8(options, "speed").unwrap_or(10).clamp(1, 30) as i32;
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut encoder = GifEncoder::new_with_speed(&mut cursor, speed);
        if let Some(repeat) = parse_gif_repeat(options)? {
            encoder
                .set_repeat(repeat)
                .context("Failed to configure GIF repeat")?;
        }
        encoder
            .encode(&data, width, height, ExtendedColorType::Rgba8)
            .context("GIF encode failed")?;
    }
    Ok(cursor.into_inner())
}

fn encode_generic(image: &DynamicImage, format: ImageFormat) -> Result<Vec<u8>> {
    let mut cursor = Cursor::new(Vec::new());
    image
        .write_to(&mut cursor, format)
        .with_context(|| format!("Failed to encode image as {:?}", format))?;
    Ok(cursor.into_inner())
}

fn to_rgb8(image: &DynamicImage) -> (Vec<u8>, u32, u32) {
    let rgb = image.to_rgb8();
    let (width, height) = rgb.dimensions();
    (rgb.into_raw(), width, height)
}

fn to_rgba8(image: &DynamicImage) -> (Vec<u8>, u32, u32) {
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    (rgba.into_raw(), width, height)
}

fn load_icc_profile(options: &StageParameters) -> Result<Option<(Vec<u8>, String)>> {
    match options.get("icc_profile_path") {
        Some(Value::String(path)) => {
            let data = fs::read(Path::new(path))
                .with_context(|| format!("Failed to read ICC profile from '{path}'"))?;
            Ok(Some((data, path.clone())))
        }
        Some(other) => bail!("icc_profile_path must be a string, got {other:?}"),
        None => Ok(None),
    }
}

fn parse_png_compression(options: &StageParameters) -> Result<PngCompressionType> {
    let Some(value) = options.get("compression") else {
        return Ok(PngCompressionType::Default);
    };
    if let Some(s) = value.as_str() {
        return match s.trim().to_lowercase().as_str() {
            "fast" => Ok(PngCompressionType::Fast),
            "default" => Ok(PngCompressionType::Default),
            "best" => Ok(PngCompressionType::Best),
            other => bail!("Unknown PNG compression profile '{other}'"),
        };
    }
    if let Some(level) = value_as_u64(value) {
        return Ok(match level {
            0..=3 => PngCompressionType::Fast,
            4..=6 => PngCompressionType::Default,
            7..=9 => PngCompressionType::Best,
            _ => PngCompressionType::Best,
        });
    }
    bail!("Unsupported PNG compression value: {value}")
}

fn parse_png_filter(options: &StageParameters) -> Result<PngFilterType> {
    let Some(value) = options.get("filter") else {
        return Ok(PngFilterType::Adaptive);
    };
    if let Some(s) = value.as_str() {
        return match s.trim().to_lowercase().as_str() {
            "adaptive" => Ok(PngFilterType::Adaptive),
            "none" | "nofilter" => Ok(PngFilterType::NoFilter),
            "sub" => Ok(PngFilterType::Sub),
            "up" => Ok(PngFilterType::Up),
            "avg" | "average" => Ok(PngFilterType::Avg),
            "paeth" => Ok(PngFilterType::Paeth),
            other => bail!("Unknown PNG filter '{other}'"),
        };
    }
    if let Some(value) = value_as_u64(value) {
        return match value {
            0 => Ok(PngFilterType::NoFilter),
            1 => Ok(PngFilterType::Sub),
            2 => Ok(PngFilterType::Up),
            3 => Ok(PngFilterType::Avg),
            4 => Ok(PngFilterType::Paeth),
            _ => bail!("PNG filter numeric value must be in 0..=4"),
        };
    }
    bail!("Unsupported PNG filter value: {value}")
}

fn parse_avif_colorspace(options: &StageParameters) -> Result<Option<AvifColorSpace>> {
    let Some(Value::String(value)) = options.get("colorspace") else {
        return Ok(None);
    };
    let normalized = value.trim().to_lowercase();
    match normalized.as_str() {
        "srgb" => Ok(Some(AvifColorSpace::Srgb)),
        "bt709" | "rec709" => Ok(Some(AvifColorSpace::Bt709)),
        other => bail!("Unsupported AVIF colorspace '{other}'"),
    }
}

fn parse_gif_repeat(options: &StageParameters) -> Result<Option<GifRepeat>> {
    let Some(value) = options.get("repeat") else {
        return Ok(None);
    };
    if let Some(text) = value.as_str() {
        let normalized = text.trim().to_lowercase();
        return if normalized == "infinite" || normalized == "loop" {
            Ok(Some(GifRepeat::Infinite))
        } else {
            let count: u16 = normalized
                .parse()
                .map_err(|_| anyhow!("Failed to parse GIF repeat count from '{text}'"))?;
            Ok(Some(GifRepeat::Finite(count)))
        };
    }
    if let Some(count) = value_as_u64(value) {
        let count =
            u16::try_from(count).map_err(|_| anyhow!("GIF repeat count {count} exceeds 65535"))?;
        return Ok(Some(GifRepeat::Finite(count)));
    }
    bail!("Unsupported GIF repeat value: {value}")
}

fn record_encoder_metadata(artifact: &mut Artifact, options: &StageParameters) {
    if let Some(q) = param_f64(options, "quality") {
        artifact
            .metadata
            .insert("output.encoder.quality".into(), json!(q));
    }
    if let Some(speed) = param_u8(options, "speed") {
        artifact
            .metadata
            .insert("output.encoder.speed".into(), json!(speed));
    }
    if let Some(lossless) = param_bool(options, "lossless") {
        artifact
            .metadata
            .insert("output.encoder.lossless".into(), json!(lossless));
    }
    if let Some(path) = param_string(options, "icc_profile_path") {
        artifact.metadata.insert(
            "output.encoder.icc_profile_path".into(),
            Value::String(path),
        );
    }
    if let Some(color) = param_string(options, "colorspace") {
        artifact
            .metadata
            .insert("output.encoder.colorspace".into(), Value::String(color));
    }
    for key in ["compression", "filter", "repeat"] {
        if let Some(value) = options.get(key) {
            artifact
                .metadata
                .insert(format!("output.encoder.{key}"), value.clone());
        }
    }
}

fn param_string(options: &StageParameters, key: &str) -> Option<String> {
    options
        .get(key)
        .and_then(|value| value.as_str().map(|s| s.to_string()))
}

fn param_f64(options: &StageParameters, key: &str) -> Option<f64> {
    options.get(key).and_then(value_as_f64)
}

fn param_u8(options: &StageParameters, key: &str) -> Option<u8> {
    options
        .get(key)
        .and_then(value_as_u64)
        .and_then(|value| u8::try_from(value).ok())
}

fn param_bool(options: &StageParameters, key: &str) -> Option<bool> {
    options.get(key).and_then(value_as_bool)
}

fn value_as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(num) => num.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(true) => Some(1.0),
        Value::Bool(false) => Some(0.0),
        _ => None,
    }
}

fn value_as_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(num) => num.as_u64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(true) => Some(1),
        Value::Bool(false) => Some(0),
        _ => None,
    }
}

fn value_as_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(b) => Some(*b),
        Value::Number(num) => num.as_u64().map(|n| n != 0),
        Value::String(s) => match s.trim().to_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Some(true),
            "false" | "no" | "off" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn take_string(params: &mut StageParameters, key: &str) -> Option<String> {
    params.remove(key).and_then(|value| match value {
        Value::String(s) => Some(s),
        other => Some(other.to_string()),
    })
}

fn take_u32(params: &mut StageParameters, key: &str) -> Option<u32> {
    params.remove(key).and_then(|value| match value {
        Value::Number(num) => num.as_u64().and_then(|n| n.try_into().ok()),
        Value::String(s) => s.parse().ok(),
        _ => None,
    })
}

#[derive(Clone, Copy)]
enum ResizeMode {
    Inside,
    Cover,
    Exact,
}

impl ResizeMode {
    fn from_str(value: &str) -> Option<Self> {
        match value.to_lowercase().as_str() {
            "cover" => Some(Self::Cover),
            "exact" | "stretch" => Some(Self::Exact),
            "inside" | "fit" => Some(Self::Inside),
            _ => None,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Inside => "inside",
            Self::Cover => "cover",
            Self::Exact => "exact",
        }
    }
}

fn map_filter(value: String) -> Option<ResizeFilter> {
    match value.to_lowercase().as_str() {
        "nearest" => Some(ResizeFilter::Nearest),
        "triangle" => Some(ResizeFilter::Triangle),
        "catmullrom" => Some(ResizeFilter::CatmullRom),
        "lanczos3" => Some(ResizeFilter::Lanczos3),
        "gaussian" => Some(ResizeFilter::Gaussian),
        _ => None,
    }
}

fn infer_format<'a>(hint: Option<&'a str>, artifact: &Artifact) -> Result<(ImageFormat, String)> {
    if let Some(hint) = hint {
        if let Some(fmt) = format_from_label(hint) {
            return Ok((fmt, format_extension(fmt).to_string()));
        }
    }

    if let Some(existing) = artifact.format.as_deref() {
        if let Some(fmt) = format_from_label(existing) {
            return Ok((fmt, format_extension(fmt).to_string()));
        }
    }

    if let Some(ext) = artifact
        .input_path
        .extension()
        .and_then(|s| s.to_str())
        .and_then(format_from_label)
    {
        return Ok((ext, format_extension(ext).to_string()));
    }

    let guessed = image::guess_format(&artifact.data)
        .context("Unable to infer image format from input data")?;
    Ok((guessed, format_extension(guessed).to_string()))
}

fn format_from_label(label: &str) -> Option<ImageFormat> {
    let normalized = label.trim().trim_start_matches('.').to_lowercase();
    ImageFormat::from_extension(&normalized)
}

fn format_extension(format: ImageFormat) -> &'static str {
    format.extensions_str().first().copied().unwrap_or("bin")
}

fn record_dimensions(artifact: &mut Artifact, prefix: &str, image: &DynamicImage) {
    artifact
        .metadata
        .insert(format!("{prefix}.width"), json!(image.width()));
    artifact
        .metadata
        .insert(format!("{prefix}.height"), json!(image.height()));
}

fn format_filter(filter: ResizeFilter) -> String {
    match filter {
        ResizeFilter::Nearest => "nearest",
        ResizeFilter::Triangle => "triangle",
        ResizeFilter::CatmullRom => "catmullrom",
        ResizeFilter::Gaussian => "gaussian",
        ResizeFilter::Lanczos3 => "lanczos3",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::map_filter;
    use image::imageops::FilterType;

    #[test]
    fn filter_mapping() {
        assert_eq!(map_filter("lanczos3".into()), Some(FilterType::Lanczos3));
        assert_eq!(map_filter("nearest".into()), Some(FilterType::Nearest));
        assert_eq!(map_filter("unknown".into()), None);
    }
}
