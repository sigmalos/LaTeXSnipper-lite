//! The host over real HTTPS with the assembled add-in files
//! (`npm run assemble` in office_plugin first). Screen capture, AppleScript
//! and file panels are replaced by fakes; fonts, OMML (when Word is
//! installed) and TLS are real.

use office_host::api::{ApiError, Assets, Host};
use office_host::cert::LocalCertificate;
use office_host::outlines::Outlines;
use office_host::recognition::{Captured, Capturer, Jobs, Recognizer};
use office_host::system::{FileChooser, Output, ScriptRunner};
use office_host::typography::SettingsStore;
use office_host::{http, omml};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Capture;
impl Capturer for Capture {
    fn capture(&self, _: &AtomicBool, _: Instant) -> anyhow::Result<Captured> {
        Ok(Captured::Image(b"png".to_vec()))
    }
}

struct Recognize;
impl Recognizer for Recognize {
    fn ready(&self) -> bool {
        true
    }
    fn recognize(&self, _: &[u8]) -> anyhow::Result<String> {
        Ok("\\frac{a}{b}".into())
    }
}

#[derive(Default)]
struct Scripts(Mutex<Vec<Vec<String>>>);
struct SharedScripts(Arc<Scripts>);
impl ScriptRunner for SharedScripts {
    fn run(&self, _: &str, args: &[String], _: Option<Duration>) -> std::io::Result<Output> {
        self.0 .0.lock().unwrap().push(args.to_vec());
        Ok(Output {
            status: Some(0),
            stdout: "ok".into(),
            stderr: String::new(),
        })
    }
}

struct Files(PathBuf);
impl FileChooser for Files {
    fn open_json(&self, _: &str) -> Result<Option<PathBuf>, ApiError> {
        Ok(Some(self.0.join("in.json")))
    }
    fn save_json(&self, _: &str, _: &str) -> Result<Option<PathBuf>, ApiError> {
        Ok(Some(self.0.join("out.json")))
    }
}

struct Server {
    base: String,
    agent: ureq::Agent,
    scripts: Arc<Scripts>,
    dir: tempfile::TempDir,
}

fn assets_root() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../build/office_addin");
    assert!(
        root.join("web/taskpane.html").is_file(),
        "run `npm run assemble` in office_plugin first"
    );
    root
}

fn start() -> Server {
    let dir = tempfile::tempdir().unwrap();
    let assets = Assets::new(assets_root());
    let scripts = Arc::new(Scripts::default());
    let host = Host {
        settings: SettingsStore::new(
            dir.path().join("settings.json"),
            assets.symbol_fonts().unwrap(),
        ),
        mathjax_version: assets.mathjax_version().unwrap(),
        outlines: Outlines::system(),
        recognition: Jobs::new(Box::new(Capture), Box::new(Recognize)),
        scripts: Box::new(SharedScripts(scripts.clone())),
        files: Box::new(Files(dir.path().to_path_buf())),
        stylesheet: Box::new(|| {
            omml::find_stylesheet(&omml::application_dirs(&office_host::paths::home()))
        }),
        assets,
    };
    let identity = LocalCertificate::in_dir(&dir.path().join("cert"));
    identity.generate().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    http::spawn(
        listener,
        identity.tls_config().unwrap(),
        port,
        Arc::new(host),
    );

    let mut roots = rustls::RootCertStore::empty();
    roots.add(identity.der().unwrap()).unwrap();
    let client = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    assert!(http::health(identity.der().unwrap(), port).unwrap());
    Server {
        base: format!("https://localhost:{port}"),
        agent: ureq::AgentBuilder::new()
            .tls_config(Arc::new(client))
            .build(),
        scripts,
        dir,
    }
}

impl Server {
    fn call(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let req = self
            .agent
            .request(method, &format!("{}{path}", self.base))
            .set("X-LaTeXSnipper-Office", "1");
        let res = match body {
            Some(b) => req.send_string(&b.to_string()),
            None => req.call(),
        };
        let res = match res {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => panic!("{e}"),
        };
        let status = res.status();
        (
            status,
            serde_json::from_str(&res.into_string().unwrap()).unwrap_or(Value::Null),
        )
    }

    fn get(&self, path: &str) -> ureq::Response {
        match self.agent.get(&format!("{}{path}", self.base)).call() {
            Ok(r) | Err(ureq::Error::Status(_, r)) => r,
            Err(e) => panic!("{e}"),
        }
    }
}

#[test]
fn serves_the_add_in_files_and_nothing_else() {
    let s = start();
    for (path, kind) in [
        ("/addin/taskpane.html", "text/html"),
        ("/addin/app/controller.mjs", "text/javascript"),
        ("/addin/icons/app-80.png", "image/png"),
        ("/shared/editor.js", "text/javascript"),
        (
            "/shared/vendor/fonts/KaTeX_Main-Regular.woff2",
            "font/woff2",
        ),
        ("/mathjax/startup.js", "text/javascript"),
        ("/mathjax/fonts/mathjax-tex-font/svg.js", "text/javascript"),
        ("/renderer/typography-adapter.js", "text/javascript"),
        ("/support/alipay.jpg", "image/jpeg"),
    ] {
        let res = s.get(path);
        assert_eq!(res.status(), 200, "{path}");
        assert!(
            res.content_type()
                .starts_with(kind.split(';').next().unwrap()),
            "{path}: {}",
            res.content_type()
        );
        assert_eq!(res.header("cache-control"), Some("no-cache"));
    }
    for path in [
        "/",
        "/manifest.xml",
        "/addin/../manifest.xml",
        "/addin/%2e%2e/manifest.xml",
        "/mathjax/missing.js",
        "/settings.json",
    ] {
        assert_eq!(s.get(path).status(), 404, "{path}");
    }
    let head = s
        .agent
        .head(&format!("{}/addin/taskpane.html", s.base))
        .call()
        .unwrap();
    assert_eq!(head.status(), 200);
}

#[test]
fn api_requires_the_marker_header_same_origin_and_a_local_host() {
    let s = start();
    let bare = s
        .agent
        .get(&format!("{}/api/catalog", s.base))
        .call()
        .unwrap_err();
    assert!(matches!(bare, ureq::Error::Status(403, _)));
    let foreign = s
        .agent
        .get(&format!("{}/api/catalog", s.base))
        .set("X-LaTeXSnipper-Office", "1")
        .set("Origin", "https://evil.example")
        .call()
        .unwrap_err();
    let ureq::Error::Status(403, r) = foreign else {
        panic!()
    };
    assert_eq!(
        serde_json::from_str::<Value>(&r.into_string().unwrap()).unwrap()["error"]["code"],
        "forbidden"
    );
    let options = s
        .agent
        .request("OPTIONS", &format!("{}/api/catalog", s.base))
        .call()
        .unwrap_err();
    assert!(matches!(options, ureq::Error::Status(403, _)));
    let same = s
        .agent
        .get(&format!("{}/api/health", s.base))
        .set("X-LaTeXSnipper-Office", "1")
        .set("Origin", &s.base)
        .call()
        .unwrap();
    assert_eq!(same.status(), 200);
    let misdirected = s
        .agent
        .get(&format!("{}/addin/taskpane.html", s.base))
        .set("Host", "evil.example")
        .call()
        .unwrap_err();
    assert!(matches!(misdirected, ureq::Error::Status(421, _)));
    assert_eq!(s.call("GET", "/api/nope", None).0, 404);
    assert_eq!(s.call("POST", "/addin/taskpane.html", None).0, 405);
    let (status, body) = s.call("POST", "/api/omml", Some(json!(["not", "an", "object"])));
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (400, Some("invalid_request"))
    );
}

#[test]
fn catalog_and_settings() {
    let s = start();
    let (status, catalog) = s.call("GET", "/api/catalog", None);
    assert_eq!(status, 200);
    assert_eq!(
        catalog["symbolFonts"],
        json!(["mathjax-tex", "mathjax-stix2"])
    );
    assert_eq!(catalog["mathjaxVersion"], "4.1.3");
    assert!(catalog["systemFonts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f == "Times New Roman"));
    assert_eq!(
        catalog["options"]["powerpoint"]["insertionBackend"],
        json!(["Png", "Svg"])
    );

    let (_, mut word) = s.call("GET", "/api/settings/word", None);
    assert_eq!(word["numberPlacement"], "Right");
    word["numberPlacement"] = json!("Left");
    word["formulaColor"] = json!("#336699");
    word["formulaFontSizePoints"] = json!(14);
    let (status, saved) = s.call("PUT", "/api/settings/word", Some(word));
    assert_eq!(status, 200);
    assert_eq!(
        (
            saved["numberPlacement"].as_str(),
            saved["formulaColor"].as_str()
        ),
        (Some("Left"), Some("#336699"))
    );
    assert_eq!(saved["typography"]["fontSizePoints"], 14.0);
    let (status, bad) = s.call(
        "PUT",
        "/api/settings/word",
        Some(json!({"formulaColor": "red"})),
    );
    assert_eq!(
        (status, bad["error"]["code"].as_str()),
        (422, Some("invalid_settings"))
    );
    assert_eq!(s.call("GET", "/api/settings/excel", None).0, 404);

    let (status, exported) = s.call("POST", "/api/settings/word/export", Some(json!({})));
    assert_eq!((status, exported["kind"].as_str()), (200, Some("export")));
    let preset = std::fs::read_to_string(s.dir.path().join("out.json")).unwrap();
    std::fs::write(s.dir.path().join("in.json"), preset).unwrap();
    let (status, imported) = s.call("POST", "/api/settings/powerpoint/import", Some(json!({})));
    assert_eq!(status, 200);
    assert_eq!(imported["settings"]["formulaColor"], "#336699");
    assert_eq!(
        imported["settings"]["insertionBackend"], "Png",
        "options stay per host"
    );
}

#[test]
fn font_outlines_and_omml() {
    let s = start();
    let (status, out) = s.call(
        "POST",
        "/api/font-outlines",
        Some(
            json!({"runs": [{"key": "k1", "text": "12", "family": "Times New Roman", "style": 0}]}),
        ),
    );
    assert_eq!(status, 200);
    assert_eq!(out["outlines"]["k1"]["ActualFamily"], "Times New Roman");
    let (status, bad) = s.call("POST", "/api/font-outlines", Some(json!({"runs": "x"})));
    assert_eq!(
        (status, bad["error"]["message"].as_str()),
        (400, Some("字段无效：runs"))
    );

    let typography = json!({"typographyVersion": 1, "symbolFontId": "mathjax-tex", "numberFontFamily": "", "cjkFontFamily": "PingFang SC", "defaultMathStyle": "Automatic", "fontSizePoints": 12, "color": "#000000"});
    let mathml = "<math xmlns='http://www.w3.org/1998/Math/MathML'><mfrac><mi>a</mi><mi>b</mi></mfrac></math>";
    let (status, out) = s.call(
        "POST",
        "/api/omml",
        Some(json!({"mathml": mathml, "typography": typography})),
    );
    if omml::find_stylesheet(&omml::application_dirs(&office_host::paths::home())).is_some() {
        assert_eq!(status, 200, "{out}");
        assert!(out["omml"].as_str().unwrap().contains("<m:f>"));
    } else {
        assert_eq!(
            (status, out["error"]["code"].as_str()),
            (500, Some("omml_unavailable"))
        );
    }
    let (status, out) = s.call(
        "POST",
        "/api/omml",
        Some(json!({"mathml": mathml, "typography": {"typographyVersion": 1}})),
    );
    assert_eq!(
        (status, out["error"]["code"].as_str()),
        (422, Some("invalid_typography"))
    );
}

#[test]
fn recognition_jobs() {
    let s = start();
    assert_eq!(
        s.call("GET", "/api/recognition/config", None).1["engine"],
        "mathcraft"
    );
    let (status, created) = s.call(
        "POST",
        "/api/recognition/next",
        Some(json!({"timeout": 120})),
    );
    assert_eq!(status, 200);
    let id = created["job"]["id"].as_str().unwrap().to_string();
    let mut job = Value::Null;
    for _ in 0..200 {
        job = s
            .call("GET", &format!("/api/recognition/jobs/{id}"), None)
            .1;
        if job["job"]["state"] == "completed" {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(job["job"]["items"][0]["text"], "\\frac{a}{b}");
    for bad in [
        json!({"timeout": 0}),
        json!({"timeout": 601}),
        json!({"timeout": true}),
        json!({"timeout": "5"}),
    ] {
        assert_eq!(
            s.call("POST", "/api/recognition/next", Some(bad.clone())).0,
            400,
            "{bad}"
        );
    }
    assert_eq!(s.call("GET", "/api/recognition/jobs/unknown", None).0, 404);
    assert_eq!(s.call("GET", "/api/recognition/jobs/bad%20id", None).0, 404);
}

#[test]
fn word_variables() {
    let s = start();
    let name = format!("LS.E.{}.0123456789", "c".repeat(32));
    let (status, out) = s.call(
        "POST",
        "/api/word/variables",
        Some(json!({"marker": "_LSLink0123456789abcdef", "values": {name.clone(): "{}"}})),
    );
    assert_eq!((status, out["written"].as_i64()), (200, Some(1)));
    assert_eq!(
        s.scripts.0.lock().unwrap()[0],
        [
            "_LSLink0123456789abcdef".to_string(),
            name,
            "{}".to_string()
        ]
    );
    let (status, _) = s.call(
        "POST",
        "/api/word/variables",
        Some(json!({"marker": "x", "values": {}})),
    );
    assert_eq!(status, 400);
}
