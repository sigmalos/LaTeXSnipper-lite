//! A small namespace-aware XML tree for rewriting OMML, enough to replace
//! the lxml calls of the original host. Document type declarations are
//! rejected, so no entity can expand or load anything.

use anyhow::{anyhow, bail, Result};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Element(Element),
    /// Unescaped character data.
    Text(String),
    /// Comments and processing instructions, kept verbatim.
    Raw(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Attr {
    /// Qualified name as written, e.g. `w:val` or `xmlns:m`.
    pub name: String,
    pub ns: Option<String>,
    pub value: String,
}

impl Attr {
    pub fn local(&self) -> &str {
        local_of(&self.name)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    /// Qualified name as written.
    pub name: String,
    pub ns: Option<String>,
    pub attrs: Vec<Attr>,
    pub children: Vec<Node>,
}

fn local_of(qname: &str) -> &str {
    qname.rsplit_once(':').map_or(qname, |(_, l)| l)
}

fn prefix_of(qname: &str) -> Option<&str> {
    qname.split_once(':').map(|(p, _)| p)
}

impl Element {
    pub fn local(&self) -> &str {
        local_of(&self.name)
    }

    pub fn is(&self, ns: &str, local: &str) -> bool {
        self.ns.as_deref() == Some(ns) && self.local() == local
    }

    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            _ => None,
        })
    }

    pub fn child(&self, ns: &str, local: &str) -> Option<&Element> {
        self.elements().find(|e| e.is(ns, local))
    }

    pub fn child_index(&self, ns: &str, local: &str) -> Option<usize> {
        self.children
            .iter()
            .position(|n| matches!(n, Node::Element(e) if e.is(ns, local)))
    }

    pub fn child_mut(&mut self, ns: &str, local: &str) -> Option<&mut Element> {
        self.children.iter_mut().find_map(|n| match n {
            Node::Element(e) if e.is(ns, local) => Some(e),
            _ => None,
        })
    }

    pub fn attr(&self, ns: &str, local: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.ns.as_deref() == Some(ns) && a.local() == local)
            .map(|a| a.value.as_str())
    }

    /// Set an attribute in `ns`, written with `prefix` if it is new.
    pub fn set_attr(&mut self, ns: &str, prefix: &str, local: &str, value: &str) {
        if let Some(a) = self
            .attrs
            .iter_mut()
            .find(|a| a.ns.as_deref() == Some(ns) && a.local() == local)
        {
            a.value = value.to_string();
        } else {
            self.attrs.push(Attr {
                name: format!("{prefix}:{local}"),
                ns: Some(ns.to_string()),
                value: value.to_string(),
            });
        }
    }

    pub fn remove_attr(&mut self, ns: &str, local: &str) {
        self.attrs
            .retain(|a| !(a.ns.as_deref() == Some(ns) && a.local() == local));
    }

    /// Concatenated text of all descendants.
    pub fn text(&self) -> String {
        let mut out = String::new();
        fn walk(e: &Element, out: &mut String) {
            for n in &e.children {
                match n {
                    Node::Text(t) => out.push_str(t),
                    Node::Element(c) => walk(c, out),
                    Node::Raw(_) => {}
                }
            }
        }
        walk(self, &mut out);
        out
    }

    /// The prefix this element declares for `ns`, if any.
    pub fn declared_prefix(&self, ns: &str) -> Option<&str> {
        self.attrs.iter().find_map(|a| {
            (a.value == ns)
                .then(|| a.name.strip_prefix("xmlns:"))
                .flatten()
        })
    }

    pub fn new_child(ns: &str, prefix: &str, local: &str) -> Element {
        Element {
            name: format!("{prefix}:{local}"),
            ns: Some(ns.to_string()),
            attrs: Vec::new(),
            children: Vec::new(),
        }
    }
}

struct Scope(Vec<Vec<(String, String)>>);

impl Scope {
    fn resolve(&self, prefix: Option<&str>) -> Option<String> {
        let key = prefix.unwrap_or("");
        if key == "xml" {
            return Some("http://www.w3.org/XML/1998/namespace".into());
        }
        self.0
            .iter()
            .rev()
            .flat_map(|frame| frame.iter().rev())
            .find(|(p, _)| p == key)
            .map(|(_, uri)| uri.clone())
            .filter(|uri| !uri.is_empty())
    }
}

fn open(start: &BytesStart, scope: &mut Scope) -> Result<Element> {
    let name = String::from_utf8(start.name().as_ref().to_vec())?;
    let mut raw = Vec::new();
    let mut frame = Vec::new();
    for attr in start.attributes() {
        let attr = attr?;
        let key = String::from_utf8(attr.key.as_ref().to_vec())?;
        let value = attr.unescape_value()?.into_owned();
        if key == "xmlns" {
            frame.push((String::new(), value.clone()));
        } else if let Some(p) = key.strip_prefix("xmlns:") {
            frame.push((p.to_string(), value.clone()));
        }
        raw.push((key, value));
    }
    scope.0.push(frame);
    let ns = scope.resolve(prefix_of(&name));
    let attrs = raw
        .into_iter()
        .map(|(key, value)| {
            // Unprefixed attributes are in no namespace.
            let ns = if key == "xmlns" || key.starts_with("xmlns:") {
                None
            } else {
                prefix_of(&key).and_then(|p| scope.resolve(Some(p)))
            };
            Attr {
                name: key,
                ns,
                value,
            }
        })
        .collect();
    Ok(Element {
        name,
        ns,
        attrs,
        children: Vec::new(),
    })
}

/// Parse a document and return its root element.
pub fn parse(xml: &str) -> Result<Element> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut scope = Scope(Vec::new());
    let mut stack: Vec<Element> = Vec::new();
    let mut root: Option<Element> = None;
    loop {
        let event = reader
            .read_event()
            .map_err(|e| anyhow!("invalid XML: {e}"))?;
        match event {
            Event::DocType(_) => bail!("XML document type declarations are not allowed"),
            Event::Decl(_) => {}
            Event::Start(start) => {
                if root.is_some() {
                    bail!("content after the root element");
                }
                stack.push(open(&start, &mut scope)?);
            }
            Event::Empty(start) => {
                let element = open(&start, &mut scope)?;
                scope.0.pop();
                match stack.last_mut() {
                    Some(parent) => parent.children.push(Node::Element(element)),
                    None if root.is_none() => root = Some(element),
                    None => bail!("content after the root element"),
                }
            }
            Event::End(_) => {
                scope.0.pop();
                let element = stack.pop().ok_or_else(|| anyhow!("unbalanced XML"))?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(Node::Element(element)),
                    None => root = Some(element),
                }
            }
            Event::Text(text) => {
                let text = text
                    .unescape()
                    .map_err(|e| anyhow!("invalid XML text: {e}"))?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(Node::Text(text.into_owned())),
                    None if text.trim().is_empty() => {}
                    None => bail!("text outside the root element"),
                }
            }
            Event::CData(data) => {
                if let Some(parent) = stack.last_mut() {
                    parent
                        .children
                        .push(Node::Text(String::from_utf8(data.into_inner().to_vec())?));
                }
            }
            Event::Comment(c) => {
                if let Some(parent) = stack.last_mut() {
                    let body = String::from_utf8(c.into_inner().to_vec())?;
                    parent.children.push(Node::Raw(format!("<!--{body}-->")));
                }
            }
            Event::PI(pi) => {
                if let Some(parent) = stack.last_mut() {
                    let body = String::from_utf8(pi.into_inner().to_vec())?;
                    parent.children.push(Node::Raw(format!("<?{body}?>")));
                }
            }
            Event::Eof => break,
        }
    }
    if !stack.is_empty() {
        bail!("unclosed XML element");
    }
    root.ok_or_else(|| anyhow!("empty XML document"))
}

fn escape(s: &str, attr: bool, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attr => out.push_str("&quot;"),
            '\n' if attr => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' if attr => out.push_str("&#9;"),
            c => out.push(c),
        }
    }
}

pub fn serialize(root: &Element) -> String {
    let mut out = String::new();
    fn write(e: &Element, out: &mut String) {
        out.push('<');
        out.push_str(&e.name);
        for a in &e.attrs {
            out.push(' ');
            out.push_str(&a.name);
            out.push_str("=\"");
            escape(&a.value, true, out);
            out.push('"');
        }
        if e.children.is_empty() {
            out.push_str("/>");
            return;
        }
        out.push('>');
        for n in &e.children {
            match n {
                Node::Element(c) => write(c, out),
                Node::Text(t) => escape(t, false, out),
                Node::Raw(r) => out.push_str(r),
            }
        }
        out.push_str("</");
        out.push_str(&e.name);
        out.push('>');
    }
    write(root, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_namespaces_and_round_trips() {
        let xml = r#"<?xml version="1.0"?><m:oMath xmlns:m="urn:m" xmlns:w="urn:w"><m:r><w:rPr><w:color w:val="FF0000"/></w:rPr><m:t>a &amp; &lt;b&gt;</m:t></m:r><!--c--></m:oMath>"#;
        let root = parse(xml).unwrap();
        assert!(root.is("urn:m", "oMath"));
        let run = root.child("urn:m", "r").unwrap();
        let color = run
            .child("urn:w", "rPr")
            .and_then(|p| p.child("urn:w", "color"))
            .unwrap();
        assert_eq!(color.attr("urn:w", "val"), Some("FF0000"));
        assert_eq!(run.text(), "a & <b>");
        let out = serialize(&root);
        assert_eq!(parse(&out).unwrap(), root);
        assert!(out.contains("a &amp; &lt;b&gt;") && out.contains("<!--c-->"));
        assert_eq!(root.declared_prefix("urn:w"), Some("w"));
    }

    #[test]
    fn default_and_redeclared_namespaces() {
        let root = parse(r#"<math xmlns="urn:a"><x:y xmlns:x="urn:b"><z/></x:y></math>"#).unwrap();
        assert!(root.is("urn:a", "math"));
        let y = root.elements().next().unwrap();
        assert!(y.is("urn:b", "y"));
        assert!(y.elements().next().unwrap().is("urn:a", "z"));
    }

    #[test]
    fn rejects_doctypes_and_malformed_input() {
        let bomb = r#"<!DOCTYPE x [<!ENTITY a "aaaa">]><x>&a;</x>"#;
        assert!(parse(bomb)
            .unwrap_err()
            .to_string()
            .contains("document type"));
        assert!(parse("<a><b></a>").is_err());
        assert!(parse("<a/><b/>").is_err());
        assert!(parse("").is_err());
        assert!(parse("<a>&unknown;</a>").is_err());
    }
}
