//! MathCraft OCR, reimplemented natively on ONNX Runtime.
//!
//! Three modes mirror the original desktop app:
//! * `formula` – the whole image is one (possibly multi-line) formula
//! * `mixed`   – formula detection + text OCR merged into Markdown
//! * `text`    – plain text OCR

mod decoder;
pub mod geometry;
pub mod imageops;
pub mod latex;
pub mod layout;
pub mod lines;
pub mod models;
pub mod tokenizer;

use anyhow::{bail, Result};
use image::RgbImage;
use layout::Block;
use models::{FormulaDetector, FormulaRecognizer, TextDetector, TextRecognizer};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Formula,
    Mixed,
    Text,
}

impl Mode {
    pub fn required_models(self) -> &'static [&'static str] {
        match self {
            Mode::Formula => &[models::FORMULA_REC],
            Mode::Mixed => &[
                models::FORMULA_DET,
                models::FORMULA_REC,
                models::TEXT_DET,
                models::TEXT_REC,
            ],
            Mode::Text => &[models::TEXT_DET, models::TEXT_REC],
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Recognition {
    pub text: String,
    pub score: f64,
    pub mode: Mode,
    pub flags: Vec<&'static str>,
}

const LOW_SEGMENT_CONFIDENCE: f64 = 0.95;
const WHOLE_LINE_MARGIN: f64 = 0.02;
const FORMULA_DOMINANT_COVERAGE: f64 = 0.95;

pub struct Engine {
    root: PathBuf,
    formula_det: FormulaDetector,
    formula_rec: FormulaRecognizer,
    text_det: TextDetector,
    text_rec: TextRecognizer,
}

impl Engine {
    pub fn new(model_root: &Path) -> Self {
        let dir = |id: &str| model_root.join(id);
        Self {
            root: model_root.to_path_buf(),
            formula_det: FormulaDetector::new(dir(models::FORMULA_DET)),
            formula_rec: FormulaRecognizer::new(dir(models::FORMULA_REC)),
            text_det: TextDetector::new(dir(models::TEXT_DET)),
            text_rec: TextRecognizer::new(dir(models::TEXT_REC)),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn warmup_formula(&self) -> Result<()> {
        self.formula_rec.warmup()
    }

    pub fn recognize(&self, image: &RgbImage, mode: Mode) -> Result<Recognition> {
        if image.width() < 2 || image.height() < 2 {
            bail!("image is too small to recognize");
        }
        let mut rgb = image.clone();
        let (text, score) = match mode {
            Mode::Formula => {
                imageops::normalize_polarity(&mut rgb);
                self.formula(&rgb)?
            }
            Mode::Mixed => {
                imageops::normalize_polarity(&mut rgb);
                self.mixed(&rgb)?
            }
            Mode::Text => self.text(&rgb)?,
        };
        let flags = if mode == Mode::Text {
            vec![]
        } else {
            latex::quality_flags(&text)
        };
        Ok(Recognition {
            text,
            score,
            mode,
            flags,
        })
    }

    fn single(&self, img: &RgbImage) -> Result<(String, f64)> {
        Ok(self
            .formula_rec
            .recognize(std::slice::from_ref(img))?
            .remove(0))
    }

    /// Whole-image formula recognition with multi-line splitting and repair.
    fn formula(&self, rgb: &RgbImage) -> Result<(String, f64)> {
        let groups = lines::split_line_groups(rgb);
        if groups.is_empty() {
            return self.single(rgb);
        }
        let crops: Vec<RgbImage> = groups.iter().flatten().map(|c| c.image.clone()).collect();
        let flat = self.formula_rec.recognize(&crops)?;
        let mut grouped: Vec<Vec<(String, f64)>> = Vec::new();
        let mut it = flat.into_iter();
        for g in &groups {
            grouped.push(it.by_ref().take(g.len()).collect());
        }
        // Re-read a whole line when its segments look unreliable.
        for (g, results) in groups.iter().zip(grouped.iter_mut()) {
            if g.len() <= 1 {
                continue;
            }
            let line_text = latex::compose_formula_line(
                &results.iter().map(|r| r.0.clone()).collect::<Vec<_>>(),
            );
            let low = results.iter().any(|r| r.1 < LOW_SEGMENT_CONFIDENCE);
            if !low && !latex::has_severe_issue(&line_text) {
                continue;
            }
            let l = g.iter().map(|c| c.bbox.0).min().unwrap();
            let t = g.iter().map(|c| c.bbox.1).min().unwrap();
            let r = g.iter().map(|c| c.bbox.2).max().unwrap();
            let b = g.iter().map(|c| c.bbox.3).max().unwrap();
            let Some(crop) = imageops::crop(rgb, l as i64, t as i64, r as i64, b as i64) else {
                continue;
            };
            let (fb_text, fb_score) = self.single(&crop)?;
            let line_score = mean(results);
            let more_confident = low
                && !latex::has_severe_issue(&fb_text)
                && fb_score > line_score + WHOLE_LINE_MARGIN;
            if prefer_fallback(&line_text, line_score, &fb_text, fb_score) || more_confident {
                *results = vec![(fb_text, fb_score)];
            }
        }
        let (text, score) = merge_groups(&grouped);
        if latex::has_severe_issue(&text) {
            let (fb_text, fb_score) = self.single(rgb)?;
            if prefer_fallback(&text, score, &fb_text, fb_score) {
                return Ok((fb_text, fb_score));
            }
        }
        Ok((text, score))
    }

    fn text_blocks(
        &self,
        source: &RgbImage,
        masked: &RgbImage,
        formulas: &[[f64; 4]],
    ) -> Result<Vec<Block>> {
        let mut segments = Vec::new();
        for b in self.text_det.detect(source)? {
            if !informative(source, &b, 6.0, 48.0, 249.0, 4.0) {
                continue;
            }
            for seg in layout::split_text_around_formulas(&b, formulas) {
                if informative(masked, &seg, 6.0, 48.0, 249.0, 4.0) {
                    segments.push(seg);
                }
            }
        }
        let crops: Vec<RgbImage> = segments
            .iter()
            .filter_map(|s| line_crop(masked, s))
            .collect();
        if crops.len() != segments.len() {
            segments.retain(|s| line_crop(masked, s).is_some());
        }
        let results = self.text_rec.recognize(&crops)?;
        Ok(segments
            .into_iter()
            .zip(results)
            .filter(|(_, (t, s))| !t.trim().is_empty() && *s >= 0.45)
            .map(|(xyxy, (text, score))| Block {
                kind: "text",
                xyxy,
                text: text.trim().to_string(),
                score,
            })
            .collect())
    }

    fn text(&self, rgb: &RgbImage) -> Result<(String, f64)> {
        let blocks = self.text_blocks(rgb, rgb, &[])?;
        let size = (rgb.width() as f64, rgb.height() as f64);
        Ok((layout::merge_text(&blocks, size), mean_blocks(&blocks)))
    }

    fn mixed(&self, rgb: &RgbImage) -> Result<(String, f64)> {
        let (w, h) = (rgb.width() as f64, rgb.height() as f64);
        let formulas: Vec<models::FormulaBox> = self
            .formula_det
            .detect(rgb)?
            .into_iter()
            .filter(|f| informative(rgb, &f.xyxy, 4.0, 24.0, 252.0, 3.0))
            .collect();
        let boxes: Vec<[f64; 4]> = formulas.iter().map(|f| f.xyxy).collect();
        if !formulas.is_empty()
            && formulas.iter().all(|f| f.label == "isolated")
            && lines::ink_coverage(rgb, &boxes) >= FORMULA_DOMINANT_COVERAGE
        {
            let (t, s) = self.formula(rgb)?;
            return Ok((format!("$$\n{t}\n$$"), s));
        }
        let margin = 2.0f64.max(imageops::round_half_even(w.max(h) / 640.0));
        let mut masked = rgb.clone();
        for b in &boxes {
            let (l, t) = (
                (b[0] - margin).max(0.0) as u32,
                (b[1] - margin).max(0.0) as u32,
            );
            let (r, bt) = ((b[2] + margin).min(w) as u32, (b[3] + margin).min(h) as u32);
            for y in t..bt {
                for x in l..r {
                    masked.put_pixel(x, y, image::Rgb([255, 255, 255]));
                }
            }
        }
        let mut blocks = self.text_blocks(rgb, &masked, &boxes)?;

        // Formula crops: each split into rows/segments, recognized in one batch.
        let mut jobs: Vec<(usize, usize, RgbImage)> = Vec::new();
        let mut layouts: Vec<usize> = Vec::new();
        for (i, f) in formulas.iter().enumerate() {
            let Some(crop) = imageops::crop(
                rgb,
                f.xyxy[0] as i64,
                f.xyxy[1] as i64,
                f.xyxy[2] as i64,
                f.xyxy[3] as i64,
            ) else {
                layouts.push(0);
                continue;
            };
            let groups = lines::split_line_groups(&crop);
            if groups.is_empty() {
                layouts.push(1);
                jobs.push((i, 0, crop));
            } else {
                layouts.push(groups.len());
                for (li, g) in groups.into_iter().enumerate() {
                    jobs.extend(g.into_iter().map(|c| (i, li, c.image)));
                }
            }
        }
        let images: Vec<RgbImage> = jobs.iter().map(|j| j.2.clone()).collect();
        let results = self.formula_rec.recognize(&images)?;
        let mut grouped: Vec<Vec<Vec<(String, f64)>>> =
            layouts.iter().map(|&n| vec![vec![]; n]).collect();
        for ((i, li, _), r) in jobs.iter().zip(results) {
            grouped[*i][*li].push(r);
        }
        for (f, g) in formulas.iter().zip(grouped) {
            if g.is_empty() {
                continue;
            }
            let (text, score) = merge_groups(&g);
            blocks.push(Block {
                kind: f.label,
                xyxy: f.xyxy,
                text,
                score: f.score.min(score),
            });
        }
        if blocks.is_empty() {
            let (t, s) = self.formula(rgb)?;
            return Ok((t, s));
        }
        let blocks = layout::resolve_conflicts(blocks);
        Ok((layout::merge_text(&blocks, (w, h)), mean_blocks(&blocks)))
    }
}

fn mean(r: &[(String, f64)]) -> f64 {
    if r.is_empty() {
        0.0
    } else {
        r.iter().map(|x| x.1).sum::<f64>() / r.len() as f64
    }
}

fn mean_blocks(b: &[Block]) -> f64 {
    if b.is_empty() {
        0.0
    } else {
        b.iter().map(|x| x.score).sum::<f64>() / b.len() as f64
    }
}

pub fn merge_groups(groups: &[Vec<(String, f64)>]) -> (String, f64) {
    let lines: Vec<String> = groups
        .iter()
        .map(|g| latex::compose_formula_line(&g.iter().map(|r| r.0.clone()).collect::<Vec<_>>()))
        .collect();
    let all: Vec<(String, f64)> = groups.iter().flatten().cloned().collect();
    (latex::compose_aligned_formula(&lines), mean(&all))
}

/// Prefer a whole-image re-read when it has fewer severe LaTeX problems.
pub fn prefer_fallback(cand: &str, cand_score: f64, fb: &str, fb_score: f64) -> bool {
    let (c, f) = (latex::quality_flags(cand), latex::quality_flags(fb));
    let severe = |v: &[&str]| -> Vec<String> {
        let mut s: Vec<String> = v
            .iter()
            .filter(|x| latex::SEVERE_FLAGS.contains(x))
            .map(|x| x.to_string())
            .collect();
        s.sort();
        s
    };
    let (cs, fs) = (severe(&c), severe(&f));
    if fs.len() < cs.len() {
        return fb_score >= cand_score * 0.78;
    }
    if fs == cs {
        return fb_score > cand_score + 0.04;
    }
    false
}

/// Reject tiny or blank regions before spending inference on them.
pub fn informative(
    img: &RgbImage,
    b: &[f64; 4],
    min_side: f64,
    min_area: f64,
    blank_mean: f64,
    blank_std: f64,
) -> bool {
    let (w, h) = (b[2] - b[0], b[3] - b[1]);
    if w < min_side || h < min_side || w * h < min_area {
        return false;
    }
    let l = (b[0].floor().max(0.0) as u32).min(img.width());
    let t = (b[1].floor().max(0.0) as u32).min(img.height());
    let r = (b[2].ceil().max(0.0) as u32).min(img.width());
    let bt = (b[3].ceil().max(0.0) as u32).min(img.height());
    if r <= l || bt <= t {
        return false;
    }
    let (m, s) = imageops::region_stats(img, l, t, r, bt);
    !(m >= blank_mean && s <= blank_std)
}

/// rapidocr `get_rotate_crop_image` for axis-aligned boxes.
fn line_crop(img: &RgbImage, b: &[f64; 4]) -> Option<RgbImage> {
    let (w, h) = ((b[2] - b[0]) as i64, (b[3] - b[1]) as i64);
    let crop = imageops::crop(
        img,
        b[0] as i64,
        b[1] as i64,
        b[0] as i64 + w,
        b[1] as i64 + h,
    )?;
    if crop.height() as f64 / crop.width() as f64 >= 1.5 {
        return Some(image::imageops::rotate270(&crop));
    }
    Some(crop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_preference() {
        assert!(prefer_fallback("{a", 0.9, "a", 0.8));
        assert!(!prefer_fallback("{a", 0.9, "a", 0.5));
        assert!(prefer_fallback("a", 0.5, "b", 0.6));
        assert!(!prefer_fallback("a", 0.5, "{b", 0.99));
    }

    #[test]
    fn merges_groups_into_aligned() {
        let g = vec![
            vec![("a = b".to_string(), 0.8)],
            vec![("= c".to_string(), 0.6)],
        ];
        let (t, s) = merge_groups(&g);
        assert_eq!(t, "\\begin{aligned}\na &= b \\\\\n&= c\n\\end{aligned}");
        assert!((s - 0.7).abs() < 1e-9);
    }

    #[test]
    fn blank_regions_are_not_informative() {
        let white = RgbImage::from_pixel(50, 50, image::Rgb([255, 255, 255]));
        assert!(!informative(
            &white,
            &[0.0, 0.0, 40.0, 40.0],
            6.0,
            48.0,
            249.0,
            4.0
        ));
        let mut ink = white.clone();
        ink.put_pixel(10, 10, image::Rgb([0, 0, 0]));
        ink.put_pixel(11, 10, image::Rgb([0, 0, 0]));
        assert!(informative(
            &ink,
            &[0.0, 0.0, 40.0, 40.0],
            6.0,
            48.0,
            249.0,
            4.0
        ));
        assert!(!informative(
            &ink,
            &[0.0, 0.0, 3.0, 40.0],
            6.0,
            48.0,
            249.0,
            4.0
        ));
    }

    #[test]
    fn modes_list_required_models() {
        assert_eq!(Mode::Formula.required_models(), &[models::FORMULA_REC]);
        assert_eq!(Mode::Mixed.required_models().len(), 4);
    }

    #[test]
    fn tall_crops_are_rotated() {
        let img = RgbImage::new(100, 100);
        let c = line_crop(&img, &[0.0, 0.0, 10.0, 40.0]).unwrap();
        assert_eq!(c.dimensions(), (40, 10));
    }
}
