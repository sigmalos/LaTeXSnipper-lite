//! ONNX Runtime wrappers for the four MathCraft models.

use super::geometry::{components, min_area_rect, mini_box, point_in_polygon, unclip_rect, Pt};
use super::imageops::{resize, round_half_even, to_chw};
use super::tokenizer::Tokenizer;
use anyhow::{anyhow, bail, Context, Result};
use image::{imageops::FilterType, Rgb, RgbImage};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const FORMULA_DET: &str = "mathcraft-formula-det";
pub const FORMULA_REC: &str = "mathcraft-formula-rec";
pub const TEXT_DET: &str = "mathcraft-text-det";
pub const TEXT_REC: &str = "mathcraft-text-rec";
pub const MAX_NEW_TOKENS: usize = 512;

fn open_session(path: &Path) -> Result<Session> {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(8);
    Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level3)?
        .with_intra_threads(threads)?
        .commit_from_file(path)
        .with_context(|| format!("failed to load {}", path.display()))
}

fn find_onnx(dir: &Path, needle: &str) -> Result<PathBuf> {
    let mut hits: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("missing model directory {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.ends_with(".onnx") && name.contains(needle)
        })
        .collect();
    hits.sort();
    hits.into_iter()
        .next()
        .ok_or_else(|| anyhow!("no *{needle}*.onnx under {}", dir.display()))
}

/// A lazily opened session guarded for `&mut` access during `run`.
struct Lazy {
    path: Result<PathBuf, String>,
    cell: OnceLock<Mutex<Session>>,
}

impl Lazy {
    fn new(path: Result<PathBuf>) -> Self {
        Self {
            path: path.map_err(|e| e.to_string()),
            cell: OnceLock::new(),
        }
    }
    fn get(&self) -> Result<&Mutex<Session>> {
        if let Some(s) = self.cell.get() {
            return Ok(s);
        }
        let path = self.path.as_ref().map_err(|e| anyhow!(e.clone()))?;
        let session = open_session(path)?;
        Ok(self.cell.get_or_init(|| Mutex::new(session)))
    }
}

fn f32_output(outputs: &ort::session::SessionOutputs<'_>) -> Result<(Vec<i64>, Vec<f32>)> {
    let (shape, data) = outputs[0].try_extract_tensor::<f32>()?;
    Ok((shape.to_vec(), data.to_vec()))
}

// ------------------------------------------------------------- formula rec

pub struct FormulaRecognizer {
    encoder: Lazy,
    decoder: Lazy,
    tokenizer: OnceLock<Result<Tokenizer, String>>,
    dir: PathBuf,
}

impl FormulaRecognizer {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            encoder: Lazy::new(Ok(dir.join("encoder_model.onnx"))),
            decoder: Lazy::new(Ok(dir.join("decoder_model.onnx"))),
            tokenizer: OnceLock::new(),
            dir,
        }
    }

    fn tokenizer(&self) -> Result<&Tokenizer> {
        self.tokenizer
            .get_or_init(|| Tokenizer::from_dir(&self.dir).map_err(|e| e.to_string()))
            .as_ref()
            .map_err(|e| anyhow!(e.clone()))
    }

    fn generation_ids(&self) -> (i64, Option<i64>) {
        let read = |name: &str| -> Option<serde_json::Value> {
            serde_json::from_str(
                std::fs::read_to_string(self.dir.join(name))
                    .ok()?
                    .trim_start_matches('\u{feff}'),
            )
            .ok()
        };
        let (mut start, mut eos) = (None, None);
        for v in [read("generation_config.json"), read("config.json")]
            .into_iter()
            .flatten()
        {
            start = start
                .or(v["decoder_start_token_id"].as_i64())
                .or(v["decoder"]["decoder_start_token_id"].as_i64());
            eos = eos
                .or(v["eos_token_id"].as_i64())
                .or(v["decoder"]["eos_token_id"].as_i64());
        }
        (start.unwrap_or(2), eos.or(Some(2)))
    }

    pub fn warmup(&self) -> Result<()> {
        self.encoder.get()?;
        self.decoder.get()?;
        self.tokenizer()?;
        Ok(())
    }

    /// Greedy-decode a batch of crops. Returns (latex, mean token probability).
    pub fn recognize(&self, images: &[RgbImage]) -> Result<Vec<(String, f64)>> {
        if images.is_empty() {
            return Ok(vec![]);
        }
        const SIZE: u32 = 384;
        let plane = (SIZE * SIZE * 3) as usize;
        let mut pixels = Vec::with_capacity(plane * images.len());
        for img in images {
            let mut img = img.clone();
            super::imageops::normalize_polarity(&mut img);
            let resized = resize(&img, SIZE, SIZE, FilterType::CatmullRom);
            pixels.extend(to_chw(&resized, 1.0 / 255.0, [0.5; 3], [0.5; 3], false));
        }
        let batch = images.len();
        let (hidden_shape, hidden) = {
            let mut enc = self.encoder.get()?.lock().unwrap();
            let input =
                Tensor::from_array((vec![batch as i64, 3, SIZE as i64, SIZE as i64], pixels))?;
            let out = enc.run(ort::inputs![input])?;
            f32_output(&out)?
        };
        let (seq, dim) = (hidden_shape[1] as usize, hidden_shape[2] as usize);
        let per_row = seq * dim;
        let (start_id, eos) = self.generation_ids();

        let mut active: Vec<usize> = (0..batch).collect();
        let mut active_hidden = hidden;
        let mut ids: Vec<Vec<i64>> = vec![vec![]; batch];
        let mut scores: Vec<Vec<f64>> = vec![vec![]; batch];
        let mut repeated = vec![false; batch];
        let mut dec = self.decoder.get()?.lock().unwrap();
        for step in 0..MAX_NEW_TOKENS {
            let n = active.len();
            let len = step + 1;
            let mut input_ids = Vec::with_capacity(n * len);
            for &row in &active {
                input_ids.push(start_id);
                input_ids.extend(&ids[row]);
            }
            let id_tensor = Tensor::from_array((vec![n as i64, len as i64], input_ids))?;
            let hid_tensor = Tensor::from_array((
                vec![n as i64, seq as i64, dim as i64],
                active_hidden.clone(),
            ))?;
            let out = dec.run(ort::inputs![id_tensor, hid_tensor])?;
            let (shape, logits) = f32_output(&out)?;
            let vocab = shape[2] as usize;
            let mut keep = Vec::new();
            for (a, &row) in active.iter().enumerate() {
                let offset = (a * len + len - 1) * vocab;
                let (tok, prob) = argmax_softmax(&logits[offset..offset + vocab]);
                if Some(tok as i64) == eos {
                    continue;
                }
                ids[row].push(tok as i64);
                scores[row].push(prob);
                if let Some(cut) = repeated_suffix_start(&ids[row]) {
                    ids[row].truncate(cut);
                    scores[row].truncate(cut);
                    repeated[row] = true;
                    continue;
                }
                keep.push(a);
            }
            if keep.is_empty() {
                break;
            }
            if keep.len() != n {
                active_hidden = keep
                    .iter()
                    .flat_map(|&a| active_hidden[a * per_row..(a + 1) * per_row].to_vec())
                    .collect();
                active = keep.iter().map(|&a| active[a]).collect();
            }
        }
        drop(dec);
        let tok = self.tokenizer()?;
        Ok((0..batch)
            .map(|row| {
                let text = tok
                    .decode(&ids[row].iter().map(|&i| i as u32).collect::<Vec<_>>())
                    .trim()
                    .to_string();
                let mut score = if scores[row].is_empty() {
                    0.0
                } else {
                    scores[row].iter().sum::<f64>() / scores[row].len() as f64
                };
                if repeated[row] {
                    score = score.min(0.5);
                }
                (text, score)
            })
            .collect())
    }
}

fn argmax_softmax(logits: &[f32]) -> (usize, f64) {
    let (mut best, mut max) = (0, f32::NEG_INFINITY);
    for (i, &v) in logits.iter().enumerate() {
        if v > max {
            max = v;
            best = i;
        }
    }
    let sum: f64 = logits.iter().map(|&v| ((v - max) as f64).exp()).sum();
    (best, 1.0 / sum)
}

/// Detect degenerate decoding loops (e.g. `\quad \quad ...`) and return the cut point.
pub fn repeated_suffix_start(ids: &[i64]) -> Option<usize> {
    let n = ids.len();
    if n < 64 {
        return None;
    }
    for period in 1..=4 {
        let pattern = &ids[n - period..];
        let mut start = n - period;
        let mut reps = 1;
        while start >= period && &ids[start - period..start] == pattern {
            start -= period;
            reps += 1;
        }
        if reps >= 8 && reps * period >= 32 {
            return Some(start + period);
        }
    }
    None
}

// ------------------------------------------------------------- formula det

#[derive(Clone, Debug, PartialEq)]
pub struct FormulaBox {
    pub xyxy: [f64; 4],
    pub score: f64,
    /// "embedding" (inline) or "isolated" (display)
    pub label: &'static str,
}

pub struct FormulaDetector {
    session: Lazy,
}

impl FormulaDetector {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            session: Lazy::new(find_onnx(&dir, "mfd")),
        }
    }

    pub fn detect(&self, img: &RgbImage) -> Result<Vec<FormulaBox>> {
        const SIZE: u32 = 768;
        let (w, h) = img.dimensions();
        let scale = (SIZE as f64 / w as f64).min(SIZE as f64 / h as f64);
        let (nw, nh) = (
            round_half_even(w as f64 * scale) as u32,
            round_half_even(h as f64 * scale) as u32,
        );
        let resized = resize(img, nw.max(1), nh.max(1), FilterType::Triangle);
        let mut canvas = RgbImage::from_pixel(SIZE, SIZE, Rgb([114, 114, 114]));
        let left = round_half_even((SIZE - nw) as f64 / 2.0 - 0.1) as i64;
        let top = round_half_even((SIZE - nh) as f64 / 2.0 - 0.1) as i64;
        image::imageops::replace(&mut canvas, &resized, left, top);
        let input = Tensor::from_array((
            vec![1i64, 3, SIZE as i64, SIZE as i64],
            to_chw(&canvas, 1.0 / 255.0, [0.0; 3], [1.0; 3], false),
        ))?;
        let (shape, data) = {
            let mut s = self.session.get()?.lock().unwrap();
            let out = s.run(ort::inputs![input])?;
            f32_output(&out)?
        };
        let (channels, n) = (shape[1] as usize, shape[2] as usize);
        if channels < 6 {
            return Ok(vec![]);
        }
        let at = |c: usize, i: usize| data[c * n + i] as f64;
        let mut cands: Vec<FormulaBox> = Vec::new();
        for i in 0..n {
            let (cls, score) = (4..channels)
                .map(|c| (c - 4, at(c, i)))
                .fold((0, f64::MIN), |b, x| if x.1 > b.1 { x } else { b });
            if score < 0.25 {
                continue;
            }
            let (cx, cy, bw, bh) = (at(0, i), at(1, i), at(2, i), at(3, i));
            let fx = |v: f64| ((v - left as f64) / scale).clamp(0.0, w as f64);
            let fy = |v: f64| ((v - top as f64) / scale).clamp(0.0, h as f64);
            cands.push(FormulaBox {
                xyxy: [
                    fx(cx - bw / 2.0),
                    fy(cy - bh / 2.0),
                    fx(cx + bw / 2.0),
                    fy(cy + bh / 2.0),
                ],
                score,
                label: if cls == 0 { "embedding" } else { "isolated" },
            });
        }
        Ok(nms(cands, 0.45))
    }
}

pub fn nms(mut boxes: Vec<FormulaBox>, iou_threshold: f64) -> Vec<FormulaBox> {
    boxes.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    let area = |b: &[f64; 4]| (b[2] - b[0]).max(0.0) * (b[3] - b[1]).max(0.0);
    let mut keep: Vec<FormulaBox> = Vec::new();
    for b in boxes {
        let suppressed = keep.iter().any(|k| {
            let iw = (k.xyxy[2].min(b.xyxy[2]) - k.xyxy[0].max(b.xyxy[0])).max(0.0);
            let ih = (k.xyxy[3].min(b.xyxy[3]) - k.xyxy[1].max(b.xyxy[1])).max(0.0);
            let inter = iw * ih;
            let union = area(&k.xyxy) + area(&b.xyxy) - inter;
            union > 0.0 && inter / union > iou_threshold
        });
        if !suppressed {
            keep.push(b);
        }
    }
    keep
}

// ---------------------------------------------------------------- text det

pub struct TextDetector {
    session: Lazy,
}

impl TextDetector {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            session: Lazy::new(find_onnx(&dir, "det")),
        }
    }

    /// Axis-aligned text line boxes `[x1, y1, x2, y2]` in reading order.
    pub fn detect(&self, img: &RgbImage) -> Result<Vec<[f64; 4]>> {
        let (w, h) = img.dimensions();
        let limit = w.max(h).min(960) as f64;
        let ratio = if w.max(h) as f64 > limit {
            limit / w.max(h) as f64
        } else {
            1.0
        };
        let rw = (round_half_even((w as f64 * ratio).floor() / 32.0) * 32.0) as u32;
        let rh = (round_half_even((h as f64 * ratio).floor() / 32.0) * 32.0) as u32;
        if rw == 0 || rh == 0 {
            return Ok(vec![]);
        }
        let resized = resize(img, rw, rh, FilterType::Triangle);
        let input = Tensor::from_array((
            vec![1i64, 3, rh as i64, rw as i64],
            to_chw(&resized, 1.0 / 255.0, [0.5; 3], [0.5; 3], true),
        ))?;
        let (shape, pred) = {
            let mut s = self.session.get()?.lock().unwrap();
            let out = s.run(ort::inputs![input])?;
            f32_output(&out)?
        };
        let (ph, pw) = (shape[2] as usize, shape[3] as usize);
        Ok(db_postprocess(&pred, pw, ph, w as usize, h as usize))
    }
}

/// Differentiable-Binarization post-processing (thresh 0.3, box 0.5, unclip 1.6, dilation).
pub fn db_postprocess(
    pred: &[f32],
    pw: usize,
    ph: usize,
    dest_w: usize,
    dest_h: usize,
) -> Vec<[f64; 4]> {
    let seg: Vec<bool> = pred.iter().map(|&p| p > 0.3).collect();
    // 2x2 dilation anchored at the kernel centre, like cv2.dilate(np.ones((2,2))).
    let mut bitmap = vec![false; pw * ph];
    for y in 0..ph {
        for x in 0..pw {
            bitmap[y * pw + x] = seg[y * pw + x]
                || (x > 0 && seg[y * pw + x - 1])
                || (y > 0 && seg[(y - 1) * pw + x])
                || (x > 0 && y > 0 && seg[(y - 1) * pw + x - 1]);
        }
    }
    let mut quads: Vec<[Pt; 4]> = Vec::new();
    for comp in components(&bitmap, pw, ph).into_iter().take(1000) {
        let (rect, short) = min_area_rect(&comp);
        if short < 3.0 {
            continue;
        }
        let points = mini_box(rect);
        if box_score_fast(pred, pw, ph, &points) < 0.5 {
            continue;
        }
        let (grown, short) = min_area_rect(&unclip_rect(points, 1.6));
        if short < 5.0 {
            continue;
        }
        let quad = mini_box(grown).map(|(x, y)| {
            (
                round_half_even(x / pw as f64 * dest_w as f64)
                    .clamp(0.0, dest_w as f64)
                    .trunc(),
                round_half_even(y / ph as f64 * dest_h as f64)
                    .clamp(0.0, dest_h as f64)
                    .trunc(),
            )
        });
        let quad = quad.map(|(x, y)| {
            (
                x.min(dest_w as f64 - 1.0).max(0.0).trunc(),
                y.min(dest_h as f64 - 1.0).max(0.0).trunc(),
            )
        });
        let width = ((quad[0].0 - quad[1].0).powi(2) + (quad[0].1 - quad[1].1).powi(2)).sqrt();
        let height = ((quad[0].0 - quad[3].0).powi(2) + (quad[0].1 - quad[3].1).powi(2)).sqrt();
        if width.trunc() <= 3.0 || height.trunc() <= 3.0 {
            continue;
        }
        quads.push(quad);
    }
    // rapidocr sorted_boxes: stable by top-left y, then by x inside rows (Δy < 10).
    quads.sort_by(|a, b| a[0].1.partial_cmp(&b[0].1).unwrap());
    let mut line = 0;
    let mut keyed: Vec<(usize, f64, [Pt; 4])> = Vec::new();
    for (i, q) in quads.iter().enumerate() {
        if i > 0 && q[0].1 - quads[i - 1][0].1 >= 10.0 {
            line += 1;
        }
        keyed.push((line, q[0].0, *q));
    }
    keyed.sort_by(|a, b| (a.0, a.1).partial_cmp(&(b.0, b.1)).unwrap());
    keyed
        .into_iter()
        .map(|(_, _, q)| {
            let xs = q.iter().map(|p| p.0);
            let ys = q.iter().map(|p| p.1);
            [
                xs.clone().fold(f64::MAX, f64::min),
                ys.clone().fold(f64::MAX, f64::min),
                xs.fold(f64::MIN, f64::max),
                ys.fold(f64::MIN, f64::max),
            ]
        })
        .collect()
}

fn box_score_fast(pred: &[f32], w: usize, h: usize, quad: &[Pt; 4]) -> f64 {
    let xs = quad.iter().map(|p| p.0);
    let ys = quad.iter().map(|p| p.1);
    let xmin = (xs.clone().fold(f64::MAX, f64::min).floor() as i64).clamp(0, w as i64 - 1) as usize;
    let xmax = (xs.fold(f64::MIN, f64::max).ceil() as i64).clamp(0, w as i64 - 1) as usize;
    let ymin = (ys.clone().fold(f64::MAX, f64::min).floor() as i64).clamp(0, h as i64 - 1) as usize;
    let ymax = (ys.fold(f64::MIN, f64::max).ceil() as i64).clamp(0, h as i64 - 1) as usize;
    let poly: Vec<Pt> = quad.iter().map(|p| (p.0.trunc(), p.1.trunc())).collect();
    let (mut sum, mut n) = (0f64, 0usize);
    for y in ymin..=ymax {
        for x in xmin..=xmax {
            let (fx, fy) = (x as f64, y as f64);
            if point_in_polygon(fx + 0.5, fy + 0.5, &poly)
                || poly.iter().any(|p| p.0 == fx && p.1 == fy)
            {
                sum += pred[y * w + x] as f64;
                n += 1;
            }
        }
    }
    if n == 0 {
        0.0
    } else {
        sum / n as f64
    }
}

// ---------------------------------------------------------------- text rec

pub struct TextRecognizer {
    session: Lazy,
    keys: Result<Vec<String>, String>,
}

impl TextRecognizer {
    pub fn new(dir: PathBuf) -> Self {
        let keys = std::fs::read_to_string(dir.join("ppocrv5_keys.txt"))
            .map(|s| {
                let mut v = vec!["blank".to_string()];
                v.extend(s.split('\n').map(|l| l.trim_end_matches('\r').to_string()));
                if v.last().is_some_and(|l| l.is_empty()) {
                    v.pop();
                }
                v.push(" ".into());
                v
            })
            .map_err(|e| format!("missing PP-OCR vocabulary: {e}"));
        Self {
            session: Lazy::new(find_onnx(&dir, "rec")),
            keys,
        }
    }

    /// CRNN/CTC recognition of RGB line crops (fed to the model as BGR).
    pub fn recognize(&self, crops: &[RgbImage]) -> Result<Vec<(String, f64)>> {
        let keys = self.keys.as_ref().map_err(|e| anyhow!(e.clone()))?;
        let mut order: Vec<usize> = (0..crops.len()).collect();
        let ratio = |i: usize| crops[i].width() as f64 / crops[i].height().max(1) as f64;
        order.sort_by(|&a, &b| ratio(a).partial_cmp(&ratio(b)).unwrap());
        let mut results = vec![(String::new(), 0.0); crops.len()];
        for chunk in order.chunks(6) {
            let max_ratio = chunk.iter().map(|&i| ratio(i)).fold(320.0 / 48.0, f64::max);
            let img_w = (48.0 * max_ratio) as usize;
            let mut batch = vec![0f32; chunk.len() * 3 * 48 * img_w];
            for (b, &i) in chunk.iter().enumerate() {
                let rw = ((48.0 * ratio(i)).ceil() as usize).min(img_w).max(1);
                let resized = resize(&crops[i], rw as u32, 48, FilterType::Triangle);
                let chw = to_chw(&resized, 1.0 / 255.0, [0.5; 3], [0.5; 3], true);
                for c in 0..3 {
                    for y in 0..48 {
                        let dst = ((b * 3 + c) * 48 + y) * img_w;
                        let src = (c * 48 + y) * rw;
                        batch[dst..dst + rw].copy_from_slice(&chw[src..src + rw]);
                    }
                }
            }
            let input = Tensor::from_array((vec![chunk.len() as i64, 3, 48, img_w as i64], batch))?;
            let (shape, probs) = {
                let mut s = self.session.get()?.lock().unwrap();
                let out = s.run(ort::inputs![input])?;
                f32_output(&out)?
            };
            let (steps, classes) = (shape[1] as usize, shape[2] as usize);
            if classes != keys.len() {
                bail!(
                    "text recognizer has {classes} classes but vocabulary has {}",
                    keys.len()
                );
            }
            for (b, &i) in chunk.iter().enumerate() {
                results[i] = ctc_decode(
                    &probs[b * steps * classes..(b + 1) * steps * classes],
                    classes,
                    keys,
                );
            }
        }
        Ok(results)
    }
}

pub fn ctc_decode(probs: &[f32], classes: usize, keys: &[String]) -> (String, f64) {
    let mut text = String::new();
    let mut confs = Vec::new();
    let mut prev = usize::MAX;
    for step in probs.chunks(classes) {
        let (idx, p) =
            step.iter()
                .enumerate()
                .fold((0, f32::MIN), |b, (i, &v)| if v > b.1 { (i, v) } else { b });
        if idx != prev && idx != 0 {
            text.push_str(&keys[idx]);
            confs.push(p as f64);
        }
        prev = idx;
    }
    let score = if confs.is_empty() {
        0.0
    } else {
        confs.iter().sum::<f64>() / confs.len() as f64
    };
    (text, score)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repetition_detector() {
        let mut ids: Vec<i64> = (0..40).collect();
        assert_eq!(repeated_suffix_start(&ids), None);
        ids.extend(std::iter::repeat_n([7, 8], 16).flatten());
        assert_eq!(ids.len(), 72);
        assert_eq!(repeated_suffix_start(&ids), Some(42));
        let short: Vec<i64> = vec![1; 30];
        assert_eq!(repeated_suffix_start(&short), None);
    }

    #[test]
    fn softmax_of_argmax() {
        let (i, p) = argmax_softmax(&[0.0, 0.0, 10.0]);
        assert_eq!(i, 2);
        assert!(p > 0.99);
    }

    #[test]
    fn nms_suppresses_overlaps() {
        let b = |x: f64, s: f64| FormulaBox {
            xyxy: [x, 0.0, x + 10.0, 10.0],
            score: s,
            label: "embedding",
        };
        let kept = nms(vec![b(0.0, 0.5), b(1.0, 0.9), b(50.0, 0.3)], 0.45);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].score, 0.9);
    }

    #[test]
    fn ctc_collapses_repeats_and_blanks() {
        let keys: Vec<String> = ["blank", "a", "b", " "]
            .iter()
            .map(|s| s.to_string())
            .collect();
        // steps: a a blank a b
        let one_hot = |i: usize| {
            (0..4)
                .map(|j| if j == i { 0.9 } else { 0.03 })
                .collect::<Vec<f32>>()
        };
        let probs: Vec<f32> = [1, 1, 0, 1, 2].iter().flat_map(|&i| one_hot(i)).collect();
        let (text, score) = ctc_decode(&probs, 4, &keys);
        assert_eq!(text, "aab");
        assert!((score - 0.9).abs() < 1e-6);
        assert_eq!(ctc_decode(&one_hot(0), 4, &keys), (String::new(), 0.0));
    }

    #[test]
    fn db_postprocess_finds_a_text_line() {
        let (w, h) = (64, 32);
        let mut pred = vec![0f32; w * h];
        for y in 10..20 {
            for x in 8..56 {
                pred[y * w + x] = 0.9;
            }
        }
        let boxes = db_postprocess(&pred, w, h, w * 2, h * 2);
        assert_eq!(boxes.len(), 1);
        let b = boxes[0];
        assert!(
            b[0] < 16.0 && b[2] > 110.0 && b[1] < 20.0 && b[3] > 38.0,
            "{b:?}"
        );
    }

    #[test]
    fn db_postprocess_ignores_noise() {
        let (w, h) = (32, 32);
        let mut pred = vec![0f32; w * h];
        pred[5 * w + 5] = 0.95;
        assert!(db_postprocess(&pred, w, h, w, h).is_empty());
    }
}
