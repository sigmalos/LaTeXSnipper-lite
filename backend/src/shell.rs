//! Tray integration, background running, and the macOS capture hotkey.

use crate::commands::RecognizeResult;
use crate::store::Settings;
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{
    AppHandle, Emitter, Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

pub const TRAY_ID: &str = "latexsnipper";
pub const MAIN: &str = "main";
/// Small window that reviews a capture taken while the main window was hidden.
pub const RESULT: &str = "result";
pub const CAPTURE_SUPPORTED: bool = cfg!(target_os = "macos");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    Open,
    Capture,
    Settings,
    Quit,
}

impl TrayAction {
    pub const ALL: [TrayAction; 4] = [Self::Open, Self::Capture, Self::Settings, Self::Quit];

    pub fn id(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Capture => "capture",
            Self::Settings => "settings",
            Self::Quit => "quit",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.id() == id)
    }

    pub fn label(self, lang: &str) -> &'static str {
        let en = lang == "en";
        match self {
            Self::Open if en => "Open LaTeXSnipper",
            Self::Open => "打开主窗口",
            Self::Capture if en => "Capture",
            Self::Capture => "截图识别",
            Self::Settings if en => "Settings…",
            Self::Settings => "设置…",
            Self::Quit if en => "Quit LaTeXSnipper",
            Self::Quit => "退出 LaTeXSnipper",
        }
    }
}

/// Accelerator in macOS notation, e.g. `CommandOrControl+Alt+F` → `⌥⌘F`.
pub fn display_hotkey(accelerator: &str) -> String {
    let mut parts: Vec<&str> = accelerator.split('+').collect();
    let key = parts.pop().unwrap_or_default();
    let has = |names: &[&str]| parts.iter().any(|p| names.contains(p));
    let mut out = String::new();
    for (names, symbol) in [
        (&["Control", "Ctrl"][..], "⌃"),
        (&["Alt", "Option"][..], "⌥"),
        (&["Shift"][..], "⇧"),
        (
            &["CommandOrControl", "CmdOrCtrl", "Command", "Cmd", "Super"][..],
            "⌘",
        ),
    ] {
        if has(names) {
            out.push_str(symbol);
        }
    }
    out + key
}

pub fn tooltip(s: &Settings) -> String {
    if !CAPTURE_SUPPORTED {
        return "LaTeXSnipper Lite".into();
    }
    format!(
        "LaTeXSnipper · {} {}",
        TrayAction::Capture.label(&s.language),
        display_hotkey(&s.hotkey)
    )
}

pub fn register_hotkey<R: Runtime>(app: &AppHandle<R>, accelerator: &str) -> anyhow::Result<()> {
    if !CAPTURE_SUPPORTED {
        return Ok(());
    }
    let shortcut: Shortcut = accelerator.parse().map_err(|e| anyhow::anyhow!("{e}"))?;
    let gs = app.global_shortcut();
    gs.unregister_all()?;
    gs.register(shortcut)?;
    Ok(())
}

/// Release the global hotkey so the recorder can receive the key itself.
pub fn suspend_hotkey<R: Runtime>(app: &AppHandle<R>) -> anyhow::Result<()> {
    app.global_shortcut().unregister_all()?;
    Ok(())
}

pub fn show_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// A background capture's result, waiting for the result window to take it.
#[derive(Default)]
pub struct PendingResult(pub Mutex<Option<RecognizeResult>>);

/// Review a capture in the result window only, leaving the main window hidden.
/// The page takes the result, renders it, then shows itself (`present_result`).
pub fn open_result<R: Runtime>(app: &AppHandle<R>, result: RecognizeResult) -> tauri::Result<()> {
    *app.state::<PendingResult>()
        .0
        .lock()
        .unwrap_or_else(|p| p.into_inner()) = Some(result);
    if app.get_webview_window(RESULT).is_some() {
        return app.emit_to(RESULT, "result-ready", ());
    }
    let builder = WebviewWindowBuilder::new(app, RESULT, WebviewUrl::App("result.html".into()))
        .title("LaTeXSnipper")
        .inner_size(560.0, 460.0)
        .min_inner_size(420.0, 380.0)
        .center()
        .always_on_top(true)
        .visible(false);
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    builder.build().map(drop)
}

/// The result window, if it is on screen.
pub fn visible_result<R: Runtime>(app: &AppHandle<R>) -> Option<WebviewWindow<R>> {
    app.get_webview_window(RESULT)
        .filter(|w| w.is_visible().unwrap_or(false))
}

pub fn present_result<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window(RESULT) {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Hide the result window. With the main window also hidden, hand focus back
/// to the app the capture came from so the copied LaTeX can be pasted there.
pub fn dismiss_result<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window(RESULT) {
        let _ = w.hide();
    }
    #[cfg(target_os = "macos")]
    {
        let main_visible = app
            .get_webview_window(MAIN)
            .is_some_and(|w| w.is_visible().unwrap_or(false));
        if !main_visible {
            let _ = app.hide();
        }
    }
}

fn menu<R: Runtime>(app: &AppHandle<R>, s: &Settings) -> tauri::Result<Menu<R>> {
    let lang = s.language.as_str();
    let item = |a: TrayAction, accel: Option<&str>| {
        // Accelerators here are display-only hints; fall back to none if unparsable.
        MenuItem::with_id(app, a.id(), a.label(lang), true, accel)
            .or_else(|_| MenuItem::with_id(app, a.id(), a.label(lang), true, None::<&str>))
    };
    let menu = Menu::with_items(
        app,
        &[
            &item(TrayAction::Open, None)?,
            &PredefinedMenuItem::separator(app)?,
            &item(TrayAction::Settings, Some("CommandOrControl+,"))?,
            &PredefinedMenuItem::separator(app)?,
            &item(TrayAction::Quit, Some("CommandOrControl+Q"))?,
        ],
    )?;
    if CAPTURE_SUPPORTED {
        menu.insert(&item(TrayAction::Capture, Some(&s.hotkey))?, 0)?;
    }
    Ok(menu)
}

pub fn handle<R: Runtime>(app: &AppHandle<R>, action: TrayAction) {
    match action {
        TrayAction::Open => show_main(app),
        // Same path as the global hotkey; the webview stays alive while hidden.
        TrayAction::Capture => drop(app.emit("hotkey-capture", ())),
        TrayAction::Settings => {
            show_main(app);
            let _ = app.emit("open-settings", ());
        }
        TrayAction::Quit => app.exit(0),
    }
}

pub fn install_tray<R: Runtime>(app: &AppHandle<R>, s: &Settings) -> tauri::Result<()> {
    // macOS tints a black template glyph to match the menu bar; Windows and Linux
    // panels can be dark or light, so they get the full-colour app icon.
    #[cfg(target_os = "macos")]
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    #[cfg(not(target_os = "macos"))]
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip(tooltip(s))
        .menu(&menu(app, s)?)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            if let Some(action) = TrayAction::from_id(event.id().as_ref()) {
                handle(app, action);
            }
        })
        .build(app)?;
    Ok(())
}

/// Rebuild labels and the hotkey hint after settings change.
pub fn refresh_tray<R: Runtime>(app: &AppHandle<R>, s: &Settings) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Ok(m) = menu(app, s) {
            let _ = tray.set_menu(Some(m));
        }
        let _ = tray.set_tooltip(Some(tooltip(s)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_ids_roundtrip() {
        for a in TrayAction::ALL {
            assert_eq!(TrayAction::from_id(a.id()), Some(a));
        }
        assert_eq!(TrayAction::from_id("nope"), None);
    }

    #[test]
    fn labels_follow_language() {
        assert_eq!(TrayAction::Quit.label("zh"), "退出 LaTeXSnipper");
        assert_eq!(TrayAction::Quit.label("en"), "Quit LaTeXSnipper");
        for a in TrayAction::ALL {
            assert_ne!(a.label("zh"), a.label("en"));
        }
    }

    #[test]
    fn hotkeys_display_in_mac_order() {
        assert_eq!(display_hotkey("CommandOrControl+F"), "⌘F");
        assert_eq!(display_hotkey("CommandOrControl+Alt+F"), "⌥⌘F");
        assert_eq!(display_hotkey("Shift+Control+K"), "⌃⇧K");
        assert_eq!(display_hotkey("Alt+F5"), "⌥F5");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn tooltip_names_the_hotkey() {
        let s = Settings::default();
        assert_eq!(tooltip(&s), "LaTeXSnipper · 截图识别 ⌘F");
        let en = Settings {
            language: "en".into(),
            ..s
        };
        assert_eq!(tooltip(&en), "LaTeXSnipper · Capture ⌘F");
    }

    #[test]
    fn default_hotkey_parses_as_a_global_shortcut() {
        let s: Result<Shortcut, _> = Settings::default().hotkey.parse();
        assert!(s.is_ok());
    }
}
