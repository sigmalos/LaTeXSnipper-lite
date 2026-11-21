//! File locations and fixed identifiers.
//!
//! The identifiers match the original LaTeXSnipper Office for Mac, so this
//! host replaces it in place: same app name, LaunchAgent, port, manifest file
//! and settings file. The only desktop path it reads is the shared MathCraft
//! model folder, and it never writes there.

use std::path::{Path, PathBuf};

pub const APP_NAME: &str = "LaTeXSnipper Office";
pub const BUNDLE_ID: &str = "com.mathcraft.latexsnipper.office";
pub const AGENT_LABEL: &str = "com.mathcraft.latexsnipper.office.host";
pub const PORT: u16 = 28766;
pub const API_HEADER: &str = "x-latexsnipper-office";
/// The bundled resource directory (`Contents/Resources/office_addin`).
pub const RESOURCES_DIR: &str = "office_addin";

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Certificate, settings, models, logs. `LATEXSNIPPER_OFFICE_STATE` overrides (tests).
pub fn state_dir(home: &Path) -> PathBuf {
    if let Some(dir) = std::env::var_os("LATEXSNIPPER_OFFICE_STATE") {
        return PathBuf::from(dir);
    }
    home.join("Library/Application Support").join(APP_NAME)
}

/// Models the add-in downloads itself.
pub fn own_models_dir(state: &Path) -> PathBuf {
    state.join("MathCraft/models")
}

/// Models already downloaded by the LaTeXSnipper desktop app (Lite or the
/// original), read-only for the add-in.
pub fn desktop_models_dir(home: &Path) -> PathBuf {
    home.join("Library/Application Support/LaTeXSnipper/MathCraft/models")
}

/// Served files: `LATEXSNIPPER_OFFICE_RESOURCES`, the app bundle, or the
/// development build in `office_plugin/build`.
pub fn resources_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("LATEXSNIPPER_OFFICE_RESOURCES") {
        return PathBuf::from(dir);
    }
    if let Some(bundled) = std::env::current_exe()
        .ok()
        .and_then(|exe| {
            exe.parent()
                .map(|d| d.join("../Resources").join(RESOURCES_DIR))
        })
        .filter(|p| p.is_dir())
    {
        return bundled;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../build")
        .join(RESOURCES_DIR)
}

/// Create `dir` readable only by the current user.
pub fn ensure_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Write `bytes` to `path` through a temporary file in the same directory.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
        std::process::id()
    ));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

pub fn log(message: impl std::fmt::Display) {
    let now = time::OffsetDateTime::now_utc();
    eprintln!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z {message}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    );
}

/// Random lowercase hex, from the system's CSPRNG.
pub fn random_hex(bytes: usize) -> String {
    use std::io::Read;
    let mut buf = vec![0u8; bytes];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .expect("/dev/urandom is readable");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}
