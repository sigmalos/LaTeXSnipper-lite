//! Split multi-line formulas into per-row (and per-wide-segment) crops.
//! Port of `mathcraft_ocr/formula_lines.py`.

use super::imageops::{crop, gray601, normalize_polarity, percentile, round_half_even};
use image::RgbImage;
use std::collections::VecDeque;

pub struct LineCrop {
    pub image: RgbImage,
    /// left, top, right, bottom in the source image
    pub bbox: (u32, u32, u32, u32),
}

pub type LineGroup = Vec<LineCrop>;

const WIDE_LINE_ASPECT: f64 = 7.0;

fn r(v: f64) -> i64 {
    round_half_even(v) as i64
}

pub struct Mask {
    pub w: usize,
    pub h: usize,
    pub data: Vec<bool>,
}

impl Mask {
    fn at(&self, x: usize, y: usize) -> bool {
        self.data[y * self.w + x]
    }
    fn count(&self) -> usize {
        self.data.iter().filter(|&&v| v).count()
    }
    fn row_count(&self, y: usize) -> usize {
        self.data[y * self.w..(y + 1) * self.w]
            .iter()
            .filter(|&&v| v)
            .count()
    }
    fn col_counts(&self, top: usize, bottom: usize) -> Vec<usize> {
        let mut out = vec![0; self.w];
        for y in top..=bottom {
            for (x, c) in out.iter_mut().enumerate() {
                *c += self.at(x, y) as usize;
            }
        }
        out
    }
    /// (min_x, min_y, max_x, max_y) of set pixels in rows `top..=bottom`.
    fn bounds(&self, top: usize, bottom: usize) -> Option<(usize, usize, usize, usize)> {
        let mut b: Option<(usize, usize, usize, usize)> = None;
        for y in top..=bottom {
            for x in 0..self.w {
                if self.at(x, y) {
                    b = Some(match b {
                        None => (x, y, x, y),
                        Some((x1, y1, x2, y2)) => (x1.min(x), y1.min(y), x2.max(x), y2.max(y)),
                    });
                }
            }
        }
        b
    }
}

pub fn ink_mask(rgb: &RgbImage) -> Mask {
    let (w, h) = (rgb.width() as usize, rgb.height() as usize);
    let gray = gray601(rgb);
    let background = percentile(&gray, 95.0);
    let threshold = (background - 28.0).clamp(80.0, 245.0);
    let mut mask = Mask {
        w,
        h,
        data: gray.iter().map(|&g| g < threshold).collect(),
    };
    remove_dark_outer_frame(&mut mask);
    let border = (w.min(h) / 80).max(1);
    for y in 0..h {
        for x in 0..w {
            if y < border
                || y >= h.saturating_sub(border)
                || x < border
                || x >= w.saturating_sub(border)
            {
                mask.data[y * w + x] = false;
            }
        }
    }
    mask
}

fn remove_dark_outer_frame(mask: &mut Mask) {
    let (w, h) = (mask.w, mask.h);
    let cs = (w.min(h) / 30).max(4);
    if cs > w || cs > h {
        return;
    }
    let origins = [(0, 0), (0, w - cs), (h - cs, 0), (h - cs, w - cs)];
    let dark = origins
        .iter()
        .filter(|(y0, x0)| {
            let mut n = 0;
            for y in *y0..y0 + cs {
                for x in *x0..x0 + cs {
                    n += mask.at(x, y) as usize;
                }
            }
            n as f64 / (cs * cs) as f64 >= 0.25
        })
        .count();
    if dark < 3 {
        return;
    }
    let mut connected = vec![false; w * h];
    let mut queue = VecDeque::new();
    for (y0, x0) in origins {
        for y in y0..y0 + cs {
            for x in x0..x0 + cs {
                if mask.at(x, y) && !connected[y * w + x] {
                    connected[y * w + x] = true;
                    queue.push_back((y, x));
                }
            }
        }
    }
    while let Some((y, x)) = queue.pop_front() {
        for ny in y.saturating_sub(1)..(y + 2).min(h) {
            for nx in x.saturating_sub(1)..(x + 2).min(w) {
                let i = ny * w + nx;
                if !connected[i] && mask.data[i] {
                    connected[i] = true;
                    queue.push_back((ny, nx));
                }
            }
        }
    }
    let n = connected.iter().filter(|&&v| v).count() as i64;
    if n < 32.max(r((w * h) as f64 * 0.0005)) {
        return;
    }
    for (m, c) in mask.data.iter_mut().zip(connected) {
        *m = *m && !c;
    }
}

fn row_bands(has_ink: &[bool]) -> Vec<(usize, usize)> {
    let mut bands = Vec::new();
    let mut start = None;
    for (i, &v) in has_ink.iter().enumerate() {
        match (v, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                bands.push((s, i - 1));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        bands.push((s, has_ink.len() - 1));
    }
    bands
}

fn merge_close_bands(bands: Vec<(usize, usize)>, max_gap: i64) -> Vec<(usize, usize)> {
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (top, bottom) in bands {
        match merged.last_mut() {
            Some(last) if top as i64 - last.1 as i64 - 1 <= max_gap => last.1 = bottom,
            _ => merged.push((top, bottom)),
        }
    }
    merged
}

fn band_looks_like_row(mask: &Mask, top: usize, bottom: usize, width: usize) -> bool {
    let ink: usize = (top..=bottom).map(|y| mask.row_count(y)).sum();
    if bottom - top + 1 < 5 || (ink as i64) < 12.max(r(width as f64 * 0.02)) {
        return false;
    }
    let cols = mask.col_counts(top, bottom);
    let active = cols.iter().filter(|&&c| c > 0).count() as i64;
    if active < 12.max(r(width as f64 * 0.015)) {
        return false;
    }
    let first = cols.iter().position(|&c| c > 0);
    let last = cols.iter().rposition(|&c| c > 0);
    matches!((first, last), (Some(a), Some(b)) if b - a + 1 >= 24)
}

fn filter_annotation_bands(
    mask: &Mask,
    bands: Vec<(usize, usize)>,
    width: usize,
) -> Vec<(usize, usize)> {
    if bands.len() < 2 {
        return bands;
    }
    // (active_columns, left, height, top, bottom)
    let stats: Vec<(i64, i64, i64, usize, usize)> = bands
        .iter()
        .filter_map(|&(t, b)| {
            let (x1, _, _, _) = mask.bounds(t, b)?;
            let active = mask.col_counts(t, b).iter().filter(|&&c| c > 0).count() as i64;
            Some((active, x1 as i64, (b - t + 1) as i64, t, b))
        })
        .collect();
    if stats.len() < 2 {
        return stats.iter().map(|s| (s.3, s.4)).collect();
    }
    let max_active = stats.iter().map(|s| s.0).max().unwrap_or(0);
    let min_active = r(width as f64 * 0.13).max(r(max_active as f64 * 0.35));
    let strong: Vec<_> = stats.iter().filter(|s| s.0 >= min_active).collect();
    if strong.is_empty() {
        return vec![];
    }
    let mut heights: Vec<i64> = strong.iter().map(|s| s.2).collect();
    heights.sort();
    let median = heights[heights.len() / 2];
    let tol = 10.max(r(width as f64 * 0.03));
    let min_ctx = 5.max(r(median as f64 * 0.40));
    let lefts: Vec<i64> = strong.iter().map(|s| s.1).collect();
    stats
        .iter()
        .filter(|s| {
            s.0 >= min_active || (s.2 >= min_ctx && lefts.iter().any(|l| (s.1 - l).abs() <= tol))
        })
        .map(|s| (s.3, s.4))
        .collect()
}

fn looks_like_compact_fraction(mask: &Mask, bands: &[(usize, usize)], w: usize, h: usize) -> bool {
    if bands.len() != 2 || h < 70 || w as f64 / h.max(1) as f64 > 2.65 {
        return false;
    }
    let (Some(a), Some(b)) = (
        mask.bounds(bands[0].0, bands[0].1),
        mask.bounds(bands[1].0, bands[1].1),
    ) else {
        return false;
    };
    let (fw, fh) = (w as f64, h as f64);
    let gap = b.1 as i64 - a.3 as i64 - 1;
    if gap < 0 || gap > 12.max(r(fh * 0.09)) {
        return false;
    }
    let aw = (a.2 - a.0 + 1) as f64;
    let bw = (b.2 - b.0 + 1) as f64;
    let a_inset = a.0.min(w - a.2 - 1) as f64;
    let b_inset = b.0.min(w - b.2 - 1) as f64;
    let first_centered = aw <= fw * 0.86 && a_inset >= fw * 0.06;
    let second_wider = bw >= aw * 1.15 && b_inset <= fw * 0.08;
    let centers_close = ((a.0 + a.2) as f64 - (b.0 + b.2) as f64).abs() <= fw * 0.12;
    let comparable = bw >= aw * 0.90 && b_inset >= fw * 0.04 && centers_close;
    first_centered && (second_wider || comparable)
}

fn crop_line(rgb: &RgbImage, mask: &Mask, top: usize, bottom: usize) -> Option<LineCrop> {
    let (w, h) = (rgb.width() as i64, rgb.height() as i64);
    let (x1, _, x2, _) = mask.bounds(top, bottom)?;
    let y_pad = 5.max(r((bottom - top + 1) as f64 * 0.22));
    let x_pad = 8.max(r(w as f64 * 0.015));
    let t = (top as i64 - y_pad).max(0);
    let b = (bottom as i64 + y_pad + 1).min(h);
    let l = (x1 as i64 - x_pad).max(0);
    let rr = (x2 as i64 + x_pad + 1).min(w);
    if b - t < 8 || rr - l < 12 {
        return None;
    }
    Some(LineCrop {
        image: crop(rgb, l, t, rr, b)?,
        bbox: (l as u32, t as u32, rr as u32, b as u32),
    })
}

fn has_large_vertical_delimiter(mask: &Mask) -> bool {
    let (w, h) = (mask.w, mask.h);
    if h < 48 || w < 80 {
        return false;
    }
    let Some((_, y1, _, y2)) = mask.bounds(0, h - 1) else {
        return false;
    };
    let content_h = (y2 - y1 + 1) as f64;
    if content_h < h as f64 * 0.42 {
        return false;
    }
    let window = 6.max(r(w as f64 * 0.035)) as usize;
    let cols = mask.col_counts(0, h - 1);
    let tall = 12.max(r(content_h * 0.42)) as usize;
    let left = cols[..window.min(w)].iter().any(|&c| c >= tall);
    let right = cols[w.saturating_sub(window)..].iter().any(|&c| c >= tall);
    let interior = if w > window * 2 {
        &cols[window..w - window]
    } else {
        &cols[..]
    };
    left || right || interior.iter().filter(|&&c| c >= tall).count() >= 2
}

fn crop_segment(seg: &RgbImage, offset_x: u32, base: (u32, u32)) -> Option<LineCrop> {
    let mask = ink_mask(seg);
    let (x1, y1, x2, y2) = mask.bounds(0, mask.h.checked_sub(1)?)?;
    let (w, h) = (seg.width() as i64, seg.height() as i64);
    let t = (y1 as i64 - 4).max(0);
    let b = (y2 as i64 + 5).min(h);
    let l = (x1 as i64 - 6).max(0);
    let rr = (x2 as i64 + 7).min(w);
    if b - t < 8 || rr - l < 16 {
        return None;
    }
    Some(LineCrop {
        image: crop(seg, l, t, rr, b)?,
        bbox: (
            base.0 + offset_x + l as u32,
            base.1 + t as u32,
            base.0 + offset_x + rr as u32,
            base.1 + b as u32,
        ),
    })
}

fn split_wide_line(line: LineCrop) -> Vec<LineCrop> {
    let (w, h) = (line.image.width() as usize, line.image.height() as usize);
    if w < 220 || (w as f64 / h.max(1) as f64) < WIDE_LINE_ASPECT {
        return vec![line];
    }
    let mask = ink_mask(&line.image);
    if has_large_vertical_delimiter(&mask) {
        return vec![line];
    }
    let cols = mask.col_counts(0, h - 1);
    let bands = row_bands(&cols.iter().map(|&c| c > 0).collect::<Vec<_>>());
    if bands.len() < 2 {
        return vec![line];
    }
    let rows: Vec<usize> = (0..h).filter(|&y| mask.row_count(y) > 0).collect();
    let ink_h = (rows[rows.len() - 1] - rows[0] + 1) as i64;
    let min_gap = 6.max(r(ink_h as f64 * 0.45));
    let min_seg = 48.max(ink_h * 4);
    let min_rem = 48.max(ink_h * 3);
    let mut splits: Vec<i64> = Vec::new();
    for pair in bands.windows(2) {
        let (right_a, left_b) = (pair[0].1 as i64, pair[1].0 as i64);
        let gap = left_b - right_a - 1;
        if gap < min_gap {
            continue;
        }
        let at = right_a + 1 + gap / 2;
        if at - (bands[0].0 as i64) < min_seg || bands[bands.len() - 1].1 as i64 - at < min_rem {
            continue;
        }
        if splits.last().is_some_and(|&p| at - p < min_seg) {
            continue;
        }
        splits.push(at);
    }
    if splits.is_empty() {
        return vec![line];
    }
    let mut segments = Vec::new();
    let mut left = 0i64;
    for right in splits.into_iter().chain([w as i64]) {
        if let Some(part) = crop(&line.image, left, 0, right, h as i64) {
            if let Some(seg) = crop_segment(&part, left as u32, (line.bbox.0, line.bbox.1)) {
                segments.push(seg);
            }
        }
        left = right;
    }
    if segments.len() > 1 {
        segments
    } else {
        vec![line]
    }
}

fn split_rows(rgb: &RgbImage) -> Vec<LineCrop> {
    let (w, h) = (rgb.width() as usize, rgb.height() as usize);
    if h < 72 || w < 24 {
        return vec![];
    }
    let mask = ink_mask(rgb);
    let threshold = 3.max(r(w as f64 * 0.006)) as usize;
    let has: Vec<bool> = (0..h).map(|y| mask.row_count(y) >= threshold).collect();
    let bands = merge_close_bands(row_bands(&has), 3.max(14.min(r(h as f64 * 0.018))));
    let bands: Vec<_> = bands
        .into_iter()
        .filter(|&(t, b)| band_looks_like_row(&mask, t, b, w))
        .collect();
    let bands = filter_annotation_bands(&mask, bands, w);
    if looks_like_compact_fraction(&mask, &bands, w, h) || bands.len() < 2 {
        return vec![];
    }
    bands
        .into_iter()
        .filter_map(|(t, b)| crop_line(rgb, &mask, t, b))
        .collect()
}

/// Rows of a multi-line formula, each possibly split into wide segments.
/// Empty when the image should be recognized as a single unit.
pub fn split_line_groups(image: &RgbImage) -> Vec<LineGroup> {
    let mut rgb = image.clone();
    normalize_polarity(&mut rgb);
    let rows = split_rows(&rgb);
    if rows.is_empty() {
        let (w, h) = rgb.dimensions();
        let segs = split_wide_line(LineCrop {
            image: rgb,
            bbox: (0, 0, w, h),
        });
        return if segs.len() > 1 { vec![segs] } else { vec![] };
    }
    rows.into_iter().map(split_wide_line).collect()
}

/// Fraction of ink pixels covered by any of the given boxes.
pub fn ink_coverage(image: &RgbImage, boxes: &[[f64; 4]]) -> f64 {
    let mut rgb = image.clone();
    normalize_polarity(&mut rgb);
    let mask = ink_mask(&rgb);
    let total = mask.count();
    if total == 0 || boxes.is_empty() {
        return 0.0;
    }
    let mut covered = vec![false; mask.w * mask.h];
    for b in boxes {
        let l = (b[0].floor().max(0.0) as usize).min(mask.w);
        let t = (b[1].floor().max(0.0) as usize).min(mask.h);
        let rr = (b[2].ceil().max(0.0) as usize).min(mask.w);
        let bb = (b[3].ceil().max(0.0) as usize).min(mask.h);
        for y in t..bb {
            for x in l..rr {
                covered[y * mask.w + x] = true;
            }
        }
    }
    let hit = mask
        .data
        .iter()
        .zip(&covered)
        .filter(|(m, c)| **m && **c)
        .count();
    hit as f64 / total as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    fn canvas(w: u32, h: u32) -> RgbImage {
        RgbImage::from_pixel(w, h, Rgb([255, 255, 255]))
    }

    fn bar(img: &mut RgbImage, x: std::ops::Range<u32>, y: std::ops::Range<u32>) {
        for yy in y {
            for xx in x.clone() {
                img.put_pixel(xx, yy, Rgb([0, 0, 0]));
            }
        }
    }

    #[test]
    fn single_line_is_not_split() {
        let mut img = canvas(300, 80);
        bar(&mut img, 20..280, 30..50);
        assert!(split_line_groups(&img).is_empty());
    }

    #[test]
    fn two_rows_are_split() {
        let mut img = canvas(300, 160);
        bar(&mut img, 20..280, 20..45);
        bar(&mut img, 20..260, 100..125);
        let groups = split_line_groups(&img);
        assert_eq!(groups.len(), 2);
        assert!(groups[0][0].bbox.1 < groups[1][0].bbox.1);
    }

    #[test]
    fn wide_line_splits_on_large_gaps() {
        let mut img = canvas(1000, 60);
        bar(&mut img, 20..300, 20..40);
        bar(&mut img, 400..700, 20..40);
        bar(&mut img, 800..980, 20..40);
        let groups = split_line_groups(&img);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 3);
    }

    #[test]
    fn small_images_never_split() {
        let img = canvas(20, 20);
        assert!(split_line_groups(&img).is_empty());
    }

    #[test]
    fn coverage_counts_ink_inside_boxes() {
        let mut img = canvas(100, 100);
        bar(&mut img, 10..30, 10..30);
        bar(&mut img, 60..80, 60..80);
        let c = ink_coverage(&img, &[[0.0, 0.0, 50.0, 50.0]]);
        assert!((c - 0.5).abs() < 0.01, "{c}");
        assert_eq!(ink_coverage(&img, &[]), 0.0);
    }

    #[test]
    fn dark_frame_is_removed_from_mask() {
        let mut img = canvas(200, 200);
        bar(&mut img, 0..200, 0..8);
        bar(&mut img, 0..200, 192..200);
        bar(&mut img, 0..8, 0..200);
        bar(&mut img, 192..200, 0..200);
        bar(&mut img, 80..120, 90..110);
        let mask = ink_mask(&img);
        assert!(!mask.at(4, 100));
        assert!(mask.at(100, 100));
    }
}
