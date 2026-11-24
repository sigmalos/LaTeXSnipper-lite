//! Minimal GGUF reader: metadata, tensor index and dequantization to `f32`.
//!
//! Only what the formula recognizers need: GGUF v2/v3 files whose tensors are
//! stored as F32, F16, BF16 or Q8_0. Tensors are dequantized on request, so a
//! caller pays only for the weights it actually builds into a graph.

use anyhow::{anyhow, bail, ensure, Context, Result};
use std::collections::HashMap;
use std::path::Path;

/// A metadata value. Arrays keep their elements; nested arrays are allowed.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    U64(u64),
    I64(i64),
    F64(f64),
    Bool(bool),
    Str(String),
    Array(Vec<Value>),
}

impl Value {
    pub fn as_u64(&self) -> Option<u64> {
        match *self {
            Value::U64(v) => Some(v),
            Value::I64(v) => u64::try_from(v).ok(),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// GGML tensor storage types this reader can dequantize.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DType {
    F32,
    F16,
    BF16,
    Q8_0,
}

impl DType {
    fn from_code(code: u32) -> Result<Self> {
        Ok(match code {
            0 => DType::F32,
            1 => DType::F16,
            8 => DType::Q8_0,
            30 => DType::BF16,
            other => bail!("unsupported GGUF tensor type {other}"),
        })
    }

    /// Bytes taken by `n` elements.
    fn size_of(self, n: usize) -> Result<usize> {
        Ok(match self {
            DType::F32 => n * 4,
            DType::F16 | DType::BF16 => n * 2,
            DType::Q8_0 => {
                ensure!(
                    n % 32 == 0,
                    "Q8_0 tensor of {n} elements is not whole blocks"
                );
                n / 32 * 34
            }
        })
    }
}

#[derive(Clone, Debug)]
pub struct TensorInfo {
    /// GGML order: `dims[0]` is the innermost (contiguous) dimension.
    pub dims: Vec<u64>,
    pub dtype: DType,
    offset: usize,
}

impl TensorInfo {
    pub fn len(&self) -> usize {
        self.dims.iter().product::<u64>() as usize
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// The shape in row-major (PyTorch / ONNX) order.
    pub fn shape(&self) -> Vec<i64> {
        self.dims.iter().rev().map(|&d| d as i64).collect()
    }
}

pub struct Gguf {
    data: Vec<u8>,
    pub metadata: HashMap<String, Value>,
    pub tensors: HashMap<String, TensorInfo>,
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.data.len())
            .ok_or_else(|| anyhow!("GGUF file is truncated"))?;
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().unwrap())
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.array()?))
    }
    fn len(&mut self) -> Result<usize> {
        let n = self.u64()?;
        // Every element takes at least a byte, so a longer count is corrupt.
        usize::try_from(n)
            .ok()
            .filter(|&n| n <= self.data.len() - self.pos)
            .ok_or_else(|| anyhow!("GGUF length {n} exceeds the file"))
    }
    fn string(&mut self) -> Result<String> {
        let n = self.len()?;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn value(&mut self, kind: u32, depth: usize) -> Result<Value> {
        Ok(match kind {
            0 => Value::U64(self.take(1)?[0] as u64),
            1 => Value::I64(self.take(1)?[0] as i8 as i64),
            2 => Value::U64(u16::from_le_bytes(self.array()?) as u64),
            3 => Value::I64(i16::from_le_bytes(self.array()?) as i64),
            4 => Value::U64(self.u32()? as u64),
            5 => Value::I64(i32::from_le_bytes(self.array()?) as i64),
            6 => Value::F64(f32::from_le_bytes(self.array()?) as f64),
            7 => Value::Bool(self.take(1)?[0] != 0),
            8 => Value::Str(self.string()?),
            9 => {
                ensure!(depth < 4, "GGUF arrays nest too deeply");
                let inner = self.u32()?;
                let n = self.len()?;
                let mut items = Vec::with_capacity(n.min(1 << 16));
                for _ in 0..n {
                    items.push(self.value(inner, depth + 1)?);
                }
                Value::Array(items)
            }
            10 => Value::U64(self.u64()?),
            11 => Value::I64(i64::from_le_bytes(self.array()?)),
            12 => Value::F64(f64::from_le_bytes(self.array()?)),
            other => bail!("unknown GGUF metadata type {other}"),
        })
    }
}

impl Gguf {
    pub fn open(path: &Path) -> Result<Self> {
        let data =
            std::fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
        Self::parse(data).with_context(|| format!("invalid GGUF file {}", path.display()))
    }

    pub fn parse(data: Vec<u8>) -> Result<Self> {
        let mut r = Reader {
            data: &data,
            pos: 0,
        };
        ensure!(r.take(4)? == b"GGUF", "not a GGUF file");
        let version = r.u32()?;
        ensure!(
            (2..=3).contains(&version),
            "unsupported GGUF version {version}"
        );
        let n_tensors = r.len()?;
        let n_kv = r.len()?;
        let mut metadata = HashMap::new();
        for _ in 0..n_kv {
            let key = r.string()?;
            let kind = r.u32()?;
            metadata.insert(key, r.value(kind, 0)?);
        }
        let mut index = Vec::with_capacity(n_tensors);
        for _ in 0..n_tensors {
            let name = r.string()?;
            let n_dims = r.u32()? as usize;
            ensure!(n_dims <= 4, "tensor {name} has {n_dims} dimensions");
            let dims = (0..n_dims).map(|_| r.u64()).collect::<Result<Vec<_>>>()?;
            let dtype = DType::from_code(r.u32()?).with_context(|| format!("tensor {name}"))?;
            let offset = r.u64()?;
            index.push((name, dims, dtype, offset));
        }
        let alignment = metadata
            .get("general.alignment")
            .and_then(Value::as_u64)
            .unwrap_or(32)
            .max(1) as usize;
        let base = r.pos.div_ceil(alignment) * alignment;
        let mut tensors = HashMap::new();
        for (name, dims, dtype, offset) in index {
            let info = TensorInfo {
                dims,
                dtype,
                offset: base + offset as usize,
            };
            let end = dtype.size_of(info.len())? + info.offset;
            ensure!(
                end <= data.len(),
                "tensor {name} lies past the end of the file"
            );
            tensors.insert(name, info);
        }
        Ok(Self {
            data,
            metadata,
            tensors,
        })
    }

    pub fn u64(&self, key: &str) -> Result<u64> {
        self.metadata
            .get(key)
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("GGUF metadata {key} is missing"))
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        self.metadata.get(key).and_then(Value::as_str)
    }

    pub fn strings(&self, key: &str) -> Result<Vec<String>> {
        match self.metadata.get(key) {
            Some(Value::Array(items)) => items
                .iter()
                .map(|v| v.as_str().map(str::to_string))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| anyhow!("GGUF metadata {key} is not a list of strings")),
            _ => bail!("GGUF metadata {key} is missing"),
        }
    }

    pub fn info(&self, name: &str) -> Result<&TensorInfo> {
        self.tensors
            .get(name)
            .ok_or_else(|| anyhow!("GGUF tensor {name} is missing"))
    }

    pub fn has(&self, name: &str) -> bool {
        self.tensors.contains_key(name)
    }

    /// The tensor's elements as `f32`, in row-major order.
    pub fn f32(&self, name: &str) -> Result<Vec<f32>> {
        let info = self.info(name)?;
        let n = info.len();
        let bytes = &self.data[info.offset..info.offset + info.dtype.size_of(n)?];
        Ok(match info.dtype {
            DType::F32 => bytes
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                .collect(),
            DType::F16 => bytes
                .chunks_exact(2)
                .map(|b| f16_to_f32(u16::from_le_bytes([b[0], b[1]])))
                .collect(),
            DType::BF16 => bytes
                .chunks_exact(2)
                .map(|b| f32::from_bits((u16::from_le_bytes([b[0], b[1]]) as u32) << 16))
                .collect(),
            DType::Q8_0 => {
                let mut out = Vec::with_capacity(n);
                for block in bytes.chunks_exact(34) {
                    let scale = f16_to_f32(u16::from_le_bytes([block[0], block[1]]));
                    out.extend(block[2..].iter().map(|&q| q as i8 as f32 * scale));
                }
                out
            }
        })
    }
}

/// IEEE 754 half precision to single precision, subnormals included.
pub fn f16_to_f32(h: u16) -> f32 {
    let sign = ((h >> 15) as u32) << 31;
    let exp = ((h >> 10) & 0x1f) as u32;
    let frac = (h & 0x3ff) as u32;
    let bits = match (exp, frac) {
        (0, 0) => sign,
        (0, _) => {
            // Subnormal: renormalize the fraction.
            let shift = frac.leading_zeros() - 21;
            let frac = (frac << shift) & 0x3ff;
            sign | ((113 - shift) << 23) | (frac << 13)
        }
        (0x1f, _) => sign | 0x7f80_0000 | (frac << 13),
        _ => sign | ((exp + 112) << 23) | (frac << 13),
    };
    f32::from_bits(bits)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A GGUF v3 file with the given string metadata and F32 / Q8_0 tensors.
    pub fn build(meta: &[(&str, Value)], tensors: &[(&str, Vec<u64>, DType, Vec<u8>)]) -> Vec<u8> {
        fn string(out: &mut Vec<u8>, s: &str) {
            out.extend((s.len() as u64).to_le_bytes());
            out.extend(s.as_bytes());
        }
        fn value(out: &mut Vec<u8>, v: &Value) {
            match v {
                Value::U64(n) => {
                    out.extend(4u32.to_le_bytes());
                    out.extend((*n as u32).to_le_bytes());
                }
                Value::Str(s) => {
                    out.extend(8u32.to_le_bytes());
                    string(out, s);
                }
                Value::Array(items) => {
                    out.extend(9u32.to_le_bytes());
                    out.extend(8u32.to_le_bytes());
                    out.extend((items.len() as u64).to_le_bytes());
                    for item in items {
                        string(out, item.as_str().unwrap());
                    }
                }
                _ => unimplemented!(),
            }
        }
        let mut out = b"GGUF".to_vec();
        out.extend(3u32.to_le_bytes());
        out.extend((tensors.len() as u64).to_le_bytes());
        out.extend((meta.len() as u64).to_le_bytes());
        for (k, v) in meta {
            string(&mut out, k);
            value(&mut out, v);
        }
        let mut offset = 0u64;
        for (name, dims, dtype, data) in tensors {
            string(&mut out, name);
            out.extend((dims.len() as u32).to_le_bytes());
            for d in dims {
                out.extend(d.to_le_bytes());
            }
            let code: u32 = match dtype {
                DType::F32 => 0,
                DType::F16 => 1,
                DType::Q8_0 => 8,
                DType::BF16 => 30,
            };
            out.extend(code.to_le_bytes());
            out.extend(offset.to_le_bytes());
            offset += (data.len() as u64).div_ceil(32) * 32;
        }
        while out.len() % 32 != 0 {
            out.push(0);
        }
        for (_, _, _, data) in tensors {
            out.extend(data);
            while out.len() % 32 != 0 {
                out.push(0);
            }
        }
        out
    }

    pub fn f32_bytes(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    #[test]
    fn half_precision_conversion() {
        assert_eq!(f16_to_f32(0x3c00), 1.0);
        assert_eq!(f16_to_f32(0xc000), -2.0);
        assert_eq!(f16_to_f32(0x3555), 0.333_251_95);
        assert_eq!(f16_to_f32(0x0001), 5.960_464_5e-8); // smallest subnormal
        assert_eq!(f16_to_f32(0x7c00), f32::INFINITY);
        assert_eq!(f16_to_f32(0x8000).to_bits(), 0x8000_0000);
    }

    #[test]
    fn reads_metadata_and_dequantizes() {
        // One Q8_0 block: scale 0.5, quants -2, -1, 0, 1, ... 29.
        let mut q8 = 0x3800u16.to_le_bytes().to_vec();
        q8.extend((-2i8..30).map(|q| q as u8));
        let file = build(
            &[
                ("general.architecture", Value::Str("ppformulanet".into())),
                ("ppfn.decoder.d_model", Value::U64(384)),
                (
                    "tokenizer.tokens",
                    Value::Array(vec![Value::Str("<s>".into()), Value::Str("Ġx".into())]),
                ),
            ],
            &[
                (
                    "a",
                    vec![3, 2],
                    DType::F32,
                    f32_bytes(&[1., 2., 3., 4., 5., 6.]),
                ),
                ("q", vec![32], DType::Q8_0, q8),
            ],
        );
        let g = Gguf::parse(file).unwrap();
        assert_eq!(g.str("general.architecture"), Some("ppformulanet"));
        assert_eq!(g.u64("ppfn.decoder.d_model").unwrap(), 384);
        assert_eq!(g.strings("tokenizer.tokens").unwrap(), ["<s>", "Ġx"]);
        assert_eq!(g.info("a").unwrap().shape(), [2, 3]);
        assert_eq!(g.f32("a").unwrap(), [1., 2., 3., 4., 5., 6.]);
        let q = g.f32("q").unwrap();
        assert_eq!(q.len(), 32);
        assert_eq!(&q[..4], &[-1.0, -0.5, 0.0, 0.5]);
        assert!(g.f32("missing").is_err());
        assert!(g.u64("missing").is_err());
    }

    #[test]
    fn rejects_corrupt_files() {
        assert!(Gguf::parse(b"GGML".to_vec()).is_err());
        let file = build(
            &[],
            &[("a", vec![4], DType::F32, f32_bytes(&[1., 2., 3., 4.]))],
        );
        assert!(
            Gguf::parse(file[..file.len() - 24].to_vec()).is_err(),
            "truncated tensor data"
        );
        let mut huge = b"GGUF".to_vec();
        huge.extend(3u32.to_le_bytes());
        huge.extend(u64::MAX.to_le_bytes());
        huge.extend(0u64.to_le_bytes());
        assert!(Gguf::parse(huge).is_err(), "absurd tensor count");
    }
}
