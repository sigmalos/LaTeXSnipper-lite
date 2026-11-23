//! Self-update from GitHub releases, and the changelog shown after one.
//!
//! Release builds check `latest.json` on the latest GitHub release a few seconds
//! after launch and every few hours after that. A newer, signed release is
//! downloaded in the background, then:
//! - macOS: installed at once (the bundle is swapped; the running app is
//!   unaffected) and used from the next launch;
//! - Windows: kept in memory and installed silently when the app quits, since the
//!   NSIS installer has to replace the running program;
//! - Linux: kept until 安装并重启 is clicked, since dpkg asks for a password.
//!
//! The release notes are written next to the app data right before installing,
//! and the first launch of that version shows them once.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_updater::{Update, UpdaterExt};

pub const STATUS_EVENT: &str = "update-status";
const FIRST_CHECK: Duration = Duration::from_secs(8);
const RECHECK: Duration = Duration::from_secs(6 * 60 * 60);
const RETRY: Duration = Duration::from_secs(30 * 60);
const CHANGELOG_FILE: &str = "update-changelog.json";

/// Where an update stands, as the settings button shows it.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Checking,
    UpToDate,
    Available {
        version: String,
    },
    Downloading {
        version: String,
        done: u64,
        total: Option<u64>,
    },
    /// macOS and Linux: installed or ready to install, takes effect on restart.
    /// Windows: downloaded, installed when the app quits (or now, on request).
    Ready {
        version: String,
    },
    Failed {
        error: String,
    },
}

#[derive(Serialize, Clone, Debug)]
pub struct UpdateView {
    #[serde(flatten)]
    pub phase: Phase,
    /// Windows installs a downloaded update when the app quits.
    pub installs_on_quit: bool,
}

/// Release notes of the version just updated to, shown once on its first launch.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Changelog {
    pub version: String,
    pub notes: String,
}

#[derive(Default)]
struct Inner {
    phase: Option<Phase>,
    update: Option<Update>,
    bytes: Option<Vec<u8>>,
    /// The macOS bundle has already been replaced.
    installed: bool,
}

#[derive(Default)]
pub struct Updates(Mutex<Inner>);

impl Updates {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn view(&self) -> UpdateView {
        UpdateView {
            phase: self.lock().phase.clone().unwrap_or(Phase::Idle),
            installs_on_quit: cfg!(windows),
        }
    }
}

/// Self-update runs only in release builds launched normally, not in dev or smoke tests.
pub fn enabled() -> bool {
    !cfg!(debug_assertions) && !crate::headless()
}

fn set<R: Runtime>(app: &AppHandle<R>, phase: Phase) {
    app.state::<Updates>().lock().phase = Some(phase);
    let _ = app.emit(STATUS_EVENT, app.state::<Updates>().view());
}

fn phase<R: Runtime>(app: &AppHandle<R>) -> Phase {
    app.state::<Updates>().view().phase
}

fn busy(p: &Phase) -> bool {
    matches!(
        p,
        Phase::Checking | Phase::Downloading { .. } | Phase::Ready { .. }
    )
}

/// Ask GitHub for a newer release. Quiet checks leave the button alone when
/// nothing is found or the network is down; a manual check reports both.
pub async fn check<R: Runtime>(app: &AppHandle<R>, manual: bool) -> anyhow::Result<Phase> {
    let current = phase(app);
    if busy(&current) {
        return Ok(current);
    }
    if manual {
        set(app, Phase::Checking);
    }
    let found = async { anyhow::Ok(app.updater()?.check().await?) }.await;
    if let Err(e) = &found {
        eprintln!("update check failed: {e:#}");
    }
    let next = match found {
        Ok(Some(update)) => {
            let version = update.version.clone();
            app.state::<Updates>().lock().update = Some(update);
            Phase::Available { version }
        }
        Ok(None) if manual => Phase::UpToDate,
        Err(e) if manual => Phase::Failed {
            error: format!("{e:#}"),
        },
        Ok(None) | Err(_) => match current {
            Phase::Available { .. } => current,
            _ => Phase::Idle,
        },
    };
    set(app, next.clone());
    Ok(next)
}

/// Download the update found by `check`, verifying its signature, then install
/// it where that does not disturb the running app (macOS).
pub async fn download<R: Runtime>(app: &AppHandle<R>) -> anyhow::Result<()> {
    let Some(update) = app.state::<Updates>().lock().update.clone() else {
        anyhow::bail!("no update to download");
    };
    let version = update.version.clone();
    let mut done = 0u64;
    let mut shown = u64::MAX;
    set(
        app,
        Phase::Downloading {
            version: version.clone(),
            done: 0,
            total: None,
        },
    );
    let bytes = update
        .download(
            |chunk, total| {
                done += chunk as u64;
                // One event per percent is plenty for a progress label.
                let step = total.map_or(done >> 20, |t| done * 100 / t.max(1));
                if step != shown {
                    shown = step;
                    set(
                        app,
                        Phase::Downloading {
                            version: version.clone(),
                            done,
                            total,
                        },
                    );
                }
            },
            || {},
        )
        .await;
    let bytes = match bytes {
        Ok(b) => b,
        Err(e) => {
            set(app, Phase::Available { version });
            return Err(e.into());
        }
    };
    if cfg!(target_os = "macos") {
        if let Err(e) = install_now(&update, &bytes) {
            set(app, Phase::Available { version });
            return Err(e);
        }
        app.state::<Updates>().lock().installed = true;
    } else {
        app.state::<Updates>().lock().bytes = Some(bytes);
    }
    set(app, Phase::Ready { version });
    Ok(())
}

fn install_now(update: &Update, bytes: &[u8]) -> anyhow::Result<()> {
    save_changelog(
        &crate::data_dir(),
        &Changelog {
            version: update.version.clone(),
            notes: update.body.clone().unwrap_or_default(),
        },
    )?;
    update.install(bytes)?;
    Ok(())
}

/// The settings button: download if needed, install, and restart into the new version.
pub async fn apply<R: Runtime>(app: &AppHandle<R>) -> anyhow::Result<()> {
    anyhow::ensure!(enabled(), "updates are installed only by release builds");
    if matches!(phase(app), Phase::Available { .. }) {
        download(app).await?;
    }
    let (update, bytes, installed) = {
        let state = app.state::<Updates>();
        let mut inner = state.lock();
        (inner.update.clone(), inner.bytes.take(), inner.installed)
    };
    let Some(update) = update else {
        anyhow::bail!("no update to install");
    };
    if !installed {
        let bytes = bytes.ok_or_else(|| anyhow::anyhow!("the update has not been downloaded"))?;
        // Windows exits here and the installer starts the new version (`/R`).
        install_now(&update.restart_after_install(true), &bytes)?;
    }
    app.restart();
}

/// Windows: install a downloaded update as the app quits. The installer runs
/// silently and does not reopen the app.
pub fn install_on_exit<R: Runtime>(app: &AppHandle<R>) {
    if !cfg!(windows) || !enabled() {
        return;
    }
    let (update, bytes) = {
        let state = app.state::<Updates>();
        let mut inner = state.lock();
        (inner.update.clone(), inner.bytes.take())
    };
    if let (Some(update), Some(bytes)) = (update, bytes) {
        let version = update.version.clone();
        if let Err(e) = install_now(&update.restart_after_install(false), &bytes) {
            eprintln!("update {version} not installed: {e:#}");
        }
    }
}

/// Check now and then; download whatever is found.
pub fn spawn_auto<R: Runtime>(app: AppHandle<R>) {
    if !enabled() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            let found = check(&app, false).await;
            let wait = match found {
                Ok(Phase::Available { .. }) => match download(&app).await {
                    Ok(()) => RECHECK,
                    Err(_) => RETRY,
                },
                Ok(Phase::Ready { .. }) | Ok(Phase::Idle) | Ok(Phase::UpToDate) => RECHECK,
                _ => RETRY,
            };
            if matches!(phase(&app), Phase::Ready { .. }) {
                return; // nothing more to do until the next launch
            }
            tokio::time::sleep(wait).await;
        }
    });
}

// ------------------------------------------------------------------ changelog

fn changelog_path(dir: &Path) -> PathBuf {
    dir.join(CHANGELOG_FILE)
}

pub fn save_changelog(dir: &Path, log: &Changelog) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(changelog_path(dir), serde_json::to_vec_pretty(log)?)?;
    Ok(())
}

/// The notes to show on this launch: only when this is the version they were saved for.
/// A file left by an update that never installed is dropped.
pub fn pending_changelog(dir: &Path, current: &str) -> Option<Changelog> {
    let path = changelog_path(dir);
    let log: Changelog = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
    if log.version == current {
        Some(log)
    } else {
        let _ = std::fs::remove_file(&path);
        None
    }
}

pub fn dismiss_changelog(dir: &Path) {
    let _ = std::fs::remove_file(changelog_path(dir));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changelog_is_shown_once_for_its_own_version() {
        let dir = tempfile::tempdir().unwrap();
        let log = Changelog {
            version: "1.4.0".into(),
            notes: "### 改进\n- a".into(),
        };
        save_changelog(dir.path(), &log).unwrap();
        assert_eq!(pending_changelog(dir.path(), "1.4.0"), Some(log.clone()));
        assert_eq!(pending_changelog(dir.path(), "1.4.0"), Some(log));
        dismiss_changelog(dir.path());
        assert_eq!(pending_changelog(dir.path(), "1.4.0"), None);
    }

    #[test]
    fn a_changelog_for_another_version_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let log = Changelog {
            version: "1.5.0".into(),
            notes: String::new(),
        };
        save_changelog(dir.path(), &log).unwrap();
        assert_eq!(pending_changelog(dir.path(), "1.4.0"), None);
        assert!(!changelog_path(dir.path()).exists());
    }

    #[test]
    fn status_serializes_flat_for_the_ui() {
        let v = UpdateView {
            phase: Phase::Downloading {
                version: "1.4.0".into(),
                done: 5,
                total: Some(10),
            },
            installs_on_quit: false,
        };
        assert_eq!(
            serde_json::to_value(v).unwrap(),
            serde_json::json!({"phase": "downloading", "version": "1.4.0", "done": 5, "total": 10, "installs_on_quit": false})
        );
    }
}
