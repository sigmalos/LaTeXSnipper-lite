//! Formula typography and per-host settings, ported from the original host's
//! `typography.py` and `options.py`. Field names, validation and the
//! `settings.json` schema match the Windows plugin, so presets exported on
//! one platform import on the other, and an existing settings file of the
//! original Mac add-in is read as it is.

use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::sync::Mutex;

pub const TYPOGRAPHY_VERSION: i64 = 1;
pub const SETTINGS_SCHEMA: i64 = 2;
pub const MAX_PRESET_BYTES: usize = 64 * 1024;
pub const MINIMUM_POINTS: f64 = 1.0;
pub const MAXIMUM_POINTS: f64 = 1638.0;
pub const HOSTS: [&str; 2] = ["word", "powerpoint"];

pub const MATH_STYLES: [(&str, &str, &str); 17] = [
    ("Automatic", "自动数学样式", "Automatic"),
    ("Upright", "正体", "Upright"),
    ("Bold", "粗体", "Bold"),
    ("Italic", "斜体", "Italic"),
    ("BoldItalic", "粗斜体", "Bold Italic"),
    ("SansSerif", "无衬线", "Sans Serif"),
    ("SansSerifBold", "无衬线粗体", "Sans Serif Bold"),
    ("SansSerifItalic", "无衬线斜体", "Sans Serif Italic"),
    (
        "SansSerifBoldItalic",
        "无衬线粗斜体",
        "Sans Serif Bold Italic",
    ),
    ("Monospace", "等宽", "Monospace"),
    ("Calligraphic", "花体", "Calligraphic"),
    ("BoldCalligraphic", "粗花体", "Bold Calligraphic"),
    ("Script", "手写体", "Script"),
    ("BoldScript", "粗手写体", "Bold Script"),
    ("Fraktur", "哥特体", "Fraktur"),
    ("BoldFraktur", "哥特粗体", "Bold Fraktur"),
    ("Blackboard", "双线体", "Blackboard Bold"),
];

pub const NAMED_SIZES: [(&str, f64); 16] = [
    ("初号", 42.0),
    ("小初", 36.0),
    ("一号", 26.0),
    ("小一", 24.0),
    ("二号", 22.0),
    ("小二", 18.0),
    ("三号", 16.0),
    ("小三", 15.0),
    ("四号", 14.0),
    ("小四", 12.0),
    ("五号", 10.5),
    ("小五", 9.0),
    ("六号", 7.5),
    ("小六", 6.5),
    ("七号", 5.5),
    ("八号", 5.0),
];

pub const COMMON_POINT_SIZES: [f64; 24] = [
    5.0, 5.5, 6.5, 7.5, 8.0, 9.0, 10.0, 10.5, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 18.0, 20.0, 22.0,
    24.0, 26.0, 28.0, 36.0, 42.0, 48.0, 72.0,
];

/// A fixed list in this order, never probed, like the Windows list.
pub const CJK_FONTS: [(&str, &str); 23] = [
    ("PingFang SC", "苹方-简"),
    ("Hiragino Sans GB", "冬青黑体简体中文"),
    ("Heiti SC", "黑体-简"),
    ("STHeiti", "华文黑体"),
    ("Songti SC", "宋体-简"),
    ("STSong", "华文宋体"),
    ("Kaiti SC", "楷体-简"),
    ("STKaiti", "华文楷体"),
    ("STFangsong", "华文仿宋"),
    ("Yuanti SC", "圆体-简"),
    ("Lantinghei SC", "兰亭黑-简"),
    ("Libian SC", "隶变-简"),
    ("Xingkai SC", "行楷-简"),
    ("Weibei SC", "魏碑-简"),
    ("Baoli SC", "报隶-简"),
    ("Hannotate SC", "手札体-简"),
    ("Wawati SC", "娃娃体-简"),
    ("Microsoft YaHei", "微软雅黑"),
    ("SimSun", "宋体"),
    ("SimHei", "黑体"),
    ("KaiTi", "楷体"),
    ("FangSong", "仿宋"),
    ("DengXian", "等线"),
];
pub const DEFAULT_CJK_FONT: &str = "PingFang SC";

/// Per-host choices: the first value is the default.
const CHOICES: [(&str, &str, &[&str]); 4] = [
    ("word", "numberPlacement", &["Right", "Left"]),
    (
        "word",
        "numberEnclosure",
        &["Parentheses", "SquareBrackets", "Braces", "None"],
    ),
    ("word", "numberSeparator", &["-", ".", "·", ":", "/"]),
    ("powerpoint", "insertionBackend", &["Png", "Svg"]),
];
const FLAGS: [(&str, &str); 4] = [
    ("word", "includeChapter"),
    ("word", "includeSection"),
    ("word", "hideChapterBoundary"),
    ("word", "hideSectionBoundary"),
];

#[derive(Debug, Clone, PartialEq)]
pub struct TypographyError(pub String);

impl std::fmt::Display for TypographyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TypographyError {}

type R<T> = Result<T, TypographyError>;

fn err<T>(message: impl Into<String>) -> R<T> {
    Err(TypographyError(message.into()))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Typography {
    pub symbol_font_id: String,
    pub number_font_family: Option<String>,
    pub cjk_font_family: String,
    pub default_math_style: String,
    pub font_size_points: f64,
    pub color: String,
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            symbol_font_id: "mathjax-tex".into(),
            number_font_family: None,
            cjk_font_family: DEFAULT_CJK_FONT.into(),
            default_math_style: "Automatic".into(),
            font_size_points: 12.0,
            color: "#000000".into(),
        }
    }
}

fn is_symbol_font_id(id: &str) -> bool {
    !id.is_empty()
        && id.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

fn is_color(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

fn family(value: &str) -> R<String> {
    let family = value.trim();
    if family.is_empty() {
        return err("字体家族不能为空。");
    }
    if family.chars().any(|c| (c as u32) < 32) {
        return err("字体家族包含控制字符。");
    }
    let lower = family.to_lowercase();
    let unsafe_chars = family.chars().any(|c| "<>;:/\\".contains(c));
    if family.chars().count() > 128
        || unsafe_chars
        || lower.contains("url(")
        || lower.contains("@import")
    {
        return err("预设包含无效的字体家族描述。");
    }
    Ok(family.to_string())
}

pub fn validate_points(value: &Value) -> R<f64> {
    let Some(points) = value.as_f64().filter(|_| value.is_number()) else {
        return err("公式字号无效。");
    };
    if !(MINIMUM_POINTS..=MAXIMUM_POINTS).contains(&points) {
        return err("公式字号必须在 1–1638 pt 之间。");
    }
    Ok(points)
}

/// A number, a decimal string or a Chinese size name such as `小四`.
pub fn parse_points(value: &Value) -> R<f64> {
    if let Some(text) = value.as_str() {
        let text = text.trim();
        if let Some((_, pts)) = NAMED_SIZES.iter().find(|(name, _)| *name == text) {
            return Ok(*pts);
        }
        let mut parts = text.splitn(2, '.');
        let whole = parts.next().unwrap_or_default();
        let frac = parts.next();
        let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        if !digits(whole) || frac.is_some_and(|f| !digits(f)) {
            return err("请输入有效的数字字号或中文字号名称。");
        }
        return validate_points(&json!(text.parse::<f64>().unwrap_or(0.0)));
    }
    validate_points(value)
}

/// `str(value)` of the original: strings as they are, other scalars as JSON.
fn text_of(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

impl Typography {
    pub fn new(
        symbol_font_id: &str,
        number_font_family: Option<&str>,
        cjk_font_family: &str,
        default_math_style: &str,
        font_size_points: &Value,
        color: &str,
    ) -> R<Self> {
        if !is_symbol_font_id(symbol_font_id) {
            return err("数学字体必须使用稳定的字体方案 ID。");
        }
        if !MATH_STYLES
            .iter()
            .any(|(id, _, _)| *id == default_math_style)
        {
            return err("无效的数学样式。");
        }
        let cjk = family(cjk_font_family)?;
        let number = number_font_family.map(family).transpose()?;
        let points = validate_points(font_size_points)?;
        if !is_color(color) {
            return err("颜色必须为 #RRGGBB 格式。");
        }
        Ok(Self {
            symbol_font_id: symbol_font_id.into(),
            number_font_family: number,
            cjk_font_family: cjk,
            default_math_style: default_math_style.into(),
            font_size_points: points,
            color: color.to_uppercase(),
        })
    }

    pub fn to_fields(&self) -> Value {
        json!({
            "typographyVersion": TYPOGRAPHY_VERSION,
            "symbolFontId": self.symbol_font_id,
            "numberFontFamily": self.number_font_family.clone().unwrap_or_default(),
            "cjkFontFamily": self.cjk_font_family,
            "defaultMathStyle": self.default_math_style,
            "fontSizePoints": self.font_size_points,
            "color": self.color,
        })
    }

    pub fn from_fields(fields: &Value) -> R<Self> {
        let Some(map) = fields.as_object() else {
            return err("预设缺少字体样式。");
        };
        let required = |name: &str| -> R<&Value> {
            match map.get(name) {
                None | Some(Value::Null) => err(format!("缺少公式样式字段：{name}")),
                Some(v) => Ok(v),
            }
        };
        if text_of(required("typographyVersion")?) != TYPOGRAPHY_VERSION.to_string() {
            return err("不支持的公式样式版本。");
        }
        let number = text_of(required("numberFontFamily")?);
        let points = parse_points(required("fontSizePoints")?)?;
        Self::new(
            &text_of(required("symbolFontId")?),
            (!number.is_empty()).then_some(number.as_str()),
            &text_of(required("cjkFontFamily")?),
            &text_of(required("defaultMathStyle")?),
            &json!(points),
            &text_of(required("color")?),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct HostDefaults {
    pub typography: Typography,
    pub follow_host_font_size: bool,
}

impl HostDefaults {
    pub fn to_preset(&self) -> Value {
        json!({
            "schema": SETTINGS_SCHEMA,
            "followHostFontSize": self.follow_host_font_size,
            "typography": self.typography.to_fields(),
        })
    }

    pub fn from_preset(data: &Value) -> R<Self> {
        let schema_ok = data
            .get("schema")
            .is_some_and(|s| s.is_number() && s.as_f64() == Some(SETTINGS_SCHEMA as f64));
        if !data.is_object() || !schema_ok {
            return err("不支持的用户配置版本。");
        }
        let Some(follow) = data.get("followHostFontSize").and_then(Value::as_bool) else {
            return err("无效的预设字号模式。");
        };
        Ok(Self {
            typography: Typography::from_fields(data.get("typography").unwrap_or(&Value::Null))?,
            follow_host_font_size: follow,
        })
    }
}

pub fn default_options(host: &str) -> Map<String, Value> {
    let mut options = Map::new();
    for (h, name, values) in CHOICES {
        if h == host {
            options.insert(name.into(), json!(values[0]));
        }
    }
    for (h, name) in FLAGS {
        if h == host {
            options.insert(name.into(), json!(false));
        }
    }
    options
}

/// The host's options from `payload`; missing or unknown values become defaults.
pub fn normalize_options(host: &str, payload: &Value) -> Map<String, Value> {
    let mut options = default_options(host);
    for (h, name, values) in CHOICES {
        if h == host {
            if let Some(v) = payload
                .get(name)
                .and_then(Value::as_str)
                .filter(|v| values.contains(v))
            {
                options.insert(name.into(), json!(v));
            }
        }
    }
    for (h, name) in FLAGS {
        if h == host {
            if let Some(v) = payload.get(name).and_then(Value::as_bool) {
                options.insert(name.into(), json!(v));
            }
        }
    }
    options
}

pub fn option_catalog() -> Value {
    let mut catalog = Map::new();
    for host in HOSTS {
        let mut entry = Map::new();
        for (h, name, values) in CHOICES {
            if h == host {
                entry.insert(name.into(), json!(values));
            }
        }
        catalog.insert(host.into(), Value::Object(entry));
    }
    Value::Object(catalog)
}

pub fn math_style_catalog() -> Value {
    Value::Array(
        MATH_STYLES
            .iter()
            .map(|(id, zh, en)| json!({"id": id, "zh": zh, "en": en}))
            .collect(),
    )
}

pub fn cjk_font_catalog() -> Value {
    Value::Array(
        CJK_FONTS
            .iter()
            .map(|(id, label)| json!({"id": id, "label": label}))
            .collect(),
    )
}

pub fn named_sizes() -> Value {
    Value::Object(
        NAMED_SIZES
            .iter()
            .map(|(n, p)| (n.to_string(), json!(p)))
            .collect(),
    )
}

fn validate_host(host: &str) -> R<()> {
    if HOSTS.contains(&host) {
        Ok(())
    } else {
        err("未知 Office 宿主。")
    }
}

type HostSettings = (HostDefaults, Map<String, Value>);

/// Per-host formula defaults and add-in options in one private JSON file.
pub struct SettingsStore {
    pub path: PathBuf,
    pub symbol_fonts: Vec<String>,
    lock: Mutex<()>,
}

impl SettingsStore {
    pub fn new(path: PathBuf, symbol_fonts: Vec<String>) -> Self {
        Self {
            path,
            symbol_fonts,
            lock: Mutex::new(()),
        }
    }

    pub fn load(&self, host: &str) -> R<HostSettings> {
        validate_host(host)?;
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        Ok(self.read().remove(host).unwrap_or_default())
    }

    pub fn save(&self, host: &str, defaults: HostDefaults, options: &Value) -> R<()> {
        validate_host(host)?;
        self.validate(&defaults)?;
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let mut settings = self.read();
        settings.insert(host.into(), (defaults, normalize_options(host, options)));
        self.write(&settings)
            .map_err(|e| TypographyError(format!("无法保存设置：{e}")))
    }

    pub fn import_preset(&self, host: &str, raw: &[u8]) -> R<HostDefaults> {
        if raw.len() > MAX_PRESET_BYTES {
            return err("预设文件过大。");
        }
        let raw = raw.strip_prefix(b"\xef\xbb\xbf").unwrap_or(raw);
        let data: Value = std::str::from_utf8(raw)
            .ok()
            .and_then(|s| serde_json::from_str(s).ok())
            .ok_or_else(|| TypographyError("预设不是有效的 JSON。".into()))?;
        let defaults = HostDefaults::from_preset(&data)?;
        self.validate(&defaults)?;
        let (_, options) = self.load(host)?;
        self.save(host, defaults.clone(), &Value::Object(options))?;
        Ok(defaults)
    }

    pub fn export_preset(&self, host: &str) -> R<String> {
        let (defaults, _) = self.load(host)?;
        Ok(serde_json::to_string_pretty(&defaults.to_preset()).unwrap_or_default())
    }

    fn validate(&self, defaults: &HostDefaults) -> R<()> {
        if !self
            .symbol_fonts
            .contains(&defaults.typography.symbol_font_id)
        {
            return err(format!(
                "预设使用未打包的数学符号字体：{}",
                defaults.typography.symbol_font_id
            ));
        }
        Ok(())
    }

    fn defaults() -> std::collections::HashMap<String, HostSettings> {
        HOSTS
            .iter()
            .map(|h| (h.to_string(), (HostDefaults::default(), default_options(h))))
            .collect()
    }

    fn parse_file(&self, text: &str) -> R<std::collections::HashMap<String, HostSettings>> {
        let data: Value =
            serde_json::from_str(text).map_err(|_| TypographyError("invalid JSON".into()))?;
        if data.get("schema").and_then(Value::as_i64) != Some(SETTINGS_SCHEMA) {
            return err("不支持的用户配置版本。");
        }
        let mut result = std::collections::HashMap::new();
        for host in HOSTS {
            let entry = data.get(host).unwrap_or(&Value::Null);
            let defaults = HostDefaults::from_preset(entry)?;
            self.validate(&defaults)?;
            let options = normalize_options(host, entry.get("options").unwrap_or(&Value::Null));
            result.insert(host.to_string(), (defaults, options));
        }
        Ok(result)
    }

    fn read(&self) -> std::collections::HashMap<String, HostSettings> {
        match std::fs::read_to_string(&self.path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::defaults(),
            Ok(text) if self.parse_file(&text).is_ok() => self.parse_file(&text).unwrap(),
            // Like the Windows store: unsupported or corrupt settings are
            // rebuilt from current defaults rather than migrated.
            _ => {
                let settings = Self::defaults();
                let _ = self.write(&settings);
                settings
            }
        }
    }

    fn write(
        &self,
        settings: &std::collections::HashMap<String, HostSettings>,
    ) -> std::io::Result<()> {
        let mut payload = Map::new();
        payload.insert("schema".into(), json!(SETTINGS_SCHEMA));
        for host in HOSTS {
            if let Some((defaults, options)) = settings.get(host) {
                let mut entry = defaults.to_preset();
                entry["options"] = Value::Object(options.clone());
                payload.insert(host.into(), entry);
            }
        }
        let text = serde_json::to_string_pretty(&Value::Object(payload)).unwrap_or_default();
        crate::paths::write_atomic(&self.path, text.as_bytes())
    }
}

/// The `/api/settings/<host>` payload.
pub fn settings_payload(defaults: &HostDefaults, options: &Map<String, Value>) -> Value {
    let t = &defaults.typography;
    let mut payload = options.clone();
    for (k, v) in [
        ("followHostFontSize", json!(defaults.follow_host_font_size)),
        ("formulaColor", json!(t.color)),
        ("formulaMathStyle", json!(t.default_math_style)),
        ("formulaFontSizePoints", json!(t.font_size_points)),
        ("symbolFontId", json!(t.symbol_font_id)),
        (
            "numberFontFamily",
            json!(t.number_font_family.clone().unwrap_or_default()),
        ),
        ("cjkFontFamily", json!(t.cjk_font_family)),
        ("typography", t.to_fields()),
    ] {
        payload.insert(k.into(), v);
    }
    Value::Object(payload)
}

/// Parse the settings form of `/api/settings/<host>` (PUT).
pub fn defaults_from_form(payload: &Value) -> R<HostDefaults> {
    let s = |name: &str| -> String {
        match payload.get(name) {
            None | Some(Value::Null) | Some(Value::Bool(false)) => String::new(),
            Some(Value::String(v)) => v.clone(),
            Some(other) => other.to_string(),
        }
    };
    let number = s("numberFontFamily");
    let typography = Typography::new(
        &s("symbolFontId"),
        (!number.is_empty()).then_some(number.as_str()),
        &s("cjkFontFamily"),
        &s("formulaMathStyle"),
        payload.get("formulaFontSizePoints").unwrap_or(&Value::Null),
        &s("formulaColor"),
    )?;
    let Some(follow) = payload.get("followHostFontSize").and_then(Value::as_bool) else {
        return err("无效的预设字号模式。");
    };
    Ok(HostDefaults {
        typography,
        follow_host_font_size: follow,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FONTS: [&str; 2] = ["mathjax-tex", "mathjax-stix2"];
    /// A preset exported by the Windows TypographySettingsStore.
    const WINDOWS_PRESET: &str = r##"{"schema":2,"followHostFontSize":true,"typography":{"typographyVersion":1,"symbolFontId":"mathjax-stix2","numberFontFamily":"Arial","cjkFontFamily":"Microsoft YaHei","defaultMathStyle":"BoldItalic","fontSizePoints":10.5,"color":"#1f2937"}}"##;

    fn store() -> (tempfile::TempDir, SettingsStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(
            dir.path().join("settings.json"),
            FONTS.iter().map(|s| s.to_string()).collect(),
        );
        (dir, store)
    }

    #[test]
    fn fields_match_the_windows_wire_names() {
        let t = Typography::new(
            "mathjax-tex",
            Some("Arial"),
            "PingFang SC",
            "Automatic",
            &json!(12),
            "#abcdef",
        )
        .unwrap();
        assert_eq!(
            t.to_fields(),
            json!({"typographyVersion":1,"symbolFontId":"mathjax-tex","numberFontFamily":"Arial","cjkFontFamily":"PingFang SC","defaultMathStyle":"Automatic","fontSizePoints":12.0,"color":"#ABCDEF"})
        );
        assert_eq!(Typography::from_fields(&t.to_fields()).unwrap(), t);
        assert_eq!(
            serde_json::to_string(&t.to_fields()["fontSizePoints"]).unwrap(),
            "12.0"
        );
    }

    #[test]
    fn rejects_invalid_fields() {
        let base = Typography::default().to_fields();
        for (key, value) in [
            ("typographyVersion", json!(2)),
            ("symbolFontId", json!("Bad Font")),
            ("defaultMathStyle", json!("Shiny")),
            ("fontSizePoints", json!(0.5)),
            ("fontSizePoints", json!(true)),
            ("color", json!("red")),
            ("cjkFontFamily", json!("")),
            ("cjkFontFamily", json!("x; url(evil)")),
            ("numberFontFamily", json!(format!("a{}b", char::from(1u8)))),
        ] {
            let mut fields = base.clone();
            fields[key] = value.clone();
            assert!(Typography::from_fields(&fields).is_err(), "{key}={value}");
        }
        let mut missing = base.clone();
        missing.as_object_mut().unwrap().remove("color");
        assert_eq!(
            Typography::from_fields(&missing).unwrap_err().0,
            "缺少公式样式字段：color"
        );
    }

    #[test]
    fn parses_named_and_decimal_sizes() {
        assert_eq!(parse_points(&json!("小四")).unwrap(), 12.0);
        assert_eq!(parse_points(&json!(" 10.5 ")).unwrap(), 10.5);
        assert_eq!(parse_points(&json!(14)).unwrap(), 14.0);
        for bad in [
            json!("1e3"),
            json!("-2"),
            json!("abc"),
            json!("2000"),
            json!(null),
        ] {
            assert!(parse_points(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn imports_windows_presets_and_exports_them_back() {
        let (_dir, store) = store();
        let defaults = store
            .import_preset("word", WINDOWS_PRESET.as_bytes())
            .unwrap();
        assert!(defaults.follow_host_font_size);
        assert_eq!(defaults.typography.symbol_font_id, "mathjax-stix2");
        assert_eq!(defaults.typography.color, "#1F2937");
        let exported: Value = serde_json::from_str(&store.export_preset("word").unwrap()).unwrap();
        let mut original: Value = serde_json::from_str(WINDOWS_PRESET).unwrap();
        original["typography"]["color"] = json!("#1F2937");
        assert_eq!(exported, original);
        // PowerPoint keeps its own defaults.
        assert_eq!(store.load("powerpoint").unwrap().0, HostDefaults::default());
        // BOM-prefixed files from Windows are accepted.
        let bom = [b"\xef\xbb\xbf".as_slice(), WINDOWS_PRESET.as_bytes()].concat();
        assert!(store.import_preset("powerpoint", &bom).is_ok());
    }

    #[test]
    fn rejects_unpackaged_fonts_oversized_and_invalid_presets() {
        let (_dir, store) = store();
        let unknown = WINDOWS_PRESET.replace("mathjax-stix2", "mathjax-newcm");
        assert!(store
            .import_preset("word", unknown.as_bytes())
            .unwrap_err()
            .0
            .contains("mathjax-newcm"));
        assert!(store
            .import_preset("word", &vec![b' '; MAX_PRESET_BYTES + 1])
            .is_err());
        assert!(store.import_preset("word", b"{").is_err());
        assert!(store.import_preset("word", br#"{"schema":3}"#).is_err());
        assert!(store.load("excel").is_err());
    }

    #[test]
    fn saves_options_and_rebuilds_corrupt_files() {
        let (_dir, store) = store();
        let options = json!({"numberPlacement": "Left", "numberEnclosure": "Bogus", "includeChapter": true, "insertionBackend": "Svg"});
        store
            .save("word", HostDefaults::default(), &options)
            .unwrap();
        let (_, saved) = store.load("word").unwrap();
        assert_eq!(saved["numberPlacement"], "Left");
        assert_eq!(saved["numberEnclosure"], "Parentheses");
        assert_eq!(saved["includeChapter"], true);
        assert!(!saved.contains_key("insertionBackend"));
        let file: Value =
            serde_json::from_str(&std::fs::read_to_string(&store.path).unwrap()).unwrap();
        assert_eq!(file["schema"], 2);
        assert_eq!(file["word"]["options"]["numberPlacement"], "Left");
        assert_eq!(file["powerpoint"]["options"]["insertionBackend"], "Png");

        std::fs::write(&store.path, "{broken").unwrap();
        assert_eq!(store.load("word").unwrap().1, default_options("word"));
        let rebuilt: Value =
            serde_json::from_str(&std::fs::read_to_string(&store.path).unwrap()).unwrap();
        assert_eq!(rebuilt["schema"], 2);
    }

    #[test]
    fn settings_form_round_trip() {
        let defaults = HostDefaults::default();
        let payload = settings_payload(&defaults, &default_options("powerpoint"));
        assert_eq!(payload["insertionBackend"], "Png");
        assert_eq!(payload["typography"]["symbolFontId"], "mathjax-tex");
        assert_eq!(defaults_from_form(&payload).unwrap(), defaults);
        let mut bad = payload.clone();
        bad["followHostFontSize"] = json!("yes");
        assert!(defaults_from_form(&bad).is_err());
        let mut text_size = payload;
        text_size["formulaFontSizePoints"] = json!("12");
        assert!(defaults_from_form(&text_size).is_err());
    }

    #[test]
    fn catalogs() {
        assert_eq!(
            option_catalog()["word"]["numberSeparator"],
            json!(["-", ".", "·", ":", "/"])
        );
        assert_eq!(math_style_catalog().as_array().unwrap().len(), 17);
        assert_eq!(
            cjk_font_catalog()[0],
            json!({"id": "PingFang SC", "label": "苹方-简"})
        );
        assert_eq!(named_sizes()["五号"], json!(10.5));
    }
}
