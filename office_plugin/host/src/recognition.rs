//! Screenshot recognition for the add-in pages.
//!
//! The pages speak the job protocol of the original add-in (`next`, then poll
//! the job, or cancel it). The original relayed it to the desktop app's
//! Automation API; this host runs the job itself: an interactive
//! `screencapture` selection, then the MathCraft formula model linked into
//! this binary. The LaTeXSnipper desktop app does not need to be running.
//!
//! Models are read from the desktop app's folder when it has them (never
//! written), otherwise from the add-in's own folder, where Setup downloads them.

use crate::api::ApiError;
use mathcraft::modelhub;
use mathcraft::ocr::{imageops, Engine, Mode};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const FORMULA_MODEL: &str = "mathcraft-formula-rec";
const KEEP_FINISHED: Duration = Duration::from_secs(600);
const ENGINE_IDLE: Duration = Duration::from_secs(600);

pub const MODEL_MISSING: &str =
    "截图识别需要 MathCraft 公式识别模型。请打开“LaTeXSnipper Office”下载模型后重试。";
pub const BUSY: &str = "截图识别正忙，请稍后再试。";
pub const TIMED_OUT: &str = "截图识别超时，请重试。";
pub const CANCELED: &str = "截图识别已取消。";
pub const EMPTY: &str = "未识别到公式内容，请重新截图识别。";
pub const FAILED: &str = "截图识别未能完成，请重试。";
pub const CAPTURE_FAILED: &str =
    "无法启动截图。请在“系统设置 › 隐私与安全性 › 屏幕录制”中允许 LaTeXSnipper Office 后重试。";

#[derive(Debug, PartialEq)]
pub enum Captured {
    Image(Vec<u8>),
    Canceled,
    TimedOut,
}

pub trait Capturer: Send + Sync {
    /// Let the user select a screen area; polls `cancel` and stops at `deadline`.
    fn capture(&self, cancel: &AtomicBool, deadline: Instant) -> anyhow::Result<Captured>;
}

pub trait Recognizer: Send + Sync {
    /// Whether recognition can run (the model is installed).
    fn ready(&self) -> bool;
    fn recognize(&self, image: &[u8]) -> anyhow::Result<String>;
}

// ------------------------------------------------------------------ jobs

#[derive(Clone)]
struct Job {
    state: &'static str,
    text: Option<String>,
    error: Option<(&'static str, &'static str)>,
    cancel: Arc<AtomicBool>,
    finished: Option<Instant>,
}

impl Job {
    fn active(&self) -> bool {
        matches!(self.state, "awaiting_result" | "running")
    }

    fn to_json(&self, id: &str) -> Value {
        let items: Vec<Value> = self
            .text
            .iter()
            .map(|t| json!({"index": 0, "text": t, "status": "completed"}))
            .collect();
        let error = self
            .error
            .map(|(code, message)| json!({"code": code, "message": message}));
        json!({"job": {"id": id, "state": self.state, "items": items, "error": error}})
    }
}

pub struct Jobs {
    capturer: Box<dyn Capturer>,
    recognizer: Box<dyn Recognizer>,
    jobs: Mutex<HashMap<String, Job>>,
}

impl Jobs {
    pub fn new(capturer: Box<dyn Capturer>, recognizer: Box<dyn Recognizer>) -> Arc<Self> {
        Arc::new(Self {
            capturer,
            recognizer,
            jobs: Mutex::new(HashMap::new()),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Job>> {
        let mut jobs = self.jobs.lock().unwrap_or_else(|p| p.into_inner());
        jobs.retain(|_, j| j.finished.is_none_or(|t| t.elapsed() < KEEP_FINISHED));
        jobs
    }

    fn update(&self, id: &str, f: impl FnOnce(&mut Job)) {
        if let Some(job) = self.lock().get_mut(id) {
            // A canceled job stays canceled.
            if job.state != "canceled" {
                f(job);
            }
        }
    }

    fn finish(
        &self,
        id: &str,
        state: &'static str,
        error: Option<(&'static str, &'static str)>,
        text: Option<String>,
    ) {
        self.update(id, |job| {
            job.state = state;
            job.error = error;
            job.text = text;
            job.finished = Some(Instant::now());
        });
    }

    pub fn config(&self) -> Result<Value, ApiError> {
        if !self.recognizer.ready() {
            return Err(ApiError::new(503, "model_unavailable", MODEL_MISSING));
        }
        Ok(json!({"engine": "mathcraft", "modes": ["formula"], "capture": true}))
    }

    /// Start a capture and recognition; the job runs in the background.
    pub fn next(self: &Arc<Self>, timeout: Duration) -> Result<Value, ApiError> {
        if !self.recognizer.ready() {
            return Err(ApiError::new(503, "model_unavailable", MODEL_MISSING));
        }
        let id = crate::paths::random_hex(16);
        let job = Job {
            state: "awaiting_result",
            text: None,
            error: None,
            cancel: Arc::new(AtomicBool::new(false)),
            finished: None,
        };
        {
            let mut jobs = self.lock();
            if jobs.values().any(Job::active) {
                return Err(ApiError::new(409, "next_result_busy", BUSY));
            }
            jobs.insert(id.clone(), job.clone());
        }
        let this = self.clone();
        let job_id = id.clone();
        std::thread::spawn(move || this.run(&job_id, &job.cancel, Instant::now() + timeout));
        Ok(self.lock()[&id].to_json(&id))
    }

    fn run(&self, id: &str, cancel: &AtomicBool, deadline: Instant) {
        let image = match self.capturer.capture(cancel, deadline) {
            Ok(Captured::Image(bytes)) => bytes,
            Ok(Captured::Canceled) => {
                return self.finish(id, "canceled", Some(("canceled", CANCELED)), None)
            }
            Ok(Captured::TimedOut) => {
                return self.finish(id, "failed", Some(("timeout", TIMED_OUT)), None)
            }
            Err(e) => {
                crate::paths::log(format!("screen capture failed: {e:#}"));
                return self.finish(id, "failed", Some(("capture_failed", CAPTURE_FAILED)), None);
            }
        };
        if cancel.load(Ordering::SeqCst) {
            return;
        }
        self.update(id, |job| job.state = "running");
        match self.recognizer.recognize(&image) {
            Ok(text) if text.trim().is_empty() => {
                self.finish(id, "failed", Some(("empty_formula", EMPTY)), None)
            }
            Ok(text) => self.finish(id, "completed", None, Some(text.trim().to_string())),
            Err(e) => {
                crate::paths::log(format!("recognition failed: {e:#}"));
                let code = if e.to_string().starts_with("MODELS_MISSING") {
                    ("model_unavailable", MODEL_MISSING)
                } else {
                    ("recognition_failed", FAILED)
                };
                self.finish(id, "failed", Some(code), None)
            }
        }
    }

    pub fn job(&self, id: &str) -> Result<Value, ApiError> {
        self.lock()
            .get(id)
            .map(|j| j.to_json(id))
            .ok_or_else(|| ApiError::new(404, "job_not_found", "识别任务已失效，请重新截图识别。"))
    }

    pub fn cancel(&self, id: &str) -> Result<Value, ApiError> {
        let mut jobs = self.lock();
        let job = jobs.get_mut(id).ok_or_else(|| {
            ApiError::new(404, "job_not_found", "识别任务已失效，请重新截图识别。")
        })?;
        if job.active() {
            job.cancel.store(true, Ordering::SeqCst);
            job.state = "canceled";
            job.error = Some(("canceled", CANCELED));
            job.finished = Some(Instant::now());
        }
        Ok(job.to_json(id))
    }
}

// ------------------------------------------------------------------ real capture

/// `screencapture -i`: the system selection UI; Esc cancels.
pub struct ScreenCapture;

impl Capturer for ScreenCapture {
    fn capture(&self, cancel: &AtomicBool, deadline: Instant) -> anyhow::Result<Captured> {
        let path = std::env::temp_dir().join(format!(
            "latexsnipper-office-{}.png",
            crate::paths::random_hex(8)
        ));
        let mut child = Command::new("/usr/sbin/screencapture")
            .args(["-i", "-x", "-t", "png"])
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let outcome = loop {
            if child.try_wait()?.is_some() {
                break None;
            }
            if cancel.load(Ordering::SeqCst) {
                break Some(Captured::Canceled);
            }
            if Instant::now() >= deadline {
                break Some(Captured::TimedOut);
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        if let Some(stopped) = outcome {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&path);
            return Ok(stopped);
        }
        let bytes = std::fs::read(&path).unwrap_or_default();
        let _ = std::fs::remove_file(&path);
        Ok(if bytes.is_empty() {
            Captured::Canceled
        } else {
            Captured::Image(bytes)
        })
    }
}

// ------------------------------------------------------------------ real recognizer

/// Where the formula model is: the desktop app's folder first, then the add-in's.
pub fn model_root(home: &Path, state: &Path) -> Option<(&'static str, PathBuf)> {
    [
        ("desktop", crate::paths::desktop_models_dir(home)),
        ("addin", crate::paths::own_models_dir(state)),
    ]
    .into_iter()
    .find(|(_, root)| modelhub::is_installed(root, FORMULA_MODEL))
}

struct Loaded {
    root: PathBuf,
    engine: Arc<Engine>,
    used: Instant,
}

/// The MathCraft formula model, loaded on first use and released when idle.
pub struct MathCraft {
    home: PathBuf,
    state: PathBuf,
    loaded: Arc<Mutex<Option<Loaded>>>,
}

impl MathCraft {
    pub fn new(home: PathBuf, state: PathBuf) -> Self {
        let loaded: Arc<Mutex<Option<Loaded>>> = Arc::new(Mutex::new(None));
        let weak = Arc::downgrade(&loaded);
        // The host runs from login; free the model's memory when unused.
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(60));
            let Some(loaded) = weak.upgrade() else { return };
            let mut guard = loaded.lock().unwrap_or_else(|p| p.into_inner());
            if guard
                .as_ref()
                .is_some_and(|l| l.used.elapsed() > ENGINE_IDLE)
            {
                *guard = None;
            }
        });
        Self {
            home,
            state,
            loaded,
        }
    }

    fn engine(&self) -> anyhow::Result<Arc<Engine>> {
        let (_, root) = model_root(&self.home, &self.state)
            .ok_or_else(|| anyhow::anyhow!("MODELS_MISSING:{FORMULA_MODEL}"))?;
        let mut guard = self.loaded.lock().unwrap_or_else(|p| p.into_inner());
        if guard.as_ref().is_none_or(|l| l.root != root) {
            *guard = Some(Loaded {
                engine: Arc::new(Engine::new(&root)),
                root,
                used: Instant::now(),
            });
        }
        let loaded = guard.as_mut().expect("set above");
        loaded.used = Instant::now();
        Ok(loaded.engine.clone())
    }
}

impl Recognizer for MathCraft {
    fn ready(&self) -> bool {
        model_root(&self.home, &self.state).is_some()
    }

    fn recognize(&self, image: &[u8]) -> anyhow::Result<String> {
        let image = imageops::decode_rgb(image)?;
        Ok(self.engine()?.recognize(&image, Mode::Formula)?.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeCapture(Mutex<Option<Captured>>, Duration);
    impl Capturer for FakeCapture {
        fn capture(&self, cancel: &AtomicBool, deadline: Instant) -> anyhow::Result<Captured> {
            let until = Instant::now() + self.1;
            while Instant::now() < until {
                if cancel.load(Ordering::SeqCst) {
                    return Ok(Captured::Canceled);
                }
                if Instant::now() >= deadline {
                    return Ok(Captured::TimedOut);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(self.0.lock().unwrap().take().unwrap_or(Captured::Canceled))
        }
    }

    struct FakeRecognizer(bool, &'static str);
    impl Recognizer for FakeRecognizer {
        fn ready(&self) -> bool {
            self.0
        }
        fn recognize(&self, image: &[u8]) -> anyhow::Result<String> {
            if self.1 == "fail" {
                anyhow::bail!("boom");
            }
            Ok(format!("{}{}", self.1, image.len()))
        }
    }

    fn jobs(result: Captured, delay_ms: u64, recognizer: FakeRecognizer) -> Arc<Jobs> {
        Jobs::new(
            Box::new(FakeCapture(
                Mutex::new(Some(result)),
                Duration::from_millis(delay_ms),
            )),
            Box::new(recognizer),
        )
    }

    fn wait(jobs: &Jobs, id: &str) -> Value {
        for _ in 0..400 {
            let v = jobs.job(id).unwrap();
            if !matches!(
                v["job"]["state"].as_str(),
                Some("awaiting_result" | "running")
            ) {
                return v;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("job did not finish");
    }

    fn id(v: &Value) -> String {
        v["job"]["id"].as_str().unwrap().to_string()
    }

    #[test]
    fn captures_and_recognizes() {
        let j = jobs(
            Captured::Image(vec![1, 2, 3]),
            20,
            FakeRecognizer(true, "x^"),
        );
        let created = j.next(Duration::from_secs(5)).unwrap();
        assert_eq!(created["job"]["state"], "awaiting_result");
        assert_eq!(id(&created).len(), 32);
        // Only one capture at a time.
        assert_eq!(
            j.next(Duration::from_secs(5)).unwrap_err().code,
            "next_result_busy"
        );
        let done = wait(&j, &id(&created));
        assert_eq!(done["job"]["state"], "completed");
        assert_eq!(done["job"]["items"][0]["text"], "x^3");
        assert_eq!(done["job"]["error"], Value::Null);
        // A finished job frees the slot.
        assert!(j.next(Duration::from_secs(5)).is_ok());
    }

    #[test]
    fn reports_cancel_timeout_empty_and_failure() {
        let j = jobs(Captured::Canceled, 0, FakeRecognizer(true, "x"));
        let v = wait(&j, &id(&j.next(Duration::from_secs(5)).unwrap()));
        assert_eq!(
            (
                v["job"]["state"].as_str(),
                v["job"]["error"]["code"].as_str()
            ),
            (Some("canceled"), Some("canceled"))
        );

        let j = jobs(Captured::Image(vec![]), 200, FakeRecognizer(true, "x"));
        let v = wait(&j, &id(&j.next(Duration::from_millis(20)).unwrap()));
        assert_eq!(v["job"]["error"]["code"], "timeout");

        struct Blank;
        impl Recognizer for Blank {
            fn ready(&self) -> bool {
                true
            }
            fn recognize(&self, _: &[u8]) -> anyhow::Result<String> {
                Ok("  ".into())
            }
        }
        let j = Jobs::new(
            Box::new(FakeCapture(
                Mutex::new(Some(Captured::Image(vec![1]))),
                Duration::ZERO,
            )),
            Box::new(Blank),
        );
        let v = wait(&j, &id(&j.next(Duration::from_secs(5)).unwrap()));
        assert_eq!(v["job"]["error"]["code"], "empty_formula");

        let j = jobs(Captured::Image(vec![1]), 0, FakeRecognizer(true, "fail"));
        let v = wait(&j, &id(&j.next(Duration::from_secs(5)).unwrap()));
        assert_eq!(
            (
                v["job"]["state"].as_str(),
                v["job"]["error"]["message"].as_str()
            ),
            (Some("failed"), Some(FAILED))
        );
    }

    #[test]
    fn cancel_stops_a_running_capture() {
        let j = jobs(Captured::Image(vec![1]), 2_000, FakeRecognizer(true, "x"));
        let created = j.next(Duration::from_secs(5)).unwrap();
        let canceled = j.cancel(&id(&created)).unwrap();
        assert_eq!(canceled["job"]["state"], "canceled");
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(j.job(&id(&created)).unwrap()["job"]["state"], "canceled");
        assert!(j.next(Duration::from_secs(5)).is_ok());
        assert_eq!(j.job("nope").unwrap_err().status, 404);
        assert_eq!(j.cancel("nope").unwrap_err().code, "job_not_found");
    }

    #[test]
    fn missing_model_is_reported_before_capturing() {
        let j = jobs(Captured::Image(vec![1]), 0, FakeRecognizer(false, "x"));
        let e = j.next(Duration::from_secs(5)).unwrap_err();
        assert_eq!((e.status, e.code.as_str()), (503, "model_unavailable"));
        assert_eq!(j.config().unwrap_err().code, "model_unavailable");
        let ready = jobs(Captured::Canceled, 0, FakeRecognizer(true, "x"));
        assert_eq!(ready.config().unwrap()["engine"], "mathcraft");
    }

    #[test]
    fn prefers_the_desktop_models_and_never_needs_them() {
        let home = tempfile::tempdir().unwrap();
        let state = home.path().join("state");
        assert!(model_root(home.path(), &state).is_none());
        let install = |root: &Path| {
            let dir = root.join(FORMULA_MODEL);
            std::fs::create_dir_all(&dir).unwrap();
            for f in modelhub::spec(FORMULA_MODEL).unwrap().files {
                std::fs::write(dir.join(f.path), b"x").unwrap();
            }
        };
        install(&crate::paths::own_models_dir(&state));
        assert_eq!(model_root(home.path(), &state).unwrap().0, "addin");
        install(&crate::paths::desktop_models_dir(home.path()));
        assert_eq!(model_root(home.path(), &state).unwrap().0, "desktop");
    }

    /// The real engine on a fixture, with the desktop app's models or
    /// `$LATEXSNIPPER_MODELS` (CI); skipped when neither is installed.
    #[test]
    fn recognizes_a_fixture_with_the_real_model() {
        let temp = tempfile::tempdir().unwrap();
        let home = match std::env::var_os("LATEXSNIPPER_MODELS") {
            Some(models) => {
                let link = crate::paths::desktop_models_dir(temp.path());
                std::fs::create_dir_all(link.parent().unwrap()).unwrap();
                std::os::unix::fs::symlink(models, &link).unwrap();
                temp.path().to_path_buf()
            }
            None => crate::paths::home(),
        };
        let m = MathCraft::new(home, temp.path().join("state"));
        if !m.ready() {
            eprintln!("skipped: MathCraft formula model not installed");
            return;
        }
        let png = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../backend/tests/fixtures/frac.png"),
        )
        .unwrap();
        let text = m.recognize(&png).unwrap();
        assert!(text.contains("\\frac"), "{text}");
    }
}
