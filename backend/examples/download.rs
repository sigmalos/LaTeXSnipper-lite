//! `cargo run --release --example download -- <data_dir> <model-id>`
//! A model Settings offers goes the same path as its "下载" button
//! (Core::download_model). Any other model in the manifest (the FP32 formula
//! models the engine parity test and the Office add-in use) is fetched with the
//! same verified downloader into the same models directory.
use latexsnipper_lib::commands::{is_known_model, Core};
use latexsnipper_lib::modelhub;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (dir, id) = (std::path::Path::new(&args[0]), args[1].as_str());
    let mut last = 0;
    let mut progress = |p: modelhub::Progress| {
        let pct = if p.total > 0 {
            p.downloaded * 100 / p.total
        } else {
            0
        };
        if pct >= last + 25 || p.downloaded == p.total {
            println!("progress {} {pct}% ({}/{})", p.model, p.downloaded, p.total);
            last = pct;
        }
    };
    if !is_known_model(id) {
        modelhub::download(&dir.join("MathCraft/models"), id, &mut progress)?;
        println!("{id:<28} installed (not built-in)");
        return Ok(());
    }
    let state = Core::new(dir).download_model(id, &mut progress)?;
    for m in state.models {
        println!(
            "{:<28} installed={} downloading={}",
            m.id, m.installed, m.downloading
        );
    }
    println!("modes {:?}", state.modes);
    Ok(())
}
