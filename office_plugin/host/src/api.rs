//! The `/api` routes the add-in pages call, and the files they load. The
//! routes, payloads and error codes are those of the original host
//! (`server.py`, `services.py`), so the pages run unchanged.

use crate::omml::{self, OmmlError};
use crate::outlines::Outlines;
use crate::recognition::Jobs;
use crate::system::{self, FileChooser, ScriptRunner};
use crate::typography::{self, SettingsStore, Typography};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct ApiError {
    pub status: u16,
    pub code: String,
    pub message: String,
}

impl ApiError {
    pub fn new(status: u16, code: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn body(&self) -> Value {
        json!({"error": {"code": self.code, "message": self.message}})
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}: {}", self.status, self.code, self.message)
    }
}

// ------------------------------------------------------------------ assets

/// The packaged add-in files (see `tools/assemble.mjs`).
#[derive(Clone, Debug)]
pub struct Assets {
    pub root: PathBuf,
}

impl Assets {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn web(&self) -> PathBuf {
        self.root.join("web")
    }

    pub fn mathjax(&self) -> PathBuf {
        self.root.join("MathJax")
    }

    pub fn manifest_template(&self) -> PathBuf {
        self.root.join("manifest.xml")
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        let required = [
            self.web().join("taskpane.html"),
            self.root.join("shared/editor.js"),
            self.root.join("MathJaxTypographyAdapter.js"),
            self.manifest_template(),
            self.mathjax().join("startup.js"),
        ];
        let missing: Vec<String> = required
            .iter()
            .filter(|p| !p.is_file())
            .map(|p| p.display().to_string())
            .collect();
        if !missing.is_empty() {
            anyhow::bail!("Office 加载项资源缺失：{}", missing.join(", "));
        }
        Ok(())
    }

    /// The renderer's symbol-font catalog, read from the packaged MathJax config.
    pub fn symbol_fonts(&self) -> anyhow::Result<Vec<String>> {
        let config = std::fs::read_to_string(self.mathjax().join("config.js"))?;
        let (start, end) = (config.find('{'), config.rfind('}'));
        let (Some(start), Some(end)) = (start, end) else {
            anyhow::bail!("Invalid MathJax font configuration.");
        };
        let parsed: Value = serde_json::from_str(&config[start..=end])?;
        let fonts: Vec<String> = parsed["fonts"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|f| f.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        if fonts.is_empty() {
            anyhow::bail!("MathJax font catalog is empty.");
        }
        Ok(fonts)
    }

    pub fn mathjax_version(&self) -> anyhow::Result<String> {
        let data: Value = serde_json::from_str(&std::fs::read_to_string(
            self.mathjax().join("resources.json"),
        )?)?;
        let version = data["version"].as_str().unwrap_or_default().to_string();
        let ok = (1..=32).contains(&version.len())
            && version
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b));
        if !ok {
            anyhow::bail!("Invalid MathJax version.");
        }
        Ok(version)
    }

    /// The file for a static URL path, confined to its root.
    pub fn static_target(&self, path: &str) -> Option<PathBuf> {
        if path == "/renderer/typography-adapter.js" {
            return Some(self.root.join("MathJaxTypographyAdapter.js"));
        }
        let roots = [
            ("/addin/", "web"),
            ("/shared/", "shared"),
            ("/mathjax/", "MathJax"),
            ("/support/", "support"),
        ];
        let (prefix, dir) = roots.iter().find(|(p, _)| path.starts_with(p))?;
        let relative = percent_decode(&path[prefix.len()..])?;
        let parts: Vec<&str> = relative.split('/').filter(|p| !p.is_empty()).collect();
        if parts.is_empty()
            || parts
                .iter()
                .any(|p| *p == "." || *p == ".." || p.contains('\\') || p.contains('\0'))
        {
            return None;
        }
        let mut target = self.root.join(dir);
        for p in parts {
            target.push(p);
        }
        // No symlink may lead out of the root.
        let base = self.root.join(dir).canonicalize().ok()?;
        let resolved = target.canonicalize().ok()?;
        resolved.starts_with(&base).then_some(resolved)
    }
}

pub fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

pub fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("xml") => "application/xml; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

// ------------------------------------------------------------------ services

pub struct Host {
    pub assets: Assets,
    pub settings: SettingsStore,
    pub outlines: Outlines,
    pub recognition: Arc<Jobs>,
    pub scripts: Box<dyn ScriptRunner>,
    pub files: Box<dyn FileChooser>,
    /// Word's `mathml2omml.xsl`, looked up per request (Word may be installed later).
    pub stylesheet: Box<dyn Fn() -> Option<PathBuf> + Send + Sync>,
    pub mathjax_version: String,
}

pub type Body = Result<Value, ApiError>;

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::new(400, "invalid_request", message)
}

fn require_string<'a>(body: &'a Value, key: &str) -> Result<&'a str, ApiError> {
    body.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| invalid(format!("缺少字段：{key}")))
}

fn timeout(body: &Value) -> Result<Duration, ApiError> {
    let value = body.get("timeout").cloned().unwrap_or(json!(120));
    match value.as_u64().filter(|_| value.is_u64() || value.is_i64()) {
        Some(t @ 1..=600) => Ok(Duration::from_secs(t)),
        _ => Err(invalid("等待时间无效。")),
    }
}

fn is_job_id(id: &str) -> bool {
    (1..=128).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn host_label(host: &str) -> &'static str {
    if host == "word" {
        "Word"
    } else {
        "PowerPoint"
    }
}

fn settings_error(e: typography::TypographyError) -> ApiError {
    ApiError::new(422, "invalid_settings", e.0)
}

impl Host {
    pub fn catalog(&self) -> Value {
        json!({
            "symbolFonts": self.settings.symbol_fonts,
            "systemFonts": self.outlines.families(),
            "cjkFonts": typography::cjk_font_catalog(),
            "namedSizes": typography::named_sizes(),
            "commonPointSizes": typography::COMMON_POINT_SIZES,
            "minimumPoints": typography::MINIMUM_POINTS,
            "maximumPoints": typography::MAXIMUM_POINTS,
            "mathStyles": typography::math_style_catalog(),
            "options": typography::option_catalog(),
            "mathjaxVersion": self.mathjax_version,
        })
    }

    fn load_settings(&self, host: &str) -> Result<Value, ApiError> {
        let (defaults, options) = self.settings.load(host).map_err(settings_error)?;
        Ok(typography::settings_payload(&defaults, &options))
    }

    fn omml(&self, body: &Value) -> Result<Value, ApiError> {
        let mathml = require_string(body, "mathml")?;
        let typography = Typography::from_fields(body.get("typography").unwrap_or(&Value::Null))
            .map_err(|e| ApiError::new(422, "invalid_typography", e.0))?;
        let stylesheet = (self.stylesheet)().ok_or_else(|| {
            ApiError::new(
                500,
                "omml_unavailable",
                "未找到 Microsoft Word 的 mathml2omml.xsl，无法生成 Word 原生公式。请确认 Word 已安装在“应用程序”文件夹。",
            )
        })?;
        match omml::convert(&stylesheet, mathml, &typography) {
            Ok(omml) => Ok(json!({"omml": omml})),
            Err(OmmlError::StylesheetUnavailable) => Err(ApiError::new(
                500,
                "omml_unavailable",
                "未找到 Microsoft Word 的 mathml2omml.xsl。",
            )),
            Err(OmmlError::Invalid(detail)) => {
                crate::paths::log(format!("OMML conversion failed: {detail}"));
                Err(ApiError::new(
                    422,
                    "omml_failed",
                    "MathML 无法转换为 Word 公式。",
                ))
            }
        }
    }

    fn import_settings(&self, host: &str) -> Result<Value, ApiError> {
        let Some(path) = self
            .files
            .open_json(&format!("导入 {} 公式预设", host_label(host)))?
        else {
            return Ok(json!({"canceled": true}));
        };
        let unreadable = || ApiError::new(422, "invalid_preset", "无法读取预设文件。");
        let size = std::fs::metadata(&path).map_err(|_| unreadable())?.len();
        if size > typography::MAX_PRESET_BYTES as u64 {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            return Err(ApiError::new(
                422,
                "invalid_preset",
                format!("预设文件过大：{name}"),
            ));
        }
        let raw = std::fs::read(&path).map_err(|_| unreadable())?;
        self.settings
            .import_preset(host, &raw)
            .map_err(|e| ApiError::new(422, "invalid_preset", e.0))?;
        Ok(json!({"kind": "import", "name": stem(&path), "settings": self.load_settings(host)?}))
    }

    fn export_settings(&self, host: &str) -> Result<Value, ApiError> {
        let label = host_label(host);
        let Some(path) = self.files.save_json(
            &format!("导出 {label} 公式预设"),
            &format!("LaTeXSnipper {label} 公式.json"),
        )?
        else {
            return Ok(json!({"canceled": true}));
        };
        let text = self.settings.export_preset(host).map_err(settings_error)?;
        std::fs::write(&path, text)
            .map_err(|_| ApiError::new(500, "export_failed", "无法写入预设文件。"))?;
        Ok(json!({"kind": "export", "name": stem(&path)}))
    }

    pub fn handle(&self, method: &str, path: &str, body: Body) -> Result<Value, ApiError> {
        let body = || body.clone();
        match (method, path) {
            ("GET", "/api/health") => return Ok(json!({"ok": true})),
            ("GET", "/api/catalog") => return Ok(self.catalog()),
            ("POST", "/api/font-outlines") => {
                let body = body()?;
                let runs = body
                    .get("runs")
                    .and_then(Value::as_array)
                    .filter(|r| r.len() <= 512 && r.iter().all(Value::is_object))
                    .ok_or_else(|| invalid("字段无效：runs"))?;
                let outlines = self
                    .outlines
                    .shape_runs(runs)
                    .map_err(|e| ApiError::new(422, "font_unavailable", e.0))?;
                return Ok(json!({"outlines": outlines}));
            }
            ("POST", "/api/omml") => return self.omml(&body()?),
            ("POST", "/api/word/variables") => {
                let body = body()?;
                let null = Value::Null;
                return system::write_word_variables(
                    self.scripts.as_ref(),
                    body.get("marker").unwrap_or(&null),
                    body.get("values").unwrap_or(&null),
                );
            }
            ("GET", "/api/recognition/config") => return self.recognition.config(),
            ("POST", "/api/recognition/next") => return self.recognition.next(timeout(&body()?)?),
            _ => {}
        }
        if let Some(id) = path
            .strip_prefix("/api/recognition/jobs/")
            .filter(|id| is_job_id(id))
        {
            match method {
                "GET" => return self.recognition.job(id),
                "DELETE" => return self.recognition.cancel(id),
                _ => {}
            }
        }
        if let Some(rest) = path.strip_prefix("/api/settings/") {
            let (host, action) = rest
                .split_once('/')
                .map_or((rest, None), |(h, a)| (h, Some(a)));
            if typography::HOSTS.contains(&host) {
                match (method, action) {
                    ("GET", None) => return self.load_settings(host),
                    ("PUT", None) => {
                        let body = body()?;
                        let defaults =
                            typography::defaults_from_form(&body).map_err(settings_error)?;
                        self.settings
                            .save(host, defaults, &body)
                            .map_err(settings_error)?;
                        return self.load_settings(host);
                    }
                    ("POST", Some("import")) => return self.import_settings(host),
                    ("POST", Some("export")) => return self.export_settings(host),
                    _ => {}
                }
            }
        }
        Err(ApiError::new(404, "not_found", "接口不存在。"))
    }
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}
