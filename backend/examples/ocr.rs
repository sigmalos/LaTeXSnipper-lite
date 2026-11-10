//! `cargo run --release --example ocr -- <models_dir> <formula|mixed|text> <image>...`
use latexsnipper_lib::ocr::{imageops, Engine, Mode};
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let engine = Engine::new(std::path::Path::new(&args[0]));
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
