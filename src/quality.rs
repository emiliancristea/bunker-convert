use anyhow::{Result, anyhow};
use image::{DynamicImage, GenericImageView};
use serde::Serialize;

type GrayFImage = image::ImageBuffer<image::Luma<f32>, Vec<f32>>;

#[derive(Debug, Clone, Serialize)]
pub struct QualityMetrics {
    pub mse: f64,
    pub psnr: f64,
    pub ssim: f64,
}

pub fn compute_metrics(
    reference: &DynamicImage,
    candidate: &DynamicImage,
) -> Result<QualityMetrics> {
    ensure_dimensions_match(reference, candidate)?;

    let mse = mean_squared_error(reference, candidate);
    let psnr = peak_signal_to_noise_ratio(mse);
    let ssim = structural_similarity(reference, candidate)?;

    Ok(QualityMetrics { mse, psnr, ssim })
}

fn ensure_dimensions_match(reference: &DynamicImage, candidate: &DynamicImage) -> Result<()> {
    if reference.dimensions() != candidate.dimensions() {
        return Err(anyhow!(
            "Cannot compute metrics: dimension mismatch {}x{} vs {}x{}",
            reference.width(),
            reference.height(),
            candidate.width(),
            candidate.height()
        ));
    }
    Ok(())
}

fn mean_squared_error(reference: &DynamicImage, candidate: &DynamicImage) -> f64 {
    let ref_rgb = reference.to_rgb8();
    let cand_rgb = candidate.to_rgb8();

    let mut total = 0.0;
    for (r, c) in ref_rgb.pixels().zip(cand_rgb.pixels()) {
        for chan in 0..3 {
            let diff = r[chan] as f64 - c[chan] as f64;
            total += diff * diff;
        }
    }
    total / ((ref_rgb.width() * ref_rgb.height() * 3) as f64)
}

fn peak_signal_to_noise_ratio(mse: f64) -> f64 {
    if mse == 0.0 {
        f64::INFINITY
    } else {
        20.0 * ((255.0f64).log10()) - 10.0 * mse.log10()
    }
}

fn structural_similarity(reference: &DynamicImage, candidate: &DynamicImage) -> Result<f64> {
    let ref_gray: GrayFImage = reference.to_luma32f();
    let cand_gray: GrayFImage = candidate.to_luma32f();

    let mean_ref = mean(&ref_gray);
    let mean_cand = mean(&cand_gray);
    let cov = covariance(&ref_gray, &cand_gray, mean_ref, mean_cand);
    let var_ref = variance(&ref_gray, mean_ref);
    let var_cand = variance(&cand_gray, mean_cand);

    let c1 = (0.01_f64 * 255.0_f64).powi(2);
    let c2 = (0.03_f64 * 255.0_f64).powi(2);

    let numerator = (2.0 * mean_ref * mean_cand + c1) * (2.0 * cov + c2);
    let denominator = (mean_ref.powi(2) + mean_cand.powi(2) + c1) * (var_ref + var_cand + c2);
    if denominator == 0.0 {
        return Err(anyhow!("SSIM denominator is zero"));
    }

    Ok(numerator / denominator)
}

fn mean(image: &GrayFImage) -> f64 {
    image.pixels().map(|p| p[0] as f64).sum::<f64>() / (image.width() * image.height()) as f64
}

fn variance(image: &GrayFImage, mean: f64) -> f64 {
    image
        .pixels()
        .map(|p| {
            let diff = p[0] as f64 - mean;
            diff * diff
        })
        .sum::<f64>()
        / (image.width() * image.height()) as f64
}

fn covariance(
    reference: &GrayFImage,
    candidate: &GrayFImage,
    mean_ref: f64,
    mean_cand: f64,
) -> f64 {
    reference
        .pixels()
        .zip(candidate.pixels())
        .map(|(r, c)| (r[0] as f64 - mean_ref) * (c[0] as f64 - mean_cand))
        .sum::<f64>()
        / (reference.width() * reference.height()) as f64
}
