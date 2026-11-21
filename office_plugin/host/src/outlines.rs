//! System-font glyph outlines for the MathJax typography adapter (the
//! original host's `outlines.py`, the macOS counterpart of the Windows
//! `SystemFontOutlineService`).
//!
//! Each run returns baseline-relative SVG path data in 1000 units per em with
//! the y axis pointing up, plus advance, ink height/depth and horizontal ink
//! bounds in em. CoreText shapes the whole run; fallback is explicit and
//! ordered, because each candidate family must cover the whole run.

use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::Mutex;

pub const EM: f64 = 1000.0;
pub const BOLD: i64 = 1;
pub const ITALIC: i64 = 2;
pub const FALLBACK_FAMILIES: [&str; 5] = [
    "Times New Roman",
    "PingFang SC",
    "Songti SC",
    "Apple Symbols",
    "Menlo",
];
const MAX_RUN_CHARACTERS: usize = 2048;
const MAX_CACHE: usize = 4096;

#[derive(Debug, Clone, PartialEq)]
pub struct OutlineError(pub String);

impl std::fmt::Display for OutlineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// How fonts are found and drawn; CoreText in the app, a stub in tests.
pub trait FontSystem: Send + Sync {
    /// Installed family names.
    fn families(&self) -> Vec<String>;
    /// Whether `family` has a face with `style` (bold/italic bits).
    fn has_style(&self, family: &str, style: i64) -> bool;
    /// Characters of `text` the face cannot draw, as `U+XXXX`.
    fn missing(&self, family: &str, style: i64, text: &str) -> Vec<String>;
    /// Shape `text` with exactly this face.
    fn outline(&self, family: &str, style: i64, text: &str) -> Shape;
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shape {
    /// In font units at 1000 per em.
    pub advance: f64,
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
    pub path: String,
}

pub struct Outlines {
    fonts: Box<dyn FontSystem>,
    fallback: Vec<String>,
    installed: Mutex<Option<HashMap<String, String>>>,
    cache: Mutex<HashMap<(String, String, i64), Value>>,
}

impl Outlines {
    pub fn new(fonts: Box<dyn FontSystem>) -> Self {
        Self {
            fonts,
            fallback: FALLBACK_FAMILIES.iter().map(|s| s.to_string()).collect(),
            installed: Mutex::new(None),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// CoreText-backed outlines (macOS).
    #[cfg(target_os = "macos")]
    pub fn system() -> Self {
        Self::new(Box::new(coretext::CoreTextFonts))
    }

    fn installed(&self) -> HashMap<String, String> {
        let mut guard = self.installed.lock().unwrap_or_else(|p| p.into_inner());
        guard
            .get_or_insert_with(|| {
                self.fonts
                    .families()
                    .into_iter()
                    .map(|name| (name.to_lowercase(), name))
                    .collect()
            })
            .clone()
    }

    /// Family names to offer in Settings; names starting with "." are private
    /// macOS UI fonts.
    pub fn families(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .installed()
            .into_values()
            .filter(|n| !n.starts_with('.'))
            .collect();
        names.sort_by_key(|n| n.to_lowercase());
        names.dedup();
        names
    }

    /// Shape adapter runs, keyed by the adapter's opaque run key.
    pub fn shape_runs(&self, runs: &[Value]) -> Result<Value, OutlineError> {
        let mut result = Map::new();
        for run in runs {
            let (Some(key), Some(text), Some(family)) = (
                run.get("key").and_then(Value::as_str),
                run.get("text").and_then(Value::as_str),
                run.get("family").and_then(Value::as_str),
            ) else {
                return Err(OutlineError("文字段数据无效。".into()));
            };
            let style = run
                .get("style")
                .filter(|s| s.is_i64() || s.is_u64())
                .and_then(Value::as_i64)
                .filter(|s| s & !(BOLD | ITALIC) == 0)
                .ok_or_else(|| OutlineError("文字段样式无效。".into()))?;
            result.insert(key.to_string(), self.shape(text, family, style)?);
        }
        Ok(Value::Object(result))
    }

    pub fn shape(&self, text: &str, family: &str, style: i64) -> Result<Value, OutlineError> {
        if text.chars().count() > MAX_RUN_CHARACTERS {
            return Err(OutlineError("文字段过长。".into()));
        }
        if text.chars().any(|c| (c as u32) < 32 || (c as u32) > 0xFFFF) {
            return Err(OutlineError(
                "系统字体轮廓暂不支持辅助平面字符或控制字符；请使用受支持的数学字形或显式换行结构。".into(),
            ));
        }
        let family = family.trim();
        if family.is_empty() {
            return Err(OutlineError("字体家族不能为空。".into()));
        }
        let key = (text.to_string(), family.to_string(), style);
        if let Some(hit) = self
            .cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&key)
        {
            return Ok(hit.clone());
        }
        let value = self.shape_uncached(text, family, style)?;
        let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        if cache.len() > MAX_CACHE {
            cache.clear();
        }
        cache.insert(key, value.clone());
        Ok(value)
    }

    fn shape_uncached(
        &self,
        text: &str,
        requested: &str,
        style: i64,
    ) -> Result<Value, OutlineError> {
        let installed = self.installed();
        let mut failures = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let candidates = std::iter::once(requested).chain(self.fallback.iter().map(String::as_str));
        for candidate in candidates {
            if !seen.insert(candidate.to_lowercase()) {
                continue;
            }
            let Some(name) = installed.get(&candidate.to_lowercase()) else {
                failures.push(format!("{candidate}：未安装"));
                continue;
            };
            if !self.fonts.has_style(name, style) {
                failures.push(format!("{name}：字形样式不可用"));
                continue;
            }
            let missing = self.fonts.missing(name, style, text);
            if !missing.is_empty() {
                failures.push(format!("{name}：缺少 {}", missing.join(" ")));
                continue;
            }
            let warning =
                (!failures.is_empty()).then(|| format!("{}；已使用 {name}", failures.join("；")));
            let shape = if text.is_empty() {
                Shape::default()
            } else {
                self.fonts.outline(name, style, text)
            };
            let empty = shape.path.is_empty();
            let ink = |v: f64| if empty { 0.0 } else { v / EM };
            return Ok(json!({
                "Text": text,
                "RequestedFamily": requested,
                "ActualFamily": name,
                "Style": style,
                "Warning": warning,
                "Advance": shape.advance / EM,
                "Height": ink(shape.top.max(0.0)),
                "Depth": ink((-shape.bottom).max(0.0)),
                "Left": ink(shape.left),
                "Right": ink(shape.right),
                "Path": shape.path,
            }));
        }
        Err(OutlineError(format!(
            "无法为文字段选择字体：{}",
            failures.join("；")
        )))
    }
}

/// `f"{value:.3f}"` without trailing zeros, and never `-0`.
pub fn number(value: f64) -> String {
    let text = format!("{value:.3}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    match text {
        "" | "-0" => "0".into(),
        t => t.into(),
    }
}

#[cfg(target_os = "macos")]
mod coretext {
    use super::{number, FontSystem, Shape, BOLD, EM, ITALIC};
    use core_foundation::attributed_string::CFMutableAttributedString;
    use core_foundation::base::{CFRange, CFType, TCFType};
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use core_graphics::geometry::{CGAffineTransform, CGRect};
    use core_graphics::path::{CGPath, CGPathElementType};
    use core_text::font::{self, CTFont};
    use core_text::font_descriptor::{
        self, kCTFontBoldTrait, kCTFontFamilyNameAttribute, kCTFontItalicTrait,
        kCTFontSymbolicTrait, kCTFontTraitsAttribute,
    };
    use core_text::line::CTLine;
    use core_text::string_attributes::kCTFontAttributeName;
    use foreign_types::ForeignType;
    use std::cell::RefCell;

    extern "C" {
        fn CGPathGetPathBoundingBox(path: core_graphics::sys::CGPathRef) -> CGRect;
        fn CGPathIsEmpty(path: core_graphics::sys::CGPathRef) -> bool;
    }

    pub struct CoreTextFonts;

    fn traits(style: i64) -> u32 {
        let mut t = 0;
        if style & BOLD != 0 {
            t |= kCTFontBoldTrait;
        }
        if style & ITALIC != 0 {
            t |= kCTFontItalicTrait;
        }
        t
    }

    /// The family's face for `style`, or None if the family lacks that style.
    fn face(family: &str, style: i64) -> Option<CTFont> {
        let wanted = traits(style);
        let traits_dict = CFDictionary::from_CFType_pairs(&[(
            unsafe { CFString::wrap_under_get_rule(kCTFontSymbolicTrait) },
            CFNumber::from(wanted as i32).as_CFType(),
        )]);
        let attributes = CFDictionary::from_CFType_pairs(&[
            (
                unsafe { CFString::wrap_under_get_rule(kCTFontFamilyNameAttribute) },
                CFString::new(family).as_CFType(),
            ),
            (
                unsafe { CFString::wrap_under_get_rule(kCTFontTraitsAttribute) },
                traits_dict.as_CFType(),
            ),
        ]);
        let descriptor = font_descriptor::new_from_attributes(&attributes);
        let font = font::new_from_descriptor(&descriptor, EM);
        if font.family_name().to_lowercase() != family.to_lowercase() {
            return None;
        }
        (font.symbolic_traits() & wanted == wanted).then_some(font)
    }

    impl FontSystem for CoreTextFonts {
        fn families(&self) -> Vec<String> {
            core_text::font_manager::copy_available_font_family_names()
                .iter()
                .map(|n| n.to_string())
                .collect()
        }

        fn has_style(&self, family: &str, style: i64) -> bool {
            face(family, style).is_some()
        }

        fn missing(&self, family: &str, style: i64, text: &str) -> Vec<String> {
            let Some(font) = face(family, style) else {
                return vec![];
            };
            text.chars()
                .filter(|c| !c.is_whitespace())
                .filter(|&c| {
                    let unit = c as u16;
                    let mut glyph = 0u16;
                    !unsafe { font.get_glyphs_for_characters(&unit, &mut glyph, 1) }
                })
                .map(|c| format!("U+{:04X}", c as u32))
                .collect()
        }

        fn outline(&self, family: &str, style: i64, text: &str) -> Shape {
            let Some(font) = face(family, style) else {
                return Shape::default();
            };
            let mut attributed = CFMutableAttributedString::new();
            let string = CFString::new(text);
            attributed.replace_str(&string, CFRange::init(0, 0));
            let length = string.char_len();
            attributed.set_attribute(
                CFRange::init(0, length),
                unsafe { kCTFontAttributeName },
                &font,
            );
            let line = CTLine::new_with_attributed_string(attributed.as_concrete_TypeRef());
            let advance = line.get_typographic_bounds().width;
            let parts = RefCell::new(String::new());
            let mut bounds: Option<(f64, f64, f64, f64)> = None;
            for run in line.glyph_runs().iter() {
                let run_font = run
                    .attributes()
                    .and_then(|a| {
                        a.find(unsafe { CFString::wrap_under_get_rule(kCTFontAttributeName) })
                            .map(|v| v.clone())
                    })
                    .and_then(|v: CFType| v.downcast::<CTFont>())
                    .unwrap_or_else(|| font.clone());
                let glyphs = run.glyphs();
                let positions = run.positions();
                for (glyph, position) in glyphs.iter().zip(positions.iter()) {
                    let transform =
                        CGAffineTransform::new(1.0, 0.0, 0.0, 1.0, position.x, position.y);
                    let Ok(path) = run_font.create_path_for_glyph(*glyph, &transform) else {
                        continue;
                    };
                    if unsafe { CGPathIsEmpty(path.as_ptr()) } {
                        continue;
                    }
                    let b = unsafe { CGPathGetPathBoundingBox(path.as_ptr()) };
                    let (l, bo, r, t) = (
                        b.origin.x,
                        b.origin.y,
                        b.origin.x + b.size.width,
                        b.origin.y + b.size.height,
                    );
                    bounds = Some(match bounds {
                        None => (l, bo, r, t),
                        Some((l0, b0, r0, t0)) => (l0.min(l), b0.min(bo), r0.max(r), t0.max(t)),
                    });
                    append_path(&path, &parts);
                }
            }
            let Some((left, bottom, right, top)) = bounds else {
                return Shape {
                    advance,
                    ..Shape::default()
                };
            };
            // CoreText already uses a y-up, baseline-relative space: no flip needed.
            Shape {
                advance,
                left,
                right,
                top,
                bottom,
                path: parts.into_inner(),
            }
        }
    }

    fn append_path(path: &CGPath, out: &RefCell<String>) {
        let push = |element: core_graphics::path::CGPathElementRef| {
            let p = element.points();
            let xy = |i: usize| format!("{} {}", number(p[i].x), number(p[i].y));
            let mut out = out.borrow_mut();
            match element.element_type {
                CGPathElementType::MoveToPoint => out.push_str(&format!("M{}", xy(0))),
                CGPathElementType::AddLineToPoint => out.push_str(&format!("L{}", xy(0))),
                CGPathElementType::AddQuadCurveToPoint => {
                    out.push_str(&format!("Q{} {}", xy(0), xy(1)))
                }
                CGPathElementType::AddCurveToPoint => {
                    out.push_str(&format!("C{} {} {}", xy(0), xy(1), xy(2)))
                }
                CGPathElementType::CloseSubpath => out.push('Z'),
            }
        };
        path.apply(&push);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "Serif" covers ASCII in regular and bold; "Han" covers CJK in regular only.
    struct Fake;

    impl FontSystem for Fake {
        fn families(&self) -> Vec<String> {
            ["Serif", "Han", ".UI", "Times New Roman"]
                .map(String::from)
                .to_vec()
        }
        fn has_style(&self, family: &str, style: i64) -> bool {
            family != "Han" || style == 0
        }
        fn missing(&self, family: &str, _style: i64, text: &str) -> Vec<String> {
            text.chars()
                .filter(|c| !c.is_whitespace())
                .filter(|c| match family {
                    "Han" => c.is_ascii(),
                    _ => !c.is_ascii(),
                })
                .map(|c| format!("U+{:04X}", c as u32))
                .collect()
        }
        fn outline(&self, _family: &str, _style: i64, text: &str) -> Shape {
            Shape {
                advance: 500.0 * text.chars().count() as f64,
                left: 20.0,
                right: 480.0,
                top: 700.0,
                bottom: -200.0,
                path: "M0 0L1 1Z".into(),
            }
        }
    }

    #[test]
    fn shapes_runs_with_explicit_fallback() {
        let o = Outlines::new(Box::new(Fake));
        let runs = serde_json::json!([
            {"key": "a", "text": "12", "family": "serif", "style": 1},
            {"key": "b", "text": "中文", "family": "Han", "style": 0},
            {"key": "c", "text": "x", "family": "Han", "style": 0},
        ]);
        let out = o.shape_runs(runs.as_array().unwrap()).unwrap();
        assert_eq!(out["a"]["ActualFamily"], "Serif");
        assert_eq!(out["a"]["Warning"], Value::Null);
        assert_eq!(out["a"]["Advance"], 1.0);
        assert_eq!(out["a"]["Height"], 0.7);
        assert_eq!(out["a"]["Depth"], 0.2);
        assert_eq!(
            (out["b"]["ActualFamily"].as_str(), &out["b"]["Warning"]),
            (Some("Han"), &Value::Null)
        );
        // A family that misses characters falls back in the fixed order.
        assert_eq!(out["c"]["ActualFamily"], "Times New Roman");
        assert_eq!(
            out["c"]["Warning"],
            "Han：缺少 U+0078；已使用 Times New Roman"
        );
        assert!(o.shape("中", "Serif", 0).unwrap_err().0.starts_with("无法为文字段选择字体：Serif：缺少 U+4E2D；Times New Roman：缺少 U+4E2D；PingFang SC：未安装"));
    }

    #[test]
    fn validates_runs() {
        let o = Outlines::new(Box::new(Fake));
        for bad in [
            serde_json::json!({"key": "a", "text": "x", "family": "Serif", "style": 4}),
            serde_json::json!({"key": "a", "text": "x", "family": "Serif", "style": true}),
            serde_json::json!({"key": "a", "text": "x", "style": 0}),
        ] {
            assert!(o.shape_runs(&[bad]).is_err());
        }
        assert!(o.shape("a\tb", "Serif", 0).is_err());
        assert!(o.shape("𝑥", "Serif", 0).is_err());
        assert!(o.shape(&"x".repeat(2049), "Serif", 0).is_err());
        assert!(o.shape("x", "  ", 0).is_err());
        assert!(o
            .shape("中", "Han", 1)
            .unwrap_err()
            .0
            .contains("Han：字形样式不可用"));
        let empty = o.shape("", "Serif", 0).unwrap();
        assert_eq!(
            (empty["Advance"].as_f64(), empty["Path"].as_str()),
            (Some(0.0), Some(""))
        );
    }

    #[test]
    fn lists_public_families_sorted() {
        let o = Outlines::new(Box::new(Fake));
        assert_eq!(o.families(), ["Han", "Serif", "Times New Roman"]);
        assert_eq!(
            (number(1.23456), number(-0.0001), number(2.0), number(-1.5)),
            ("1.235".into(), "0".into(), "2".into(), "-1.5".into())
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn coretext_outlines_real_system_fonts() {
        let o = Outlines::system();
        assert!(o.families().iter().any(|f| f == "Times New Roman"));
        let out = o.shape("12", "Times New Roman", 0).unwrap();
        assert_eq!(out["ActualFamily"], "Times New Roman");
        let advance = out["Advance"].as_f64().unwrap();
        assert!((0.9..1.1).contains(&advance), "{advance}");
        let height = out["Height"].as_f64().unwrap();
        assert!((0.5..0.8).contains(&height), "{height}");
        let path = out["Path"].as_str().unwrap();
        assert!(path.starts_with('M') && path.ends_with('Z'), "{path}");
        let bold = o.shape("x", "Times New Roman", BOLD | ITALIC).unwrap();
        assert_eq!(bold["Style"], 3);
        let cjk = o.shape("公式", "Times New Roman", 0).unwrap();
        assert_ne!(cjk["ActualFamily"], "Times New Roman");
        assert!(cjk["Warning"]
            .as_str()
            .unwrap()
            .contains("Times New Roman：缺少"));
    }
}
