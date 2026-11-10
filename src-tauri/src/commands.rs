//! IPC surface exposed to the webview. Each command is a thin wrapper around
//! `Core`, which holds all logic so it can be tested without a window.

use crate::external;
use crate::modelhub::{self, Progress};
use crate::ocr::{imageops, Engine, Mode, Recognition};
use crate::store::{kind_for, Entry, Settings, Store};
use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager, State};

pub struct Core {
    pub store: Mutex<Store>,
    engine: OnceLock<Arc<Engine>>,
}

#[derive(Serialize, Debug)]
pub struct Snapshot {
    pub version: &'static str,
    pub settings: Settings,
    pub history: Vec<Entry>,
    pub favorites: Vec<Entry>,
    pub models: BTreeMap<&'static str, bool>,
    pub pandoc: bool,
    pub data_dir: String,
}

#[derive(Serialize, Debug)]
pub struct RecognizeResult {
    #[serde(flatten)]
    pub recognition: Recognition,
    pub kind: &'static str,
}

pub fn pandoc_path() -> Option<PathBuf> {
    ["/opt/homebrew/bin/pandoc", "/usr/local/bin/pandoc"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
}

pub fn model_status(root: &Path) -> BTreeMap<&'static str, bool> {
    [
        ("formula", Mode::Formula),
        ("mixed", Mode::Mixed),
        ("text", Mode::Text),
    ]
    .into_iter()
    .map(|(k, m)| {
        (
            k,
            m.required_models()
                .iter()
                .all(|id| modelhub::is_installed(root, id)),
        )
    })
    .collect()
}

impl Core {
    pub fn new(dir: &Path) -> Self {
        Self {
            store: Mutex::new(Store::open(dir)),
            engine: OnceLock::new(),
        }
    }

    fn store(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn engine(&self) -> Arc<Engine> {
        let root = self.store().models_dir();
        self.engine
            .get_or_init(|| Arc::new(Engine::new(&root)))
            .clone()
    }

    pub fn snapshot(&self) -> Snapshot {
        let s = self.store();
        Snapshot {
            version: env!("CARGO_PKG_VERSION"),
            settings: s.settings.clone(),
            history: s.history.clone(),
            favorites: s.favorites.clone(),
            models: model_status(&s.models_dir()),
            pandoc: pandoc_path().is_some(),
            data_dir: s.dir().display().to_string(),
        }
    }

    pub fn settings(&self) -> Settings {
        self.store().settings.clone()
    }

    /// Recognize encoded image bytes with the configured backend.
    pub fn recognize_bytes(&self, bytes: &[u8], mode: Option<Mode>) -> Result<RecognizeResult> {
        let settings = self.settings();
        let mode = mode.unwrap_or(settings.mode);
        let recognition = if settings.backend == "external" {
            let img = imageops::decode_rgb(bytes)?;
            let mut png = Vec::new();
            img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;
            let text = external::recognize(&settings, &png, mode)?;
            Recognition {
                text,
                score: 1.0,
                mode,
                flags: vec![],
            }
        } else {
            let engine = self.engine();
            let missing: Vec<&str> = mode
                .required_models()
                .iter()
                .copied()
                .filter(|id| !modelhub::is_installed(engine.root(), id))
                .collect();
            if !missing.is_empty() {
                bail!("MODELS_MISSING:{}", missing.join(","));
            }
            let img = imageops::decode_rgb(bytes)?;
            engine.recognize(&img, mode)?
        };
        if recognition.text.trim().is_empty() {
            bail!("EMPTY_RESULT");
        }
        Ok(RecognizeResult {
            kind: kind_for(mode),
            recognition,
        })
    }

    pub fn download_models(
        &self,
        mode: Mode,
        on_progress: &mut dyn FnMut(Progress),
    ) -> Result<BTreeMap<&'static str, bool>> {
        let root = self.store().models_dir();
        for id in mode.required_models() {
            if !modelhub::is_installed(&root, id) {
                modelhub::download(&root, id, on_progress)?;
            }
        }
        Ok(model_status(&root))
    }
}

pub fn export(path: &Path, content: &str, format: &str) -> Result<()> {
    match format {
        "tex" | "md" | "txt" | "html" | "mml" | "svg" | "typ" => {
            std::fs::write(path, content).context("failed to write file")
        }
        "docx" | "odt" | "rtf" | "epub" => {
            let pandoc = pandoc_path()
                .ok_or_else(|| anyhow!("Pandoc is not installed (brew install pandoc)"))?;
            let src =
                std::env::temp_dir().join(format!("latexsnipper-export-{}.md", std::process::id()));
            std::fs::write(&src, content)?;
            let out = Command::new(pandoc)
                .arg("-f")
                .arg("markdown")
                .arg(&src)
                .arg("-o")
                .arg(path)
                .output()?;
            let _ = std::fs::remove_file(&src);
            if !out.status.success() {
                bail!(
                    "pandoc failed: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
            Ok(())
        }
        other => bail!("unsupported export format {other}"),
    }
}

/// macOS native interactive region capture. `Ok(None)` when the user presses Esc.
pub fn screencapture() -> Result<Option<Vec<u8>>> {
    let path =
        std::env::temp_dir().join(format!("latexsnipper-capture-{}.png", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let status = Command::new("/usr/sbin/screencapture")
        .args(["-i", "-x", "-t", "png"])
        .arg(&path)
        .status()?;
    if !status.success() || !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path)?;
    let _ = std::fs::remove_file(&path);
    Ok(Some(bytes))
}

// ------------------------------------------------------------------ tauri

type Res<T> = std::result::Result<T, String>;

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
        .map_err(err)
}

#[tauri::command]
pub fn bootstrap(core: State<'_, Arc<Core>>) -> Snapshot {
    core.snapshot()
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    core: State<'_, Arc<Core>>,
    settings: Settings,
) -> Res<Settings> {
    let previous = core.settings();
    core.store().save_settings(settings.clone()).map_err(err)?;
    if previous.hotkey != settings.hotkey {
        if let Err(e) = crate::shell::register_hotkey(&app, &settings.hotkey) {
            let _ = crate::shell::register_hotkey(&app, &previous.hotkey);
            core.store().save_settings(previous).map_err(err)?;
            return Err(format!("HOTKEY_INVALID:{e}"));
        }
    }
    crate::shell::refresh_tray(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub async fn recognize_image(
    core: State<'_, Arc<Core>>,
    data: String,
    mode: Option<Mode>,
) -> Res<RecognizeResult> {
    let core = core.inner().clone();
    blocking(move || {
        let b64 = data.split_once(',').map_or(data.as_str(), |(_, b)| b);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64.trim())
            .context("invalid image data")?;
        core.recognize_bytes(&bytes, mode)
    })
    .await
}

#[tauri::command]
pub async fn recognize_file(
    core: State<'_, Arc<Core>>,
    path: String,
    mode: Option<Mode>,
) -> Res<RecognizeResult> {
    let core = core.inner().clone();
    blocking(move || {
        core.recognize_bytes(
            &std::fs::read(&path).with_context(|| format!("cannot read {path}"))?,
            mode,
        )
    })
    .await
}

#[tauri::command]
pub async fn capture(
    app: AppHandle,
    core: State<'_, Arc<Core>>,
    mode: Option<Mode>,
) -> Res<Option<RecognizeResult>> {
    let window = app.get_webview_window(crate::shell::MAIN);
    let was_visible = window
        .as_ref()
        .is_some_and(|w| w.is_visible().unwrap_or(false));
    if was_visible {
        let _ = window.as_ref().map(|w| w.hide());
        // Let the window finish ordering out so it is not in the screenshot.
        std::thread::sleep(std::time::Duration::from_millis(180));
    }
    let shot = tauri::async_runtime::spawn_blocking(screencapture)
        .await
        .map_err(|e| e.to_string())?;
    let got_image = matches!(shot, Ok(Some(_)));
    // Show the window for the result dialog; a cancelled background capture stays hidden.
    if was_visible || got_image {
        crate::shell::show_main(&app);
    }
    let Some(bytes) = shot.map_err(err)? else {
        return Ok(None);
    };
    let core = core.inner().clone();
    blocking(move || core.recognize_bytes(&bytes, mode).map(Some)).await
}

#[tauri::command]
pub async fn download_models(
    app: AppHandle,
    core: State<'_, Arc<Core>>,
    mode: Mode,
) -> Res<BTreeMap<&'static str, bool>> {
    let core = core.inner().clone();
    blocking(move || core.download_models(mode, &mut |p| drop(app.emit("model-progress", p)))).await
}

#[tauri::command]
pub async fn test_external(settings: Settings) -> Res<String> {
    blocking(move || external::test_connection(&settings)).await
}

#[tauri::command]
pub async fn export_file(path: String, content: String, format: String) -> Res<()> {
    blocking(move || export(Path::new(&path), &content, &format)).await
}

#[tauri::command]
pub fn history_op(
    core: State<'_, Arc<Core>>,
    op: String,
    index: Option<usize>,
    text: Option<String>,
    kind: Option<String>,
    name: Option<String>,
) -> Res<Vec<Entry>> {
    let mut s = core.store();
    let idx = || index.ok_or_else(|| anyhow!("index required"));
    match op.as_str() {
        "add" => s.add_history(
            text.as_deref().unwrap_or(""),
            kind.as_deref().unwrap_or("mathcraft"),
        ),
        "remove" => idx().and_then(|i| s.remove_history(i)),
        "rename" => idx().and_then(|i| s.rename_history(i, name.as_deref().unwrap_or(""))),
        "clear" => s.clear_history(),
        _ => Err(anyhow!("unknown op {op}")),
    }
    .map_err(err)?;
    Ok(s.history.clone())
}

#[tauri::command]
pub fn favorite_op(
    core: State<'_, Arc<Core>>,
    op: String,
    index: Option<usize>,
    text: Option<String>,
    kind: Option<String>,
    name: Option<String>,
) -> Res<Vec<Entry>> {
    let mut s = core.store();
    let idx = || index.ok_or_else(|| anyhow!("index required"));
    match op.as_str() {
        "add" => s
            .add_favorite(
                text.as_deref().unwrap_or(""),
                kind.as_deref().unwrap_or("mathcraft"),
                name.as_deref().unwrap_or(""),
            )
            .map(drop),
        "remove" => idx().and_then(|i| s.remove_favorite(i)),
        "rename" => idx().and_then(|i| s.rename_favorite(i, name.as_deref().unwrap_or(""))),
        "clear" => s.clear_favorites(),
        _ => Err(anyhow!("unknown op {op}")),
    }
    .map_err(err)?;
    Ok(s.favorites.clone())
}

#[tauri::command]
pub fn suspend_hotkey(app: AppHandle) -> Res<()> {
    crate::shell::suspend_hotkey(&app).map_err(err)
}

#[tauri::command]
pub fn resume_hotkey(app: AppHandle, core: State<'_, Arc<Core>>) -> Res<()> {
    crate::shell::register_hotkey(&app, &core.settings().hotkey).map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_models_are_reported_with_a_stable_code() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path());
        let png = {
            let img = image::RgbImage::from_pixel(8, 8, image::Rgb([255, 255, 255]));
            let mut v = Vec::new();
            img.write_to(&mut std::io::Cursor::new(&mut v), image::ImageFormat::Png)
                .unwrap();
            v
        };
        let e = core
            .recognize_bytes(&png, Some(Mode::Formula))
            .unwrap_err()
            .to_string();
        assert_eq!(e, "MODELS_MISSING:mathcraft-formula-rec");
        let snap = core.snapshot();
        assert_eq!(snap.models.get("formula"), Some(&false));
        assert_eq!(snap.version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn export_writes_text_and_rejects_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.tex");
        export(&p, "x^2", "tex").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "x^2");
        assert!(export(&p, "x", "exe").is_err());
    }

    #[test]
    fn export_docx_uses_pandoc_when_present() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.docx");
        let r = export(&p, "# Title\n\n$x^2$", "docx");
        match pandoc_path() {
            Some(_) => assert!(r.is_ok() && p.metadata().unwrap().len() > 0),
            None => assert!(r.unwrap_err().to_string().contains("Pandoc")),
        }
    }
}
