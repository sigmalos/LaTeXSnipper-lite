//! LaTeXSnipper Office.app: the Setup window. It installs, repairs and
//! uninstalls the add-in and downloads the recognition model by running the
//! bundled host (`latexsnipper-office-host`), which does all the work and
//! keeps running from login on its own.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tauri::{AppHandle, Emitter};

/// Next to this executable in the bundle (`Contents/MacOS`); in development,
/// the host built in `office_plugin/host`.
fn host_program() -> PathBuf {
    if let Some(path) = std::env::var_os("LATEXSNIPPER_OFFICE_HOST") {
        return PathBuf::from(path);
    }
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("latexsnipper-office-host")));
    match bundled {
        Some(path) if path.is_file() => path,
        _ => PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../host/target/debug/latexsnipper-office-host"),
    }
}

fn run_host(flag: &'static str) -> Result<Value, String> {
    let out = Command::new(host_program())
        .arg(flag)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("HOST_MISSING:{e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(stderr.lines().next().unwrap_or("FAILED").to_string());
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.lines().last().unwrap_or_default())
        .map_err(|e| format!("INVALID_OUTPUT:{e}"))
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn status() -> Result<Value, String> {
    blocking(|| run_host("--status")).await
}

/// Trusting the certificate shows the macOS password prompt.
#[tauri::command]
async fn install() -> Result<Value, String> {
    blocking(|| run_host("--install")).await
}

#[tauri::command]
async fn uninstall() -> Result<Value, String> {
    blocking(|| run_host("--uninstall")).await
}

/// Streams `{"downloaded", "total"}` lines as `model-progress` events.
#[tauri::command]
async fn download_model(app: AppHandle) -> Result<Value, String> {
    blocking(move || {
        let mut child = Command::new(host_program())
            .arg("--download-model")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("HOST_MISSING:{e}"))?;
        let mut last = Value::Null;
        if let Some(stdout) = child.stdout.take() {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let Ok(value) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if value.get("total").is_some() {
                    let _ = app.emit("model-progress", &value);
                } else {
                    last = value;
                }
            }
        }
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(stderr.lines().next().unwrap_or("FAILED").to_string());
        }
        Ok(last)
    })
    .await
}

const HELP_URL: &str =
    "https://github.com/sigmalos/LaTeXSnipper-lite/blob/main/office_plugin/README.md";

#[tauri::command]
fn open_help() -> Result<(), String> {
    Command::new("/usr/bin/open")
        .arg(HELP_URL)
        .status()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            status,
            install,
            uninstall,
            download_model,
            open_help
        ])
        .run(tauri::generate_context!())
        .expect("error while running LaTeXSnipper Office");
}
