//! Pixel helpers shared by the ONNX pipelines.

use anyhow::{Context, Result};
use image::{imageops, imageops::FilterType, RgbImage};

/// Decode any supported image and flatten transparency onto white.
pub fn decode_rgb(bytes: &[u8]) -> Result<RgbImage> {
    let decoded = image::load_from_memory(bytes).context("unsupported or corrupt image")?;
    let rgba = decoded.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut rgb = RgbImage::new(w, h);
    for (src, dst) in rgba.pixels().zip(rgb.pixels_mut()) {
        let a = src[3] as u32;
        for c in 0..3 {
            dst[c] = ((src[c] as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
        }
    }
    Ok(rgb)
}

/// Invert light ink on a mostly uniform dark background (dark-mode screenshots).
pub fn normalize_polarity(img: &mut RgbImage) {
    let n = (img.width() * img.height()) as usize;
    if n == 0 {
        return;
    }
    // Gray = mean of channels; work on channel sums (0..=765) to keep it exact.
    let mut hist = vec![0usize; 766];
    for p in img.pixels() {
        hist[p[0] as usize + p[1] as usize + p[2] as usize] += 1;
    }
    let background = median_from_hist(&hist, n) / 3.0;
    if background >= 100.0 {
        return;
    }
    let mut near = 0usize;
    let mut bright = 0usize;
    for (sum, count) in hist.iter().enumerate() {
        let g = sum as f64 / 3.0;
        if (g - background).abs() <= 20.0 {
            near += count;
        }
        if g >= background + 60.0 {
            bright += count;
        }
    }
    let bg_frac = near as f64 / n as f64;
    let fg_frac = bright as f64 / n as f64;
    if bg_frac >= 0.65 && (0.003..=0.30).contains(&fg_frac) {
        imageops::invert(img);
    }
}

fn median_from_hist(hist: &[usize], n: usize) -> f64 {
    let value_at = |rank: usize| -> f64 {
        let mut acc = 0usize;
        for (v, c) in hist.iter().enumerate() {
            acc += c;
            if acc > rank {
                return v as f64;
            }
        }
        (hist.len() - 1) as f64
    };
    if n % 2 == 1 {
        value_at(n / 2)
    } else {
        (value_at(n / 2 - 1) + value_at(n / 2)) / 2.0
    }
}

/// Crop with clamping; returns `None` for empty regions.
pub fn crop(img: &RgbImage, x1: i64, y1: i64, x2: i64, y2: i64) -> Option<RgbImage> {
    let (w, h) = (img.width() as i64, img.height() as i64);
    let (l, t) = (x1.clamp(0, w), y1.clamp(0, h));
    let (r, b) = (x2.clamp(0, w), y2.clamp(0, h));
    if r <= l || b <= t {
        return None;
    }
    Some(imageops::crop_imm(img, l as u32, t as u32, (r - l) as u32, (b - t) as u32).to_image())
}

pub fn resize(img: &RgbImage, w: u32, h: u32, filter: FilterType) -> RgbImage {
    if img.width() == w && img.height() == h {
        return img.clone();
    }
    imageops::resize(img, w.max(1), h.max(1), filter)
}

/// Planar CHW float tensor, `(pixel * scale - mean) / std`, optional BGR order.
pub fn to_chw(img: &RgbImage, scale: f32, mean: [f32; 3], std: [f32; 3], bgr: bool) -> Vec<f32> {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let plane = w * h;
    let mut out = vec![0f32; plane * 3];
    for (i, p) in img.pixels().enumerate() {
        for c in 0..3 {
            let src = if bgr { 2 - c } else { c };
            out[c * plane + i] = (p[src] as f32 * scale - mean[c]) / std[c];
        }
    }
    out
}

/// Grayscale (ITU-R 601) values used by ink masks.
pub fn gray601(img: &RgbImage) -> Vec<f32> {
    img.pixels()
        .map(|p| 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32)
        .collect()
}

/// `numpy.percentile` with linear interpolation.
pub fn percentile(values: &[f32], q: f64) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mut v = values.to_vec();
    let pos = q / 100.0 * (v.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    let (_, lo_v, _) = v.select_nth_unstable_by(lo, f32::total_cmp);
    let lo_v = *lo_v;
    if hi == lo {
        return lo_v;
    }
    let hi_v = v[lo + 1..].iter().copied().fold(f32::INFINITY, f32::min);
    lo_v + ((hi_v - lo_v) as f64 * (pos - lo as f64)) as f32
}

/// Mean and population standard deviation over all channels of a region.
pub fn region_stats(img: &RgbImage, l: u32, t: u32, r: u32, b: u32) -> (f64, f64) {
    let mut sum = 0f64;
    let mut sq = 0f64;
    let mut n = 0f64;
    for y in t..b {
        for x in l..r {
            for &c in img.get_pixel(x, y).0.iter() {
                let v = c as f64;
                sum += v;
                sq += v * v;
                n += 1.0;
            }
        }
    }
    if n == 0.0 {
        return (255.0, 0.0);
    }
    let mean = sum / n;
    (mean, (sq / n - mean * mean).max(0.0).sqrt())
}

/// Python's `round()` (banker's rounding) for parity with the reference pipeline.
pub fn round_half_even(v: f64) -> f64 {
    let r = v.round();
    if (v - v.trunc()).abs() == 0.5 && r % 2.0 != 0.0 {
        r - v.signum()
    } else {
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    #[test]
    fn percentile_matches_numpy_linear() {
        let v = [1.0, 2.0, 3.0, 4.0];
        assert!((percentile(&v, 95.0) - 3.85).abs() < 1e-5);
        assert_eq!(percentile(&v, 0.0), 1.0);
        assert_eq!(percentile(&v, 100.0), 4.0);
    }

    #[test]
    fn banker_rounding() {
        assert_eq!(round_half_even(2.5), 2.0);
        assert_eq!(round_half_even(3.5), 4.0);
        assert_eq!(round_half_even(-2.5), -2.0);
        assert_eq!(round_half_even(2.4), 2.0);
    }

    #[test]
    fn dark_background_is_inverted() {
        let mut img = RgbImage::from_pixel(100, 100, Rgb([20, 20, 20]));
        for x in 10..60 {
            img.put_pixel(x, 50, Rgb([240, 240, 240]));
        }
        normalize_polarity(&mut img);
        assert_eq!(img.get_pixel(0, 0)[0], 235);
        assert_eq!(img.get_pixel(20, 50)[0], 15);
    }

    #[test]
    fn light_background_is_untouched() {
        let mut img = RgbImage::from_pixel(10, 10, Rgb([250, 250, 250]));
        normalize_polarity(&mut img);
        assert_eq!(img.get_pixel(0, 0)[0], 250);
    }

    #[test]
    fn transparent_png_flattens_to_white() {
        let rgba = image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 0, 0]));
        let mut bytes = Vec::new();
        rgba.write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
        let rgb = decode_rgb(&bytes).unwrap();
        assert_eq!(rgb.get_pixel(0, 0).0, [255, 255, 255]);
    }

    #[test]
    fn chw_layout_and_bgr() {
        let img = RgbImage::from_pixel(1, 1, Rgb([255, 0, 0]));
        let rgb = to_chw(&img, 1.0 / 255.0, [0.0; 3], [1.0; 3], false);
        assert_eq!(rgb, vec![1.0, 0.0, 0.0]);
        let bgr = to_chw(&img, 1.0 / 255.0, [0.0; 3], [1.0; 3], true);
        assert_eq!(bgr, vec![0.0, 0.0, 1.0]);
    }

    #[test]
    fn crop_clamps_and_rejects_empty() {
        let img = RgbImage::new(10, 10);
        assert_eq!(crop(&img, -5, -5, 4, 4).unwrap().dimensions(), (4, 4));
        assert!(crop(&img, 5, 5, 5, 9).is_none());
    }
}
