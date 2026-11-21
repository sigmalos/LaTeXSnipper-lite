//! Per-user installation and the `--serve` lifecycle.
//!
//! Installing trusts the loopback certificate (macOS asks once) and registers
//! a LaunchAgent that serves Word and PowerPoint from login, independently of
//! the LaTeXSnipper desktop app. The running host keeps the manifest in each
//! Office app's sideload folder. Uninstalling removes everything the add-in
//! created: the agent, its manifests, the certificate trust and its state.

use crate::api::{Assets, Host};
use crate::cert::{self, LocalCertificate, Prepared};
use crate::outlines::Outlines;
use crate::paths::{self, AGENT_LABEL, PORT};
use crate::recognition::{self, Jobs, MathCraft, ScreenCapture};
use crate::system::{Osascript, ScriptFileChooser};
use crate::typography::SettingsStore;
use crate::{http, omml};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

pub const MANIFEST_FILE: &str = "latexsnipper-office-addin.xml";
pub const OFFICE_HOSTS: [(&str, &str, &str); 2] = [
    ("word", "com.microsoft.Word", "Microsoft Word.app"),
    (
        "powerpoint",
        "com.microsoft.Powerpoint",
        "Microsoft PowerPoint.app",
    ),
];
const LAUNCHCTL: &str = "/bin/launchctl";
const SIDELOAD_INTERVAL: Duration = Duration::from_secs(60);

pub struct Env {
    pub home: PathBuf,
    pub state: PathBuf,
    pub resources: PathBuf,
    /// The host executable launchd runs.
    pub program: PathBuf,
}

impl Env {
    pub fn current() -> Result<Self> {
        let home = paths::home();
        Ok(Self {
            state: paths::state_dir(&home),
            resources: paths::resources_root(),
            program: std::env::current_exe().context("cannot locate the host executable")?,
            home,
        })
    }

    fn certificate(&self) -> LocalCertificate {
        LocalCertificate::in_dir(&self.state)
    }
}

// ------------------------------------------------------------------ manifest

pub fn container(home: &Path, bundle: &str) -> PathBuf {
    home.join("Library/Containers").join(bundle)
}

pub fn wef_dir(home: &Path, bundle: &str) -> PathBuf {
    container(home, bundle).join("Data/Documents/wef")
}

pub fn manifest_version(version: &str) -> Result<String> {
    let parts: Vec<&str> = version
        .trim()
        .split('.')
        .filter(|p| !p.is_empty())
        .collect();
    if parts.is_empty()
        || parts.len() > 4
        || !parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit()))
    {
        bail!("无效的产品版本：{version}");
    }
    let mut parts: Vec<String> = parts.into_iter().map(String::from).collect();
    parts.resize(4, "0".into());
    Ok(parts.join("."))
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Fill in the template. Icon URLs get a hash of the icon file: Office for
/// Mac caches ribbon icons by URL, so a changed icon needs a new URL.
pub fn render_manifest(
    template: &str,
    base_url: &str,
    version: &str,
    icons: &Path,
) -> Result<String> {
    if !template.contains("{{BASE_URL}}") || !template.contains("{{VERSION}}") {
        bail!("Office 加载项清单模板无效。");
    }
    const ICON: &str = "{{BASE_URL}}/addin/icons/";
    let mut out = String::with_capacity(template.len() + 1024);
    let mut rest = template;
    while let Some(at) = rest.find(ICON) {
        let after = &rest[at + ICON.len()..];
        let name_len = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-'))
            .unwrap_or(after.len());
        let name = &after[..name_len];
        out.push_str(&rest[..at + ICON.len() + name_len]);
        if name.ends_with(".png") {
            let bytes =
                std::fs::read(icons.join(name)).with_context(|| format!("missing icon {name}"))?;
            let digest: String = Sha256::digest(bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            out.push_str("?v=");
            out.push_str(&digest[..12]);
        }
        rest = &after[name_len..];
    }
    out.push_str(rest);
    Ok(out
        .replace("{{BASE_URL}}", &escape_xml(base_url))
        .replace("{{VERSION}}", &manifest_version(version)?))
}

/// Office apps that have created their sandbox container (been opened) at least once.
pub fn available_hosts(home: &Path) -> Vec<&'static str> {
    OFFICE_HOSTS
        .iter()
        .filter(|(_, bundle, _)| container(home, bundle).is_dir())
        .map(|(id, _, _)| *id)
        .collect()
}

/// Write the manifest for each available host (only when it changed). The
/// `wef` folder belongs to Office: only our own file is ever written.
pub fn sideload(manifest: &str, home: &Path) -> Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    for (id, bundle, _) in OFFICE_HOSTS {
        if !available_hosts(home).contains(&id) {
            continue;
        }
        let target = wef_dir(home, bundle).join(MANIFEST_FILE);
        if std::fs::read_to_string(&target).is_ok_and(|current| current == manifest) {
            continue;
        }
        paths::write_atomic(&target, manifest.as_bytes())?;
        written.push(target);
    }
    Ok(written)
}

pub fn remove_manifests(home: &Path) -> Vec<PathBuf> {
    OFFICE_HOSTS
        .iter()
        .map(|(_, bundle, _)| wef_dir(home, bundle).join(MANIFEST_FILE))
        .filter(|p| p.is_file() && std::fs::remove_file(p).is_ok())
        .collect()
}

// ------------------------------------------------------------------ launch agent

pub fn agent_path(home: &Path) -> PathBuf {
    home.join("Library/LaunchAgents")
        .join(format!("{AGENT_LABEL}.plist"))
}

pub fn agent_plist(program: &Path, log: &Path) -> String {
    let s = |p: &Path| escape_xml(&p.to_string_lossy());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{AGENT_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{program}</string>
    <string>--serve</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <dict>
    <key>SuccessfulExit</key>
    <false/>
  </dict>
  <key>ProcessType</key>
  <string>Interactive</string>
  <key>StandardOutPath</key>
  <string>{log}</string>
  <key>StandardErrorPath</key>
  <string>{log}</string>
  <key>LimitLoadToSessionType</key>
  <string>Aqua</string>
</dict>
</plist>
"#,
        program = s(program),
        log = s(log),
    )
}

fn domain() -> String {
    format!("gui/{}", unsafe { libc::getuid() })
}

fn launchctl(args: &[&str]) -> std::io::Result<std::process::Output> {
    Command::new(LAUNCHCTL).args(args).output()
}

pub fn agent_running() -> bool {
    launchctl(&["print", &format!("{}/{AGENT_LABEL}", domain())]).is_ok_and(|o| o.status.success())
}

fn stop_agent() {
    if agent_running() {
        let _ = launchctl(&["bootout", &format!("{}/{AGENT_LABEL}", domain())]);
        // bootout returns before the process has fully exited and released the port.
        for _ in 0..40 {
            if !agent_running() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// A host inside a mounted DMG or a translocated app would stop working
/// once the image is ejected or the app moved.
pub fn location_problem(program: &Path) -> Option<&'static str> {
    let p = program.to_string_lossy();
    if p.starts_with("/Volumes/") {
        Some("LOCATION_DMG")
    } else if p.contains("/AppTranslocation/") {
        Some("LOCATION_TRANSLOCATED")
    } else {
        None
    }
}

fn reachable(identity: &LocalCertificate) -> bool {
    identity
        .der()
        .is_ok_and(|der| http::health(der, PORT).unwrap_or(false))
}

pub fn install(env: &Env) -> Result<Value> {
    if let Some(code) = location_problem(&env.program) {
        bail!("{code}:{}", env.program.display());
    }
    Assets::new(env.resources.clone()).validate()?;
    paths::ensure_private_dir(&env.state)?;
    let identity = match cert::prepare(&env.state, true)? {
        Prepared::Ready(identity) => identity,
        Prepared::NeedsTrust => bail!("TRUST_DENIED:证书未受信任。"),
    };
    stop_agent();
    // Another program on the port would leave the agent failing silently.
    drop(http::bind(PORT)?);
    let plist = agent_path(&env.home);
    paths::write_atomic(
        &plist,
        agent_plist(&env.program, &env.state.join("host.log")).as_bytes(),
    )?;
    let out = launchctl(&["bootstrap", &domain(), &plist.to_string_lossy()])?;
    if !out.status.success() {
        bail!(
            "AGENT_FAILED:{}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    for _ in 0..50 {
        if reachable(&identity) {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(status(env))
}

pub fn uninstall(env: &Env) -> Result<Value> {
    stop_agent();
    let plist = agent_path(&env.home);
    if plist.is_file() {
        std::fs::remove_file(&plist)?;
    }
    remove_manifests(&env.home);
    env.certificate().untrust();
    if env.state.is_dir() {
        std::fs::remove_dir_all(&env.state)?;
    }
    Ok(status(env))
}

pub fn status(env: &Env) -> Value {
    let identity = env.certificate();
    let hosts: Vec<Value> = OFFICE_HOSTS
        .iter()
        .map(|(id, bundle, app)| {
            let installed_app = omml::application_dirs(&env.home)
                .iter()
                .any(|d| d.join(app).is_dir());
            json!({
                "id": id,
                "app": installed_app,
                "ready": container(&env.home, bundle).is_dir(),
                "sideloaded": wef_dir(&env.home, bundle).join(MANIFEST_FILE).is_file(),
            })
        })
        .collect();
    let model = recognition::model_root(&env.home, &env.state).map(|(source, _)| source);
    let download_bytes = mathcraft::modelhub::CATALOG
        .iter()
        .find(|(id, _)| *id == recognition::FORMULA_MODEL)
        .map_or(0, |(_, bytes)| *bytes);
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "installed": agent_path(&env.home).is_file(),
        "running": agent_running(),
        "reachable": identity.exists() && reachable(&identity),
        "certificate": {"exists": identity.exists(), "trusted": identity.is_trusted()},
        "hosts": hosts,
        "model": {"source": model, "downloadBytes": download_bytes},
        "location": location_problem(&env.program),
        "url": format!("https://localhost:{PORT}"),
    })
}

// ------------------------------------------------------------------ self test

/// Serve the bundled files with a throwaway certificate on a free port and
/// request them, without installing anything (`--self-test`, used after
/// building the app). Settings go to a temporary directory.
pub fn self_test(env: &Env) -> Result<Value> {
    let temp = std::env::temp_dir().join(format!(
        "latexsnipper-office-selftest-{}",
        paths::random_hex(6)
    ));
    let env = Env {
        home: env.home.clone(),
        state: temp.clone(),
        resources: env.resources.clone(),
        program: env.program.clone(),
    };
    let result = (|| -> Result<Value> {
        let host = Arc::new(build_host(&env)?);
        let identity = LocalCertificate::in_dir(&temp.join("cert"));
        identity.generate()?;
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        http::spawn(listener, identity.tls_config()?, port, host);
        let mut checks = serde_json::Map::new();
        for path in [
            "/api/health",
            "/api/catalog",
            "/addin/taskpane.html",
            "/shared/editor.js",
            "/mathjax/startup.js",
            "/renderer/typography-adapter.js",
        ] {
            let (status, _) = http::get(identity.der()?, port, path)?;
            checks.insert(path.into(), json!(status));
        }
        let ok = checks.values().all(|s| s == 200);
        Ok(json!({"ok": ok, "resources": env.resources, "checks": checks}))
    })();
    let _ = std::fs::remove_dir_all(&temp);
    let value = result?;
    if value["ok"] != true {
        bail!("SELF_TEST_FAILED:{value}");
    }
    Ok(value)
}

// ------------------------------------------------------------------ serve

pub fn build_host(env: &Env) -> Result<Host> {
    let assets = Assets::new(env.resources.clone());
    assets.validate()?;
    let settings = SettingsStore::new(env.state.join("settings.json"), assets.symbol_fonts()?);
    let home = env.home.clone();
    Ok(Host {
        mathjax_version: assets.mathjax_version()?,
        settings,
        outlines: Outlines::system(),
        recognition: Jobs::new(
            Box::new(ScreenCapture),
            Box::new(MathCraft::new(env.home.clone(), env.state.clone())),
        ),
        scripts: Box::new(Osascript),
        files: Box::new(ScriptFileChooser(Osascript)),
        stylesheet: Box::new(move || omml::find_stylesheet(&omml::application_dirs(&home))),
        assets,
    })
}

/// What launchd runs. Returns `Ok(false)` when the certificate needs the
/// user (Setup must run again); the process then exits cleanly, so launchd
/// does not restart it into the same state.
pub fn serve(env: &Env) -> Result<bool> {
    let host = Arc::new(build_host(env)?);
    let identity = match cert::prepare(&env.state, false)? {
        Prepared::Ready(identity) => identity,
        Prepared::NeedsTrust => {
            paths::log("the add-in certificate needs to be trusted again; open LaTeXSnipper Office and click Install");
            return Ok(false);
        }
    };
    let listener = http::bind(PORT)?;
    http::spawn(listener, identity.tls_config()?, PORT, host.clone());
    let template = std::fs::read_to_string(host.assets.manifest_template())?;
    let manifest = render_manifest(
        &template,
        &format!("https://localhost:{PORT}"),
        env!("CARGO_PKG_VERSION"),
        &host.assets.web().join("icons"),
    )?;
    paths::log(format!("serving https://localhost:{PORT}"));
    // Office apps opened for the first time after login get the manifest within a minute.
    loop {
        match sideload(&manifest, &env.home) {
            Ok(written) => written
                .iter()
                .for_each(|p| paths::log(format!("sideloaded {}", p.display()))),
            Err(e) => paths::log(format!("cannot write the add-in manifest: {e:#}")),
        }
        std::thread::sleep(SIDELOAD_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPLATE: &str = r#"<OfficeApp><Version>{{VERSION}}</Version><IconUrl DefaultValue="{{BASE_URL}}/addin/icons/app-32.png"/><SourceLocation DefaultValue="{{BASE_URL}}/addin/taskpane.html"/></OfficeApp>"#;

    #[test]
    fn manifest_versions_have_four_numeric_parts() {
        assert_eq!(manifest_version("1.0.0").unwrap(), "1.0.0.0");
        assert_eq!(manifest_version("3.3").unwrap(), "3.3.0.0");
        for bad in ["", "1.0.0-beta", "1.2.3.4.5", "v1"] {
            assert!(manifest_version(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn renders_icon_urls_with_content_hashes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("app-32.png"), b"png").unwrap();
        let out =
            render_manifest(TEMPLATE, "https://localhost:28766", "1.2.3", dir.path()).unwrap();
        let hash: String = Sha256::digest(b"png")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert!(out.contains(&format!(
            "https://localhost:28766/addin/icons/app-32.png?v={}",
            &hash[..12]
        )));
        assert!(out.contains("<Version>1.2.3.0</Version>"));
        assert!(out.contains("https://localhost:28766/addin/taskpane.html\""));
        assert!(render_manifest("<x/>", "u", "1", dir.path()).is_err());
        assert!(
            render_manifest(&TEMPLATE.replace("app-32", "missing"), "u", "1", dir.path()).is_err()
        );
    }

    /// The real template and icons render to well-formed XML.
    #[test]
    fn renders_the_packaged_template() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let template = std::fs::read_to_string(root.join("manifest.xml")).unwrap();
        let out = render_manifest(
            &template,
            "https://localhost:28766",
            env!("CARGO_PKG_VERSION"),
            &root.join("web/icons"),
        )
        .unwrap();
        crate::xml::parse(&out).unwrap();
        assert!(!out.contains("{{"));
        assert!(
            out.contains("<Id>5864b814-d492-4f4c-812c-b14a147c05c1</Id>"),
            "same add-in id as the original"
        );
    }

    #[test]
    fn sideloads_only_into_opened_office_apps_and_removes_only_its_file() {
        let home = tempfile::tempdir().unwrap();
        assert!(sideload("m1", home.path()).unwrap().is_empty());
        let wef = wef_dir(home.path(), "com.microsoft.Word");
        std::fs::create_dir_all(container(home.path(), "com.microsoft.Word")).unwrap();
        assert_eq!(available_hosts(home.path()), ["word"]);
        assert_eq!(
            sideload("m1", home.path()).unwrap(),
            [wef.join(MANIFEST_FILE)]
        );
        assert!(
            sideload("m1", home.path()).unwrap().is_empty(),
            "unchanged manifests are not rewritten"
        );
        assert_eq!(sideload("m2", home.path()).unwrap().len(), 1);
        assert!(!wef_dir(home.path(), "com.microsoft.Powerpoint").exists());
        std::fs::write(wef.join("other.xml"), "x").unwrap();
        assert_eq!(remove_manifests(home.path()).len(), 1);
        assert!(wef.join("other.xml").is_file());
    }

    #[test]
    fn launch_agent_plist_is_valid() {
        let dir = tempfile::tempdir().unwrap();
        let plist = dir.path().join("agent.plist");
        std::fs::write(
            &plist,
            agent_plist(
                Path::new(
                    "/Applications/LaTeXSnipper Office.app/Contents/MacOS/latexsnipper-office-host",
                ),
                Path::new("/tmp/a&b.log"),
            ),
        )
        .unwrap();
        let lint = Command::new("/usr/bin/plutil")
            .arg("-lint")
            .arg(&plist)
            .output()
            .unwrap();
        assert!(
            lint.status.success(),
            "{}",
            String::from_utf8_lossy(&lint.stdout)
        );
        let json = Command::new("/usr/bin/plutil")
            .args(["-convert", "json", "-o", "-"])
            .arg(&plist)
            .output()
            .unwrap();
        let v: Value = serde_json::from_slice(&json.stdout).unwrap();
        assert_eq!(v["Label"], AGENT_LABEL);
        assert_eq!(v["ProgramArguments"][1], "--serve");
        assert_eq!(v["KeepAlive"]["SuccessfulExit"], false);
        assert_eq!(v["StandardErrorPath"], "/tmp/a&b.log");
    }

    #[test]
    fn refuses_to_install_from_a_disk_image() {
        assert_eq!(
            location_problem(Path::new(
                "/Volumes/LaTeXSnipper Office/x.app/Contents/MacOS/h"
            )),
            Some("LOCATION_DMG")
        );
        assert_eq!(
            location_problem(Path::new(
                "/private/var/folders/x/AppTranslocation/1/d/x.app/h"
            )),
            Some("LOCATION_TRANSLOCATED")
        );
        assert_eq!(
            location_problem(Path::new(
                "/Applications/LaTeXSnipper Office.app/Contents/MacOS/h"
            )),
            None
        );
    }
}
