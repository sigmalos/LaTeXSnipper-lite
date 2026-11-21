//! MathML → Word OMML with LaTeXSnipper typography (the original host's
//! `omml.py`, itself a port of the Windows `OmmlTypographyMapper`).
//!
//! The conversion uses Microsoft's `mathml2omml.xsl` from the installed Word
//! and the system's libxslt (`/usr/bin/xsltproc`). Word owns equation layout;
//! the formula's symbol, number and CJK fonts, size and color are applied as
//! run properties that local source styles still override.

use crate::typography::Typography;
use crate::xml::{self, Element, Node};
use anyhow::{anyhow, bail, Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const MATH: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
pub const WORD: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
pub const WORD_STYLESHEET: &str = "Microsoft Word.app/Contents/Resources/mathml2omml.xsl";
const XSLTPROC: &str = "/usr/bin/xsltproc";

#[derive(Debug)]
pub enum OmmlError {
    /// Word (and so its stylesheet) is not installed.
    StylesheetUnavailable,
    Invalid(String),
}

impl std::fmt::Display for OmmlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StylesheetUnavailable => write!(f, "{WORD_STYLESHEET} not found"),
            Self::Invalid(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for OmmlError {}

pub fn application_dirs(home: &Path) -> [PathBuf; 2] {
    [PathBuf::from("/Applications"), home.join("Applications")]
}

pub fn find_stylesheet(dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter()
        .map(|d| d.join(WORD_STYLESHEET))
        .find(|p| p.is_file())
}

pub fn symbol_family(symbol_font_id: &str) -> Option<&'static str> {
    match symbol_font_id {
        "mathjax-tex" => Some("Cambria Math"),
        "mathjax-stix2" => Some("STIX Two Math"),
        _ => None,
    }
}

/// Apply `stylesheet` with libxslt, offline and without DTD validation.
pub fn transform(stylesheet: &Path, input: &str) -> Result<String> {
    let mut child = Command::new(XSLTPROC)
        .args(["--nonet", "--novalid"])
        .arg(stylesheet)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("cannot run xsltproc")?;
    let mut stdin = child.stdin.take().context("xsltproc stdin")?;
    let bytes = input.as_bytes().to_vec();
    // Write from another thread so a large result cannot block on full pipes.
    let writer = std::thread::spawn(move || stdin.write_all(&bytes));
    let out = child.wait_with_output()?;
    let _ = writer.join();
    if !out.status.success() {
        bail!(
            "xsltproc failed: {}",
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .next()
                .unwrap_or("")
        );
    }
    String::from_utf8(out.stdout).map_err(|_| anyhow!("xsltproc returned non-UTF-8 output"))
}

pub fn mathml_to_omml(stylesheet: &Path, mathml: &str) -> Result<String, OmmlError> {
    // Parse first: rejects DTDs and entities before libxslt sees the input.
    xml::parse(mathml).map_err(|e| OmmlError::Invalid(e.to_string()))?;
    let result = transform(stylesheet, mathml).map_err(|e| OmmlError::Invalid(e.to_string()))?;
    if !result.contains("<m:oMath") {
        return Err(OmmlError::Invalid(
            "MathML conversion did not produce OMML".into(),
        ));
    }
    let mut root = xml::parse(&result).map_err(|e| OmmlError::Invalid(e.to_string()))?;
    repair_empty_nary_operands(&mut root);
    Ok(xml::serialize(&root))
}

fn is_blank_run(node: &Node) -> bool {
    matches!(node, Node::Element(e) if e.local() == "r" && e.text().trim().is_empty())
}

/// Microsoft's stylesheet can leave an n-ary (∑, ∫ …) with an empty body and
/// its operand as the next sibling; move the operand into the body.
pub fn repair_empty_nary_operands(element: &mut Element) {
    let mut i = 0;
    while i < element.children.len() {
        let needs_operand = matches!(&element.children[i], Node::Element(e)
            if e.is(MATH, "nary") && e.child(MATH, "e").is_some_and(|b| b.children.is_empty()));
        if needs_operand {
            let operand = (i + 1..element.children.len()).find(|&j| {
                matches!(element.children[j], Node::Element(_))
                    && !is_blank_run(&element.children[j])
            });
            if let Some(j) = operand {
                let moved = element.children.remove(j);
                if let Node::Element(nary) = &mut element.children[i] {
                    if let Some(body) = nary.child_mut(MATH, "e") {
                        body.children.push(moved);
                    }
                }
            }
        }
        if let Node::Element(child) = &mut element.children[i] {
            repair_empty_nary_operands(child);
        }
        i += 1;
    }
}

fn is_number(text: &str) -> bool {
    let digit = |c: char| c.is_ascii_digit() || ('０'..='９').contains(&c);
    let mut groups = text.split(['.', ',']);
    groups
        .next()
        .is_some_and(|g| !g.is_empty() && g.chars().all(digit))
        && groups.all(|g| !g.is_empty() && g.chars().all(digit))
}

pub fn apply_typography(omml: &str, typography: &Typography) -> Result<String, OmmlError> {
    let mut root = xml::parse(omml).map_err(|e| OmmlError::Invalid(e.to_string()))?;
    if !(root.is(MATH, "oMath") || root.is(MATH, "oMathPara")) {
        return Err(OmmlError::Invalid("需要 OMML 公式根节点。".into()));
    }
    let symbol = symbol_family(&typography.symbol_font_id)
        .ok_or_else(|| OmmlError::Invalid("不支持的数学字体方案。".into()))?;
    let size = ((typography.font_size_points * 2.0 + 0.5) as i64).to_string();
    let color = typography.color.trim_start_matches('#').to_string();
    // Declare w: on the root when the document does not use it yet.
    let w = match root.declared_prefix(WORD) {
        Some(p) => p.to_string(),
        None => {
            let prefix = if root.attrs.iter().any(|a| a.name == "xmlns:w") {
                "lsw"
            } else {
                "w"
            };
            root.attrs.push(xml::Attr {
                name: format!("xmlns:{prefix}"),
                ns: None,
                value: WORD.into(),
            });
            prefix.to_string()
        }
    };
    let style = Style {
        symbol,
        number: typography.number_font_family.as_deref(),
        cjk: &typography.cjk_font_family,
        size: &size,
        color: &color,
        w: &w,
    };
    style_runs(&mut root, &style);
    Ok(xml::serialize(&root))
}

struct Style<'a> {
    symbol: &'a str,
    number: Option<&'a str>,
    cjk: &'a str,
    size: &'a str,
    color: &'a str,
    w: &'a str,
}

fn style_runs(element: &mut Element, style: &Style) {
    if element.is(MATH, "r") {
        style_run(element, style);
    }
    for child in &mut element.children {
        if let Node::Element(e) = child {
            style_runs(e, style);
        }
    }
}

fn style_run(run: &mut Element, style: &Style) {
    let text: String = run
        .elements()
        .filter(|e| e.is(MATH, "t"))
        .map(|t| {
            t.children
                .iter()
                .filter_map(|n| match n {
                    Node::Text(s) => Some(s.as_str()),
                    _ => None,
                })
                .collect::<String>()
        })
        .collect();
    let normal = run.child(MATH, "rPr").and_then(|p| p.child(MATH, "nor"));
    let is_text = normal.is_some_and(|n| !matches!(n.attr(MATH, "val"), Some("0" | "false")));
    let number = !is_text && is_number(&text);
    let family = if is_text {
        "Times New Roman"
    } else if number && style.number.is_some() {
        style.number.unwrap()
    } else {
        style.symbol
    };
    let w = style.w;
    if run.child_index(WORD, "rPr").is_none() {
        let at = run.child_index(MATH, "rPr").map_or(0, |i| i + 1);
        run.children
            .insert(at, Node::Element(Element::new_child(WORD, w, "rPr")));
    }
    let properties = run.child_mut(WORD, "rPr").expect("inserted above");
    if properties.child_index(WORD, "rFonts").is_none() {
        properties
            .children
            .insert(0, Node::Element(Element::new_child(WORD, w, "rFonts")));
    }
    let fonts = properties
        .child_mut(WORD, "rFonts")
        .expect("inserted above");
    fonts.set_attr(WORD, w, "ascii", family);
    fonts.set_attr(WORD, w, "hAnsi", family);
    fonts.set_attr(WORD, w, "eastAsia", style.cjk);
    for theme in ["asciiTheme", "hAnsiTheme", "eastAsiaTheme"] {
        fonts.remove_attr(WORD, theme);
    }
    for (name, value) in [
        ("color", style.color),
        ("sz", style.size),
        ("szCs", style.size),
    ] {
        if properties.child(WORD, name).is_none() {
            let mut e = Element::new_child(WORD, w, name);
            e.set_attr(WORD, w, "val", value);
            properties.children.push(Node::Element(e));
        }
    }
}

pub fn convert(
    stylesheet: &Path,
    mathml: &str,
    typography: &Typography,
) -> Result<String, OmmlError> {
    apply_typography(&mathml_to_omml(stylesheet, mathml)?, typography)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn omml() -> String {
        format!(
            r#"<m:oMath xmlns:m="{MATH}" xmlns:w="{WORD}"><m:r><m:t>12</m:t></m:r><m:r><m:t>x</m:t></m:r><m:r><m:rPr><m:nor/></m:rPr><m:t>中文</m:t></m:r><m:r><w:rPr><w:color w:val="FF0000"/></w:rPr><m:t>y</m:t></m:r></m:oMath>"#
        )
    }

    #[derive(Debug, PartialEq)]
    struct Run {
        text: String,
        ascii: String,
        east_asia: String,
        color: String,
        size: String,
    }

    fn runs(omml: &str) -> Vec<Run> {
        let root = xml::parse(omml).unwrap();
        let mut out = Vec::new();
        fn walk(e: &Element, out: &mut Vec<Run>) {
            if e.is(MATH, "r") {
                let p = e.child(WORD, "rPr").unwrap();
                let f = p.child(WORD, "rFonts").unwrap();
                let val = |n: &str| {
                    p.child(WORD, n)
                        .unwrap()
                        .attr(WORD, "val")
                        .unwrap()
                        .to_string()
                };
                out.push(Run {
                    text: e.text(),
                    ascii: f.attr(WORD, "ascii").unwrap().into(),
                    east_asia: f.attr(WORD, "eastAsia").unwrap().into(),
                    color: val("color"),
                    size: val("sz"),
                });
            }
            e.elements().for_each(|c| walk(c, out));
        }
        walk(&root, &mut out);
        out
    }

    fn typography(
        symbol: &str,
        number: Option<&str>,
        cjk: &str,
        size: f64,
        color: &str,
    ) -> Typography {
        Typography::new(symbol, number, cjk, "Automatic", &json!(size), color).unwrap()
    }

    #[test]
    fn applies_symbol_number_and_cjk_fonts_like_windows() {
        let t = typography("mathjax-stix2", Some("Arial"), "Songti SC", 10.5, "#123456");
        let result = runs(&apply_typography(&omml(), &t).unwrap());
        let ascii: Vec<_> = result.iter().map(|r| r.ascii.as_str()).collect();
        assert_eq!(
            ascii,
            ["Arial", "STIX Two Math", "Times New Roman", "STIX Two Math"]
        );
        assert!(result
            .iter()
            .all(|r| r.east_asia == "Songti SC" && r.size == "21"));
        // A source-local color from the converter wins over the default color.
        let colors: Vec<_> = result.iter().map(|r| r.color.as_str()).collect();
        assert_eq!(colors, ["123456", "123456", "123456", "FF0000"]);
    }

    #[test]
    fn numbers_follow_the_symbol_font_without_a_number_family() {
        let out = apply_typography(&omml(), &Typography::default()).unwrap();
        assert_eq!(runs(&out)[0].ascii, "Cambria Math");
        assert!(is_number("1,000.5") && is_number("２") && !is_number("1.") && !is_number("x1"));
    }

    #[test]
    fn declares_the_word_namespace_and_orders_run_properties() {
        let bare = format!(
            r#"<m:oMathPara xmlns:m="{MATH}"><m:oMath><m:r><m:rPr><m:sty m:val="b"/></m:rPr><m:t>b</m:t></m:r></m:oMath></m:oMathPara>"#
        );
        let out = apply_typography(&bare, &Typography::default()).unwrap();
        let root = xml::parse(&out).unwrap();
        assert_eq!(root.declared_prefix(WORD), Some("w"));
        let run = root.elements().next().unwrap().elements().next().unwrap();
        let order: Vec<_> = run.elements().map(|e| e.name.as_str()).collect();
        assert_eq!(order, ["m:rPr", "w:rPr", "m:t"]);
        let props: Vec<_> = run
            .child(WORD, "rPr")
            .unwrap()
            .elements()
            .map(|e| e.local())
            .collect();
        assert_eq!(props, ["rFonts", "color", "sz", "szCs"]);
    }

    #[test]
    fn rejects_non_omml_roots_and_unknown_symbol_fonts() {
        assert!(apply_typography("<root/>", &Typography::default()).is_err());
        let t = Typography {
            symbol_font_id: "mathjax-newcm".into(),
            ..Typography::default()
        };
        assert!(apply_typography(&omml(), &t).is_err());
    }

    #[test]
    fn moves_the_operand_into_an_empty_nary_body() {
        let input = format!(
            r#"<m:oMath xmlns:m="{MATH}"><m:nary><m:naryPr/><m:sub/><m:sup/><m:e/></m:nary><m:r><m:t> </m:t></m:r><m:r><m:t>k</m:t></m:r><m:r><m:t>+1</m:t></m:r></m:oMath>"#
        );
        let mut root = xml::parse(&input).unwrap();
        repair_empty_nary_operands(&mut root);
        let nary = root.child(MATH, "nary").unwrap();
        assert_eq!(nary.child(MATH, "e").unwrap().text(), "k");
        let rest: Vec<_> = root
            .elements()
            .filter(|e| e.local() == "r")
            .map(|e| e.text())
            .collect();
        assert_eq!(rest, [" ", "+1"]);
    }

    #[test]
    fn finds_the_stylesheet_inside_word_for_mac() {
        let dir = tempfile::tempdir().unwrap();
        let dirs = [
            dir.path().join("Applications"),
            dir.path().join("home/Applications"),
        ];
        assert!(find_stylesheet(&dirs).is_none());
        let xsl = dirs[1].join(WORD_STYLESHEET);
        std::fs::create_dir_all(xsl.parent().unwrap()).unwrap();
        std::fs::write(&xsl, "").unwrap();
        assert_eq!(find_stylesheet(&dirs), Some(xsl));
    }

    #[test]
    fn rejects_mathml_with_a_doctype_before_running_xslt() {
        let xsl = PathBuf::from("/nonexistent.xsl");
        let bomb = r#"<!DOCTYPE math [<!ENTITY a "x">]><math xmlns="http://www.w3.org/1998/Math/MathML"><mi>&a;</mi></math>"#;
        assert!(matches!(
            mathml_to_omml(&xsl, bomb),
            Err(OmmlError::Invalid(_))
        ));
    }

    /// With Word installed: the real stylesheet, including the n-ary repair.
    #[test]
    fn converts_with_the_installed_word() {
        let Some(xsl) = find_stylesheet(&application_dirs(&crate::paths::home())) else {
            eprintln!("skipped: Microsoft Word is not installed");
            return;
        };
        let mathml = "<math xmlns='http://www.w3.org/1998/Math/MathML'><munderover><mo>&#x2211;</mo><mi>k</mi><mi>n</mi></munderover><mi>k</mi></math>";
        let out = convert(&xsl, mathml, &Typography::default()).unwrap();
        let root = xml::parse(&out).unwrap();
        fn find<'a>(e: &'a Element, local: &str) -> Option<&'a Element> {
            if e.is(MATH, local) {
                return Some(e);
            }
            e.elements().find_map(|c| find(c, local))
        }
        let nary = find(&root, "nary").expect("n-ary");
        assert_eq!(nary.child(MATH, "e").unwrap().text(), "k");
        assert!(runs(&out)
            .iter()
            .all(|r| r.ascii == "Cambria Math" && r.size == "24"));
    }
}
