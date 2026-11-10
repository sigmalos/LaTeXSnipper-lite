pub mod commands;
pub mod external;
pub mod modelhub;
pub mod ocr;
pub mod shell;
pub mod store;

use commands::Core;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Emitter, RunEvent, WindowEvent};
use tauri_plugin_global_shortcut::ShortcutState;

/// Shared with the legacy Qt app so history, favorites and models carry over.
pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("LATEXSNIPPER_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join("Library/Application Support/LaTeXSnipper")
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
        .on_window_event(|window, event| {
            // Closing the main window (red button, ⌘W) keeps the app running in the menu bar.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == shell::MAIN {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(move |app| {
            let settings = core.settings();
            shell::install_tray(app.handle(), &settings)?;
            if let Err(e) = shell::register_hotkey(app.handle(), &settings.hotkey) {
                eprintln!("hotkey {} unavailable: {e}", settings.hotkey);
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
            commands::suspend_hotkey,
            commands::resume_hotkey,
        ])
        .build(tauri::generate_context!())
        .expect("error while building LaTeXSnipper")
        .run(|app, event| {
            // Clicking the Dock icon (or relaunching the .app) brings the window back.
            if let RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } = event
            {
                shell::show_main(app);
            }
        });
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
