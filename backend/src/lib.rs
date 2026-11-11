pub mod commands;
pub mod external;
pub mod modelhub;
pub mod ocr;
pub mod shell;
pub mod store;

use commands::Core;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Emitter, WindowEvent};
use tauri_plugin_global_shortcut::ShortcutState;

/// `LATEXSNIPPER_HEADLESS=1`: start without a window, Dock icon, menu bar item
/// or global hotkey, let the UI run its self-check, then exit. Used by the DMG
/// smoke test so it never takes over the desktop it runs on.
pub fn headless() -> bool {
    std::env::var("LATEXSNIPPER_HEADLESS").is_ok_and(|v| v == "1")
}

/// Local app data. On macOS, Lite uses the existing LaTeXSnipper directory.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("LATEXSNIPPER_DATA_DIR") {
        return PathBuf::from(dir);
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir())
            .join("LaTeXSnipper Lite")
    }
    #[cfg(not(target_os = "windows"))]
    {
        let home = std::env::var_os("HOME").unwrap_or_else(|| ".".into());
        PathBuf::from(home).join("Library/Application Support/LaTeXSnipper")
    }
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
            // Closing the main window keeps the app running in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == shell::MAIN {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(move |app| {
            if headless() {
                // Keep the hidden window; no Dock icon, no focus change, no hotkey.
                #[cfg(target_os = "macos")]
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                return Ok(());
            }
            let settings = core.settings();
            shell::install_tray(app.handle(), &settings)?;
            if let Err(e) = shell::register_hotkey(app.handle(), &settings.hotkey) {
                eprintln!("hotkey {} unavailable: {e}", settings.hotkey);
            }
            // The window starts hidden (tauri.conf.json) so headless runs never flash it.
            shell::show_main(app.handle());
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
            commands::list_models,
            commands::download_model,
            commands::delete_model,
            commands::frontend_ready,
            commands::test_external,
            commands::export_file,
            commands::history_op,
            commands::favorite_op,
            commands::suspend_hotkey,
            commands::resume_hotkey,
        ])
        .build(tauri::generate_context!())
        .expect("error while building LaTeXSnipper")
        .run(|_app, _event| {
            // Clicking the Dock icon (or relaunching the .app) brings the window back.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } = _event
            {
                shell::show_main(_app);
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
