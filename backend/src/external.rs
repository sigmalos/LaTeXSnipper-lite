//! OpenAI-compatible vision endpoint (OpenAI, Ollama `/v1`, LM Studio, vLLM…).

use crate::ocr::Mode;
use crate::store::Settings;
use anyhow::{anyhow, bail, Result};
use base64::Engine as _;
use serde_json::{json, Value};
use std::time::Duration;

pub fn prompt(mode: Mode) -> &'static str {
    match mode {
        Mode::Formula => "Recognize the visible formula or math-heavy content in the image. Return directly usable LaTeX only. Preserve nearby labels or text when they are part of the expression. Do not explain, summarize, add code fences, or add prefixes.",
        Mode::Mixed => "Recognize the visible page region and return Markdown only. Preserve reading order, headings, paragraphs, lists, tables, and line breaks. Use $...$ for inline formulas and $$...$$ for display formulas. Do not explain, summarize, add code fences, or add prefixes.",
        Mode::Text => "Recognize the visible text in the image and return plain text only. Preserve reading order and line breaks. Do not explain, summarize, add code fences, or add prefixes.",
    }
}

fn base_url(s: &Settings) -> Result<String> {
    let url = s.external_base_url.trim().trim_end_matches('/');
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        bail!("external model base URL must start with http:// or https://");
    }
    if s.external_model.trim().is_empty() {
        bail!("external model name is required");
    }
    Ok(if url.ends_with("/v1") {
        url.to_string()
    } else {
        format!("{url}/v1")
    })
}

fn agent(s: &Settings) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(s.external_timeout_sec))
        .build()
}

fn request(s: &Settings, req: ureq::Request) -> ureq::Request {
    let key = s.external_api_key.trim();
    let req = req.set("Content-Type", "application/json");
    if key.is_empty() {
        req
    } else {
        req.set("Authorization", &format!("Bearer {key}"))
    }
}

/// Strip ``` fences and $$ wrappers models like to add despite instructions.
pub fn clean_output(text: &str, mode: Mode) -> String {
    let mut t = text.trim();
    if t.starts_with("```") {
        t = t.trim_start_matches('`');
        t = t.split_once('\n').map_or("", |(_, rest)| rest);
        t = t.trim_end().trim_end_matches('`').trim();
    }
    if mode == Mode::Formula {
        for (open, close) in [("$$", "$$"), ("\\[", "\\]"), ("$", "$")] {
            if t.len() > open.len() + close.len() && t.starts_with(open) && t.ends_with(close) {
                t = t[open.len()..t.len() - close.len()].trim();
                break;
            }
        }
    }
    t.to_string()
}

fn content_of(raw: &Value) -> Result<String> {
    let content = &raw["choices"][0]["message"]["content"];
    let text = match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join(""),
        _ => return Err(anyhow!("response has no message content")),
    };
    Ok(text)
}

pub fn recognize(s: &Settings, png: &[u8], mode: Mode) -> Result<String> {
    let url = format!("{}/chat/completions", base_url(s)?);
    let prompt = if s.external_custom_prompt.trim().is_empty() {
        prompt(mode)
    } else {
        s.external_custom_prompt.trim()
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(png);
    let body = json!({
        "model": s.external_model.trim(),
        "temperature": 0,
        "messages": [{"role": "user", "content": [
            {"type": "text", "text": prompt},
            {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{b64}")}}
        ]}]
    });
    let raw: Value = match request(s, agent(s).post(&url)).send_json(body) {
        Ok(r) => r.into_json()?,
        Err(ureq::Error::Status(code, r)) => bail!(
            "HTTP {code}: {}",
            r.into_string()
                .unwrap_or_default()
                .chars()
                .take(300)
                .collect::<String>()
        ),
        Err(e) => return Err(e.into()),
    };
    let text = clean_output(&content_of(&raw)?, mode);
    if text.is_empty() {
        bail!("external model returned empty content");
    }
    Ok(text)
}

pub fn test_connection(s: &Settings) -> Result<String> {
    let url = format!("{}/models", base_url(s)?);
    match request(s, agent(s).get(&url)).call() {
        Ok(r) => {
            let raw: Value = r.into_json().unwrap_or(Value::Null);
            let n = raw["data"].as_array().map_or(0, Vec::len);
            Ok(format!("OK ({n} models)"))
        }
        Err(ureq::Error::Status(code, _)) => bail!("HTTP {code}"),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(url: &str) -> Settings {
        Settings {
            external_base_url: url.into(),
            external_model: "m".into(),
            external_api_key: "k".into(),
            ..Settings::default()
        }
    }

    #[test]
    fn cleans_fences_and_math_wrappers() {
        assert_eq!(clean_output("```latex\nx^2\n```", Mode::Formula), "x^2");
        assert_eq!(clean_output("$$ \\frac12 $$", Mode::Formula), "\\frac12");
        assert_eq!(clean_output("\\[a\\]", Mode::Formula), "a");
        assert_eq!(clean_output("$a$ and $b$", Mode::Mixed), "$a$ and $b$");
    }

    #[test]
    fn validates_base_url() {
        assert!(base_url(&settings("ftp://x")).is_err());
        assert_eq!(base_url(&settings("http://h:1/")).unwrap(), "http://h:1/v1");
        assert_eq!(
            base_url(&settings("http://h:1/v1")).unwrap(),
            "http://h:1/v1"
        );
        let mut s = settings("http://h");
        s.external_model = " ".into();
        assert!(base_url(&s).is_err());
    }

    #[test]
    fn calls_chat_completions_against_mock_server() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let addr = server.server_addr().to_ip().unwrap();
        let handle = std::thread::spawn(move || {
            let mut req = server.recv().unwrap();
            assert_eq!(req.url(), "/v1/chat/completions");
            let auth = req
                .headers()
                .iter()
                .find(|h| h.field.equiv("Authorization"))
                .unwrap()
                .value
                .to_string();
            assert_eq!(auth, "Bearer k");
            let mut body = String::new();
            req.as_reader().read_to_string(&mut body).unwrap();
            let v: Value = serde_json::from_str(&body).unwrap();
            assert!(v["messages"][0]["content"][1]["image_url"]["url"]
                .as_str()
                .unwrap()
                .starts_with("data:image/png;base64,"));
            let reply = r#"{"choices":[{"message":{"content":"```\n$$E=mc^2$$\n```"}}]}"#;
            req.respond(tiny_http::Response::from_string(reply))
                .unwrap();
        });
        let out = recognize(&settings(&format!("http://{addr}")), b"png", Mode::Formula).unwrap();
        handle.join().unwrap();
        assert_eq!(out, "E=mc^2");
    }

    #[test]
    fn surfaces_http_errors() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let addr = server.server_addr().to_ip().unwrap();
        let handle = std::thread::spawn(move || {
            let req = server.recv().unwrap();
            req.respond(tiny_http::Response::from_string("bad key").with_status_code(401))
                .unwrap();
        });
        let err = test_connection(&settings(&format!("http://{addr}")))
            .unwrap_err()
            .to_string();
        handle.join().unwrap();
        assert!(err.contains("401"), "{err}");
    }
}
