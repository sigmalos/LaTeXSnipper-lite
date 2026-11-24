//! `cargo run --release --example ocr -- <models_dir> <formula|mixed|text> <image>...`
//!
//! Uses the FP32 formula models; set `OCR_REC=int8|texo` (and `OCR_DET=int8`)
//! to try the other builds.
use latexsnipper_lib::ocr::{imageops, Engine, FormulaBuilds, FormulaModel, FormulaRec, Mode};
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let pick = |var: &str| std::env::var(var).unwrap_or_default();
    let builds = FormulaBuilds {
        det: match pick("OCR_DET").as_str() {
            "int8" => FormulaModel::Int8,
            _ => FormulaModel::Standard,
        },
        rec: match pick("OCR_REC").as_str() {
            "int8" => FormulaRec::Int8,
            "texo" => FormulaRec::Texo,
            _ => FormulaRec::Standard,
        },
    };
    let engine = Engine::with_builds(std::path::Path::new(&args[0]), builds);
    let mode: Mode = serde_json::from_value(serde_json::Value::String(args[1].clone()))?;
    for path in &args[2..] {
        let img = imageops::decode_rgb(&std::fs::read(path)?)?;
        let t = Instant::now();
        let r = engine.recognize(&img, mode)?;
        println!(
            "{path}\t{:.2}s\t{:.3}\t{:?}",
            t.elapsed().as_secs_f64(),
            r.score,
            r.text
        );
    }
    Ok(())
}
