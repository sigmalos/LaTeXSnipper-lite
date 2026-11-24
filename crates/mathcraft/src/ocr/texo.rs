//! Texo (distilled PP-FormulaNet-S) formula recognition from a GGUF file.
//!
//! The GGUF holds BatchNorm-folded HGNetV2 encoder weights, an MBart decoder
//! and the vocabulary. Both halves are rebuilt here as ONNX graphs, so they
//! run on the same ONNX Runtime sessions (GPU first, CPU fallback) and the
//! same greedy decoding loop as the MathCraft recognizer:
//!
//! * encoder: `pixel_values [N, 3, S, S]` → `hidden [N, (S/32)², d_model]`
//!   (the `enc_to_dec_proj` projection is folded in)
//! * decoder: `input_ids [N, L]`, `encoder_hidden_states [N, T, d_model]` →
//!   next-token logits `[N, vocab]` for the last position only

use super::gguf::Gguf;
use super::graph::{Attr, Builder, Io};
use super::tokenizer::Tokenizer;
use anyhow::{anyhow, bail, ensure, Context, Result};
use image::{imageops::FilterType, GrayImage, Luma, RgbImage};
use onnx_rs::ast::DataType;
use std::path::{Path, PathBuf};

/// `general.architecture` of a supported file.
pub const ARCHITECTURE: &str = "ppformulanet";
/// UniMERNet image normalization the model was trained with.
const MEAN: f32 = 0.7931;
const STD: f32 = 0.1738;
const LN_EPS: f32 = 1e-5;
/// MBart's learned positions start at index 2.
const POSITION_OFFSET: i64 = 2;

/// The `.gguf` file inside a model directory.
pub fn find_gguf(dir: &Path) -> Result<PathBuf> {
    let mut hits: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("missing model directory {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("gguf"))
        })
        .collect();
    hits.sort();
    hits.into_iter()
        .next()
        .ok_or_else(|| anyhow!("no *.gguf under {}", dir.display()))
}

/// Model metadata the recognizer needs outside the graphs.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub image_size: u32,
    pub start_id: i64,
    pub eos_id: i64,
    pub pad_id: i64,
}

fn check_architecture(g: &Gguf) -> Result<()> {
    match g.str("general.architecture") {
        Some(ARCHITECTURE) => Ok(()),
        Some(other) => bail!("unsupported GGUF formula model architecture {other:?}"),
        None => bail!("GGUF file has no general.architecture"),
    }
}

pub fn config(g: &Gguf) -> Result<Config> {
    check_architecture(g)?;
    let get = |k: &str, default: u64| g.u64(k).unwrap_or(default);
    Ok(Config {
        image_size: get("ppfn.encoder.image_size", 384) as u32,
        start_id: get("ppfn.decoder.decoder_start_token_id", 0) as i64,
        eos_id: get("ppfn.decoder.eos_token_id", 2) as i64,
        pad_id: get("ppfn.decoder.pad_token_id", 1) as i64,
    })
}

/// The byte-level BPE vocabulary embedded in the file.
pub fn tokenizer(g: &Gguf) -> Result<Tokenizer> {
    let tokens = g.strings("tokenizer.tokens")?;
    let mut special: Vec<u32> = tokens
        .iter()
        .enumerate()
        .filter(|(_, t)| matches!(t.as_str(), "<s>" | "<pad>" | "</s>" | "<unk>"))
        .map(|(i, _)| i as u32)
        .collect();
    for key in [
        "ppfn.decoder.bos_token_id",
        "ppfn.decoder.eos_token_id",
        "ppfn.decoder.pad_token_id",
    ] {
        if let Ok(id) = g.u64(key) {
            special.push(id as u32);
        }
    }
    Ok(Tokenizer::from_tokens(tokens, special, true))
}

/// UniMERNet preprocessing: grayscale, trim the blank margin, fit inside
/// `size`×`size` keeping the aspect ratio, center on black, normalize.
/// Returns one CHW image (the gray plane repeated three times).
pub fn preprocess(img: &RgbImage, size: u32) -> Vec<f32> {
    let gray = GrayImage::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y);
        let v = 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32;
        Luma([v.round().clamp(0.0, 255.0) as u8])
    });
    let gray = crop_margin(&gray);
    let (w, h) = gray.dimensions();
    let scale = (size as f32 / w as f32).min(size as f32 / h as f32);
    let nw = ((w as f32 * scale) as u32).clamp(1, size);
    let nh = ((h as f32 * scale) as u32).clamp(1, size);
    let resized = image::imageops::resize(&gray, nw, nh, FilterType::CatmullRom);
    let (left, top) = ((size - nw) / 2, (size - nh) / 2);
    let plane = (size * size) as usize;
    let mut out = vec![(0.0 - MEAN) / STD; plane * 3];
    for (x, y, p) in resized.enumerate_pixels() {
        let v = (p[0] as f32 / 255.0 - MEAN) / STD;
        let i = ((y + top) * size + x + left) as usize;
        for c in 0..3 {
            out[c * plane + i] = v;
        }
    }
    out
}

/// UniMERNet `crop_margin`: keep the bounding box of the pixels darker than
/// 200 on a contrast-stretched copy. Blank or flat images are kept whole.
fn crop_margin(gray: &GrayImage) -> GrayImage {
    let (lo, hi) = gray
        .pixels()
        .fold((255u8, 0u8), |(lo, hi), p| (lo.min(p[0]), hi.max(p[0])));
    if hi <= lo {
        return gray.clone();
    }
    let threshold = lo as f32 + 200.0 / 255.0 * (hi - lo) as f32;
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (x, y, p) in gray.enumerate_pixels() {
        if (p[0] as f32) < threshold {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if x0 > x1 {
        return gray.clone();
    }
    image::imageops::crop_imm(gray, x0, y0, x1 - x0 + 1, y1 - y0 + 1).to_image()
}

// ------------------------------------------------------------------ graphs

struct Net<'g> {
    g: &'g Gguf,
    b: Builder,
}

impl<'g> Net<'g> {
    fn tensor(&mut self, name: &str) -> Result<(Vec<i64>, Vec<f32>)> {
        let info = self.g.info(name)?;
        Ok((info.shape(), self.g.f32(name)?))
    }

    fn weight(&mut self, name: &str) -> Result<String> {
        let (shape, data) = self.tensor(name)?;
        Ok(self.b.f32(shape, data))
    }

    /// A conv with BatchNorm folded in. Weights are stored as
    /// `[out, in/groups · k · k]`.
    fn conv(
        &mut self,
        x: &str,
        name: &str,
        k: i64,
        stride: i64,
        groups: i64,
        relu: bool,
    ) -> Result<String> {
        self.conv_padded(x, name, k, stride, groups, relu, [(k - 1) / 2; 4])
    }

    #[allow(clippy::too_many_arguments)]
    fn conv_padded(
        &mut self,
        x: &str,
        name: &str,
        k: i64,
        stride: i64,
        groups: i64,
        relu: bool,
        pads: [i64; 4],
    ) -> Result<String> {
        let (shape, data) = self.tensor(&format!("{name}.weight"))?;
        let [out, flat] = shape[..] else {
            bail!("{name}.weight is not a flattened conv kernel: {shape:?}");
        };
        ensure!(
            flat % (k * k) == 0,
            "{name}.weight does not hold {k}×{k} kernels"
        );
        let w = self.b.f32(vec![out, flat / (k * k), k, k], data);
        let bias = self.weight(&format!("{name}.bias"))?;
        let y = self.b.op(
            "Conv",
            &[x, &w, &bias],
            vec![
                Attr::Ints("kernel_shape", vec![k, k]),
                Attr::Ints("strides", vec![stride, stride]),
                Attr::Ints("pads", pads.to_vec()),
                Attr::Int("group", groups),
            ],
        );
        Ok(if relu {
            self.b.op("Relu", &[&y], vec![])
        } else {
            y
        })
    }

    /// `x @ W^T + b` for a PyTorch `[out, in]` weight; bias optional.
    fn linear(&mut self, x: &str, name: &str) -> Result<String> {
        let (shape, data) = self.tensor(&format!("{name}.weight"))?;
        let [out, inp] = shape[..] else {
            bail!("{name}.weight is not a matrix: {shape:?}");
        };
        let (out, inp) = (out as usize, inp as usize);
        let mut t = vec![0f32; data.len()];
        for o in 0..out {
            for i in 0..inp {
                t[i * out + o] = data[o * inp + i];
            }
        }
        let w = self.b.f32(vec![inp as i64, out as i64], t);
        let y = self.b.op("MatMul", &[x, &w], vec![]);
        let bias = format!("{name}.bias");
        if self.g.has(&bias) {
            let b = self.weight(&bias)?;
            Ok(self.b.op("Add", &[&y, &b], vec![]))
        } else {
            Ok(y)
        }
    }

    fn layer_norm(&mut self, x: &str, name: &str) -> Result<String> {
        let w = self.weight(&format!("{name}.weight"))?;
        let b = self.weight(&format!("{name}.bias"))?;
        Ok(self.b.op(
            "LayerNormalization",
            &[x, &w, &b],
            vec![Attr::Int("axis", -1), Attr::Float("epsilon", LN_EPS)],
        ))
    }

    /// Exact GELU: `0.5 · x · (1 + erf(x / √2))`.
    fn gelu(&mut self, x: &str) -> String {
        let sqrt2 = self.b.scalar(std::f32::consts::SQRT_2);
        let one = self.b.scalar(1.0);
        let half = self.b.scalar(0.5);
        let d = self.b.op("Div", &[x, &sqrt2], vec![]);
        let e = self.b.op("Erf", &[&d], vec![]);
        let e = self.b.op("Add", &[&e, &one], vec![]);
        let y = self.b.op("Mul", &[x, &e], vec![]);
        self.b.op("Mul", &[&y, &half], vec![])
    }

    fn u(&self, key: &str) -> Result<i64> {
        Ok(self.g.u64(key)? as i64)
    }

    // ------------------------------------------------------------- encoder

    fn encoder(mut self) -> Result<Vec<u8>> {
        let x = "pixel_values";
        // Stem.
        let x = self.conv(x, "enc.stem.stem1", 3, 2, 1, true)?;
        let pad = self.b.i64(vec![8], vec![0, 0, 0, 0, 0, 0, 1, 1]);
        let x = self.b.op("Pad", &[&x, &pad], vec![]);
        let x2 = self.conv_padded(&x, "enc.stem.stem2a", 2, 1, 1, true, [0, 0, 1, 1])?;
        let x2 = self.conv_padded(&x2, "enc.stem.stem2b", 2, 1, 1, true, [0, 0, 0, 0])?;
        let x1 = self.b.op(
            "MaxPool",
            &[&x],
            vec![
                Attr::Ints("kernel_shape", vec![2, 2]),
                Attr::Ints("strides", vec![1, 1]),
                Attr::Int("ceil_mode", 1),
            ],
        );
        let x = self.b.op("Concat", &[&x1, &x2], vec![Attr::Int("axis", 1)]);
        let x = self.conv(&x, "enc.stem.stem3", 3, 2, 1, true)?;
        let mut x = self.conv(&x, "enc.stem.stem4", 1, 1, 1, true)?;
        let mut channels = self.g.info("enc.stem.stem4.bias")?.len() as i64;

        // HGNetV2 stages of HG blocks.
        let n_stages = self.u("ppfn.encoder.n_stages").unwrap_or(4);
        for si in 0..n_stages {
            let key = |k: &str| format!("ppfn.encoder.stage{si}.{k}");
            let mid = self.u(&key("mid_channels"))?;
            let out = self.u(&key("out_channels"))?;
            let n_blocks = self.u(&key("n_blocks"))?;
            let n_layers = self.u(&key("n_layers"))?;
            let k = self.u(&key("kernel_size"))?;
            let light = self.u(&key("light_block"))? != 0;
            let down = format!("enc.stage{si}.downsample");
            if self.g.has(&format!("{down}.weight")) {
                x = self.conv(&x, &down, 3, 2, channels, false)?;
            }
            for bi in 0..n_blocks {
                let identity = x.clone();
                let mut parts = vec![x.clone()];
                for li in 0..n_layers {
                    let layer = format!("enc.stage{si}.block{bi}.layer{li}");
                    x = if light {
                        let y = self.conv(&x, &format!("{layer}.conv1"), 1, 1, 1, false)?;
                        self.conv(&y, &format!("{layer}.conv2"), k, 1, mid, true)?
                    } else {
                        self.conv(&x, &layer, k, 1, 1, true)?
                    };
                    parts.push(x.clone());
                }
                let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
                let cat = self.b.op("Concat", &refs, vec![Attr::Int("axis", 1)]);
                let block = format!("enc.stage{si}.block{bi}");
                let y = self.conv(&cat, &format!("{block}.agg_squeeze"), 1, 1, 1, true)?;
                x = self.conv(&y, &format!("{block}.agg_excite"), 1, 1, 1, true)?;
                if bi > 0 {
                    x = self.b.op("Add", &[&x, &identity], vec![]);
                }
            }
            channels = out;
        }

        // [N, C, H, W] → [N, H·W, C] → projection to the decoder width.
        let shape = self.b.i64(vec![3], vec![0, channels, -1]);
        let flat = self.b.op("Reshape", &[&x, &shape], vec![]);
        let seq = self.b.op(
            "Transpose",
            &[&flat],
            vec![Attr::Ints("perm", vec![0, 2, 1])],
        );
        let hidden = self.linear(&seq, "enc.proj")?;
        let d_model = self.u("ppfn.decoder.d_model")?;
        let size = self.u("ppfn.encoder.image_size").unwrap_or(384);
        self.b.op_into("hidden", "Identity", &[&hidden], vec![]);
        Ok(self.b.encode(
            &[Io {
                name: "pixel_values",
                elem: DataType::Float,
                dims: vec![None, Some(3), Some(size), Some(size)],
            }],
            &[Io {
                name: "hidden",
                elem: DataType::Float,
                dims: vec![None, None, Some(d_model)],
            }],
        ))
    }

    // ------------------------------------------------------------- decoder

    /// Multi-head attention of `q_in` over `kv_in`, optionally masked.
    fn attention(
        &mut self,
        q_in: &str,
        kv_in: &str,
        name: &str,
        mask: Option<&str>,
    ) -> Result<String> {
        let d = self.u("ppfn.decoder.d_model")?;
        let heads = self.u("ppfn.decoder.decoder_attention_heads")?;
        ensure!(
            d % heads == 0,
            "d_model {d} is not divisible by {heads} heads"
        );
        let hd = d / heads;
        let split = self.b.i64(vec![4], vec![0, 0, heads, hd]);
        let merge = self.b.i64(vec![3], vec![0, 0, d]);
        let scale = self.b.scalar((hd as f32).powf(-0.5));
        let q = self.linear(q_in, &format!("{name}.q"))?;
        let q = self.b.op("Mul", &[&q, &scale], vec![]);
        let q = self.b.op("Reshape", &[&q, &split], vec![]);
        let q = self.b.op(
            "Transpose",
            &[&q],
            vec![Attr::Ints("perm", vec![0, 2, 1, 3])],
        );
        let k = self.linear(kv_in, &format!("{name}.k"))?;
        let k = self.b.op("Reshape", &[&k, &split], vec![]);
        let k = self.b.op(
            "Transpose",
            &[&k],
            vec![Attr::Ints("perm", vec![0, 2, 3, 1])],
        );
        let v = self.linear(kv_in, &format!("{name}.v"))?;
        let v = self.b.op("Reshape", &[&v, &split], vec![]);
        let v = self.b.op(
            "Transpose",
            &[&v],
            vec![Attr::Ints("perm", vec![0, 2, 1, 3])],
        );
        let mut scores = self.b.op("MatMul", &[&q, &k], vec![]);
        if let Some(mask) = mask {
            scores = self.b.op("Add", &[&scores, mask], vec![]);
        }
        let probs = self
            .b
            .op("Softmax", &[&scores], vec![Attr::Int("axis", -1)]);
        let o = self.b.op("MatMul", &[&probs, &v], vec![]);
        let o = self.b.op(
            "Transpose",
            &[&o],
            vec![Attr::Ints("perm", vec![0, 2, 1, 3])],
        );
        let o = self.b.op("Reshape", &[&o, &merge], vec![]);
        self.linear(&o, &format!("{name}.out"))
    }

    fn decoder(mut self) -> Result<Vec<u8>> {
        let d = self.u("ppfn.decoder.d_model")?;
        let n_layers = self.u("ppfn.decoder.decoder_layers")?;
        let vocab = self.g.info("dec.lm_head.weight")?.shape()[0];
        let ids = "input_ids";
        let enc = "encoder_hidden_states";

        // Scaled token embeddings plus learned positions.
        let table = self.weight("dec.embed_tokens.weight")?;
        let tok = self
            .b
            .op("Gather", &[&table, ids], vec![Attr::Int("axis", 0)]);
        let scale = self.b.scalar((d as f32).sqrt());
        let tok = self.b.op("Mul", &[&tok, &scale], vec![]);
        let len = self.b.op(
            "Shape",
            &[ids],
            vec![Attr::Int("start", 1), Attr::Int("end", 2)],
        );
        let offset = self.b.i64(vec![1], vec![POSITION_OFFSET]);
        let end = self.b.op("Add", &[&len, &offset], vec![]);
        let zero = self.b.i64(vec![1], vec![0]);
        let positions = self.weight("dec.embed_positions.weight")?;
        let pos = self
            .b
            .op("Slice", &[&positions, &offset, &end, &zero], vec![]);
        let h = self.b.op("Add", &[&tok, &pos], vec![]);
        let mut h = self.layer_norm(&h, "dec.embed_ln")?;

        // Causal mask: a large negative value above the diagonal.
        let square = self
            .b
            .op("Concat", &[&len, &len], vec![Attr::Int("axis", 0)]);
        let full = self.b.op(
            "ConstantOfShape",
            &[&square],
            vec![Attr::Scalar("value", -1e9)],
        );
        let one = self.b.i64(vec![], vec![1]);
        let mask = self
            .b
            .op("Trilu", &[&full, &one], vec![Attr::Int("upper", 1)]);

        // Pre-LN MBart layers.
        for li in 0..n_layers {
            let p = format!("dec.layers.{li}");
            let a = self.layer_norm(&h, &format!("{p}.self_attn_ln"))?;
            let a = self.attention(&a, &a, &format!("{p}.self_attn"), Some(&mask))?;
            h = self.b.op("Add", &[&h, &a], vec![]);
            let a = self.layer_norm(&h, &format!("{p}.cross_attn_ln"))?;
            let a = self.attention(&a, enc, &format!("{p}.cross_attn"), None)?;
            h = self.b.op("Add", &[&h, &a], vec![]);
            let a = self.layer_norm(&h, &format!("{p}.ffn_ln"))?;
            let a = self.linear(&a, &format!("{p}.ffn.up"))?;
            let a = self.gelu(&a);
            let a = self.linear(&a, &format!("{p}.ffn.down"))?;
            h = self.b.op("Add", &[&h, &a], vec![]);
        }

        // Only the last position feeds the vocabulary projection.
        let last = self.b.i64(vec![], vec![-1]);
        let h = self
            .b
            .op("Gather", &[&h, &last], vec![Attr::Int("axis", 1)]);
        let h = self.layer_norm(&h, "dec.final_ln")?;
        let logits = self.linear(&h, "dec.lm_head")?;
        self.b.op_into("logits", "Identity", &[&logits], vec![]);
        Ok(self.b.encode(
            &[
                Io {
                    name: "input_ids",
                    elem: DataType::Int64,
                    dims: vec![None, None],
                },
                Io {
                    name: "encoder_hidden_states",
                    elem: DataType::Float,
                    dims: vec![None, None, Some(d)],
                },
            ],
            &[Io {
                name: "logits",
                elem: DataType::Float,
                dims: vec![None, Some(vocab)],
            }],
        ))
    }
}

/// ONNX bytes of the encoder (with the decoder projection).
pub fn encoder_graph(g: &Gguf) -> Result<Vec<u8>> {
    check_architecture(g)?;
    Net {
        g,
        b: Builder::default(),
    }
    .encoder()
}

/// ONNX bytes of the next-token decoder.
pub fn decoder_graph(g: &Gguf) -> Result<Vec<u8>> {
    check_architecture(g)?;
    Net {
        g,
        b: Builder::default(),
    }
    .decoder()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preprocessing_trims_fits_and_centers() {
        // A 40×10 black outline (white inside) on a 100×100 white page.
        let mut img = RgbImage::from_pixel(100, 100, image::Rgb([255, 255, 255]));
        for y in 45..55 {
            for x in 30..70 {
                if y == 45 || y == 54 || x == 30 || x == 69 {
                    img.put_pixel(x, y, image::Rgb([0, 0, 0]));
                }
            }
        }
        let size = 64;
        let px = preprocess(&img, size);
        assert_eq!(px.len(), 3 * 64 * 64);
        let (black, white) = ((0.0 - MEAN) / STD, (1.0 - MEAN) / STD);
        let at = |x: u32, y: u32| px[(y * size + x) as usize];
        // Margins trimmed: the box fills the width, 16 rows tall from row 24.
        assert!((at(32, 32) - white).abs() < 0.05, "inside the box");
        assert!((at(32, 23) - black).abs() < 1e-5, "padding above");
        assert!((at(32, 40) - black).abs() < 1e-5, "padding below");
        assert!(
            at(32, 28) > 0.0 && at(32, 36) > 0.0,
            "rows 24..40 hold the box"
        );
        let plane = (size * size) as usize;
        assert_eq!(
            px[..plane],
            px[plane..2 * plane],
            "gray repeated per channel"
        );
    }

    #[test]
    fn blank_images_are_kept_whole() {
        let white = GrayImage::from_pixel(10, 4, Luma([255]));
        assert_eq!(crop_margin(&white).dimensions(), (10, 4));
    }
}
