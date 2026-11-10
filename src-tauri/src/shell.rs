//! macOS shell integration: menu bar item, background running and the global
//! capture hotkey. Closing the window only hides it; the app keeps running in
//! the menu bar until the user quits (⌘Q or the menu item).

use crate::store::Settings;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

pub const TRAY_ID: &str = "latexsnipper";
pub const MAIN: &str = "main";

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
    format!(
        "LaTeXSnipper · {} {}",
        TrayAction::Capture.label(&s.language),
        display_hotkey(&s.hotkey)
    )
}

pub fn register_hotkey<R: Runtime>(app: &AppHandle<R>, accelerator: &str) -> anyhow::Result<()> {
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

fn menu<R: Runtime>(app: &AppHandle<R>, s: &Settings) -> tauri::Result<Menu<R>> {
    let lang = s.language.as_str();
    let item = |a: TrayAction, accel: Option<&str>| {
        // Accelerators here are display-only hints; fall back to none if unparsable.
        MenuItem::with_id(app, a.id(), a.label(lang), true, accel)
            .or_else(|_| MenuItem::with_id(app, a.id(), a.label(lang), true, None::<&str>))
    };
    Menu::with_items(
        app,
        &[
            &item(TrayAction::Capture, Some(&s.hotkey))?,
            &item(TrayAction::Open, None)?,
            &PredefinedMenuItem::separator(app)?,
            &item(TrayAction::Settings, Some("CommandOrControl+,"))?,
            &PredefinedMenuItem::separator(app)?,
            &item(TrayAction::Quit, Some("CommandOrControl+Q"))?,
        ],
    )
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
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .icon_as_template(true)
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
