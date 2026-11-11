//! `cargo run --release --example download -- <data_dir> <model-id>`
//! Same path as the Settings "下载" button: Core::download_model.
use latexsnipper_lib::commands::Core;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let core = Core::new(std::path::Path::new(&args[0]));
    let mut last = 0;
    let state = core.download_model(&args[1], &mut |p| {
        let pct = if p.total > 0 { p.downloaded * 100 / p.total } else { 0 };
        if pct >= last + 25 || p.downloaded == p.total {
            println!("progress {} {pct}% ({}/{})", p.model, p.downloaded, p.total);
            last = pct;
        }
    })?;
    for m in state.models {
        println!("{:<24} installed={} downloading={}", m.id, m.installed, m.downloading);
    }
    println!("modes {:?}", state.modes);
    Ok(())
}
