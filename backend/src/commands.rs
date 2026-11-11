//! IPC surface exposed to the webview. Each command is a thin wrapper around
//! `Core`, which holds all logic so it can be tested without a window.

use crate::external;
use crate::modelhub::{self, Progress};
use crate::ocr::{imageops, Engine, Mode, Recognition};
use crate::store::{kind_for, Entry, Settings, Store};
use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

pub struct Core {
    pub store: Mutex<Store>,
    /// Rebuilt after a model download: sessions resolve model paths once.
    engine: Mutex<Option<Arc<Engine>>>,
    downloading: Mutex<HashSet<String>>,
}

/// One MathCraft model as shown in Settings.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct ModelInfo {
    pub id: &'static str,
    pub installed: bool,
    pub downloading: bool,
    pub download_bytes: u64,
    /// Recognition types that need this model.
    pub used_by: Vec<Mode>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct ModelsState {
    pub models: Vec<ModelInfo>,
    /// Whether each recognition type has everything it needs.
    pub modes: BTreeMap<&'static str, bool>,
}

/// Removes a model from the in-flight set when its download ends, however it ends.
struct InFlight<'a> {
    set: &'a Mutex<HashSet<String>>,
    id: String,
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.set
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.id);
    }
}

#[derive(Serialize, Debug)]
pub struct Snapshot {
    pub version: &'static str,
    pub settings: Settings,
    pub history: Vec<Entry>,
    pub favorites: Vec<Entry>,
    pub models: BTreeMap<&'static str, bool>,
    pub model_files: Vec<ModelInfo>,
    pub pandoc: bool,
    pub capture_supported: bool,
    /// Headless self-check run (smoke test): the UI reports back and the app exits.
    pub selfcheck: bool,
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
            engine: Mutex::new(None),
            downloading: Mutex::new(HashSet::new()),
        }
    }

    fn store(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn engine(&self) -> Arc<Engine> {
        let root = self.store().models_dir();
        self.engine
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get_or_insert_with(|| Arc::new(Engine::new(&root)))
            .clone()
    }

    /// Drop the cached engine so the next recognition sees newly installed models.
    /// Recognitions already running keep their own `Arc`.
    fn invalidate_engine(&self) {
        *self.engine.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }

    pub fn models_state(&self) -> ModelsState {
        let root = self.store().models_dir();
        let downloading = self
            .downloading
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let models = modelhub::CATALOG
            .iter()
            .map(|&(id, download_bytes)| ModelInfo {
                id,
                installed: modelhub::is_installed(&root, id),
                downloading: downloading.contains(id),
                download_bytes,
                used_by: [Mode::Formula, Mode::Mixed, Mode::Text]
                    .into_iter()
                    .filter(|m| m.required_models().contains(&id))
                    .collect(),
            })
            .collect();
        ModelsState {
            models,
            modes: model_status(&root),
        }
    }

    /// Download (or re-download) one model with `fetch`, which installs it under the root.
    pub fn download_model_with(
        &self,
        id: &str,
        fetch: impl FnOnce(&Path, &str, &mut dyn FnMut(Progress)) -> Result<()>,
        on_progress: &mut dyn FnMut(Progress),
    ) -> Result<ModelsState> {
        if !modelhub::CATALOG.iter().any(|(known, _)| *known == id) {
            bail!("unknown model {id}");
        }
        let guard = {
            let mut set = self.downloading.lock().unwrap_or_else(|p| p.into_inner());
            if !set.insert(id.to_string()) {
                bail!("DOWNLOAD_IN_PROGRESS:{id}");
            }
            InFlight {
                set: &self.downloading,
                id: id.to_string(),
            }
        };
        let root = self.store().models_dir();
        let fetched = fetch(&root, id, on_progress);
        drop(guard); // report the finished state, not "downloading"
        fetched?;
        self.invalidate_engine();
        Ok(self.models_state())
    }

    /// Delete one installed model. Refused while that model is downloading.
    pub fn delete_model(&self, id: &str) -> Result<ModelsState> {
        if !modelhub::CATALOG.iter().any(|(known, _)| *known == id) {
            bail!("unknown model {id}");
        }
        let guard = {
            let mut set = self.downloading.lock().unwrap_or_else(|p| p.into_inner());
            if !set.insert(id.to_string()) {
                bail!("DOWNLOAD_IN_PROGRESS:{id}");
            }
            InFlight {
                set: &self.downloading,
                id: id.to_string(),
            }
        };
        let root = self.store().models_dir();
        modelhub::remove(&root, id)?;
        // Recognitions already running keep their loaded sessions; new ones see it missing.
        self.invalidate_engine();
        drop(guard);
        Ok(self.models_state())
    }

    pub fn download_model(
        &self,
        id: &str,
        on_progress: &mut dyn FnMut(Progress),
    ) -> Result<ModelsState> {
        self.download_model_with(id, modelhub::download, on_progress)
    }

    pub fn snapshot(&self) -> Snapshot {
        let model_files = self.models_state().models;
        let s = self.store();
        Snapshot {
            version: env!("CARGO_PKG_VERSION"),
            settings: s.settings.clone(),
            history: s.history.clone(),
            favorites: s.favorites.clone(),
            models: model_status(&s.models_dir()),
            model_files,
            selfcheck: crate::headless(),
            pandoc: pandoc_path().is_some(),
            capture_supported: crate::shell::CAPTURE_SUPPORTED,
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
    ) -> Result<ModelsState> {
        let root = self.store().models_dir();
        for id in mode.required_models() {
            if !modelhub::is_installed(&root, id) {
                self.download_model(id, on_progress)?;
            }
        }
        Ok(self.models_state())
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
    if !crate::shell::CAPTURE_SUPPORTED {
        return Err("Screen capture is unavailable on this platform".into());
    }
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
) -> Res<ModelsState> {
    let core = core.inner().clone();
    blocking(move || core.download_models(mode, &mut |p| drop(app.emit("model-progress", p)))).await
}

#[tauri::command]
pub fn list_models(core: State<'_, Arc<Core>>) -> ModelsState {
    core.models_state()
}

#[tauri::command]
pub async fn download_model(
    app: AppHandle,
    core: State<'_, Arc<Core>>,
    model: String,
) -> Res<ModelsState> {
    let core = core.inner().clone();
    blocking(move || core.download_model(&model, &mut |p| drop(app.emit("model-progress", p))))
        .await
}

#[tauri::command]
pub async fn delete_model(core: State<'_, Arc<Core>>, model: String) -> Res<ModelsState> {
    let core = core.inner().clone();
    blocking(move || core.delete_model(&model)).await
}

/// The UI's answer to a headless self-check; ignored in normal runs.
#[tauri::command]
pub fn frontend_ready(app: AppHandle, ok: bool, detail: String) {
    if crate::headless() {
        println!("LATEXSNIPPER_READY ok={ok} {detail}");
        app.exit(if ok { 0 } else { 1 });
    }
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

    fn install_fake(root: &Path, id: &str) {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        for f in modelhub::spec(id).unwrap().files {
            std::fs::write(dir.join(f.path), b"x").unwrap();
        }
    }

    #[test]
    fn models_state_reports_presence_and_usage() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path());
        install_fake(
            &dir.path().join("MathCraft/models"),
            "mathcraft-formula-rec",
        );
        let state = core.models_state();
        let ids: Vec<&str> = state.models.iter().map(|m| m.id).collect();
        assert_eq!(
            ids,
            [
                "mathcraft-formula-det",
                "mathcraft-formula-rec",
                "mathcraft-text-det",
                "mathcraft-text-rec"
            ]
        );
        let by_id = |id: &str| state.models.iter().find(|m| m.id == id).unwrap().clone();
        assert!(by_id("mathcraft-formula-rec").installed);
        assert!(!by_id("mathcraft-text-det").installed);
        assert_eq!(by_id("mathcraft-formula-det").used_by, [Mode::Mixed]);
        assert_eq!(
            by_id("mathcraft-formula-rec").used_by,
            [Mode::Formula, Mode::Mixed]
        );
        assert_eq!(
            by_id("mathcraft-text-rec").used_by,
            [Mode::Mixed, Mode::Text]
        );
        assert_eq!(state.modes.get("formula"), Some(&true));
        assert_eq!(state.modes.get("text"), Some(&false));
        assert_eq!(core.snapshot().model_files, state.models);
    }

    #[test]
    fn single_downloads_are_validated_and_exclusive() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path());
        let mut progress = |_| {};
        let err = core
            .download_model_with("nope", |_, _, _| Ok(()), &mut progress)
            .unwrap_err();
        assert!(err.to_string().contains("unknown model"));

        // A second request for a model that is still downloading is refused.
        let state = core
            .download_model_with(
                "mathcraft-text-det",
                |root, id, _| {
                    assert!(core.models_state().models[2].downloading);
                    let again = core.download_model_with(id, |_, _, _| Ok(()), &mut |_| {});
                    assert_eq!(
                        again.unwrap_err().to_string(),
                        "DOWNLOAD_IN_PROGRESS:mathcraft-text-det"
                    );
                    install_fake(root, id);
                    Ok(())
                },
                &mut progress,
            )
            .unwrap();
        assert!(state.models[2].installed && !state.models[2].downloading);

        // A failed download releases the model so it can be retried.
        let failed = core.download_model_with(
            "mathcraft-text-rec",
            |_, _, _| Err(anyhow!("network unreachable")),
            &mut progress,
        );
        assert!(failed.is_err());
        assert!(!core.models_state().models[3].downloading);
        assert!(
            core.download_model_with(
                "mathcraft-text-rec",
                |r, id, _| {
                    install_fake(r, id);
                    Ok(())
                },
                &mut progress
            )
            .unwrap()
            .modes["text"]
        );
    }

    #[test]
    fn delete_removes_one_model_and_is_exclusive() {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new(dir.path());
        let root = dir.path().join("MathCraft/models");
        for id in ["mathcraft-text-det", "mathcraft-text-rec"] {
            install_fake(&root, id);
        }
        assert!(core.models_state().modes["text"]);
        let state = core.delete_model("mathcraft-text-det").unwrap();
        assert!(!state.models[2].installed && !state.models[2].downloading);
        assert!(state.models[3].installed, "other models are untouched");
        assert!(!state.modes["text"]);
        assert!(
            core.delete_model("mathcraft-text-det").is_ok(),
            "deleting twice is harmless"
        );
        assert!(core
            .delete_model("../MathCraft")
            .unwrap_err()
            .to_string()
            .contains("unknown model"));

        // Deleting a model while it downloads is refused.
        core.download_model_with(
            "mathcraft-text-det",
            |root, id, _| {
                let err = core.delete_model(id).unwrap_err().to_string();
                assert_eq!(err, "DOWNLOAD_IN_PROGRESS:mathcraft-text-det");
                install_fake(root, id);
                Ok(())
            },
            &mut |_| {},
        )
        .unwrap();
        assert!(core.models_state().modes["text"]);
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
