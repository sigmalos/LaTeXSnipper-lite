pub mod commands;
pub mod external;
pub mod modelhub;
pub mod ocr;
pub mod store;

use commands::Core;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// Shared with the legacy Qt app so history, favorites and models carry over.
pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("LATEXSNIPPER_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join("Library/Application Support/LaTeXSnipper")
}

pub fn register_hotkey(app: &AppHandle, accelerator: &str) -> anyhow::Result<()> {
    let shortcut: Shortcut = accelerator.parse().map_err(|e| anyhow::anyhow!("{e}"))?;
    let gs = app.global_shortcut();
    gs.unregister_all()?;
    gs.register(shortcut)?;
    Ok(())
}

pub fn run() {
    let core = Arc::new(Core::new(&data_dir()));
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        let _ = app.emit("hotkey-capture", ());
                    }
                })
                .build(),
        )
        .manage(core.clone())
        .setup(move |app| {
            let hotkey = core.settings().hotkey;
            if let Err(e) = register_hotkey(app.handle(), &hotkey) {
                eprintln!("hotkey {hotkey} unavailable: {e}");
            }
            // Load the formula model off the UI thread so the first capture is fast.
            let warm = core.clone();
            std::thread::spawn(move || {
                let engine = warm.engine();
                if commands::model_status(engine.root()).get("formula") == Some(&true) {
                    let _ = engine.warmup_formula();
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::save_settings,
            commands::recognize_image,
            commands::recognize_file,
            commands::capture,
            commands::download_models,
            commands::test_external,
            commands::export_file,
            commands::history_op,
            commands::favorite_op,
        ])
        .run(tauri::generate_context!())
        .expect("error while running LaTeXSnipper");
}

/// Recognize one image with the installed models and print the LaTeX.
pub fn selftest(image: Option<&str>) -> i32 {
    let core = Core::new(&data_dir());
    let result = image
        .ok_or_else(|| anyhow::anyhow!("usage: --selftest <image>"))
        .and_then(|p| Ok(std::fs::read(p)?))
        .and_then(|bytes| core.recognize_bytes(&bytes, Some(ocr::Mode::Formula)));
    match result {
        Ok(r) => {
            println!("{}", r.recognition.text);
            0
        }
        Err(e) => {
            eprintln!("selftest failed: {e:#}");
            1
        }
    }
}
