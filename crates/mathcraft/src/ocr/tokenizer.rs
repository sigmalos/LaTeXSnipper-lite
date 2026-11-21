//! Minimal byte-level BPE *decoder* for the formula model's `tokenizer.json`.
//!
//! Recognition only ever turns ids back into text, so the full `tokenizers`
//! crate (and its native regex dependencies) is unnecessary.

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub struct Tokenizer {
    id_to_token: HashMap<u32, String>,
    special: HashSet<u32>,
    byte_decoder: HashMap<char, u8>,
    clean_up_spaces: bool,
}

/// GPT-2 `bytes_to_unicode` table, inverted.
fn byte_decoder() -> HashMap<char, u8> {
    let mut bs: Vec<u32> = (b'!' as u32..=b'~' as u32)
        .chain(0xA1..=0xAC)
        .chain(0xAE..=0xFF)
        .collect();
    let mut cs = bs.clone();
    let mut n = 0;
    for b in 0..256u32 {
        if !bs.contains(&b) {
            bs.push(b);
            cs.push(256 + n);
            n += 1;
        }
    }
    bs.into_iter()
        .zip(cs)
        .map(|(b, c)| (char::from_u32(c).unwrap(), b as u8))
        .collect()
}

impl Tokenizer {
    pub fn from_dir(dir: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(dir.join("tokenizer.json"))
            .context("missing tokenizer.json")?;
        let json: Value = serde_json::from_str(raw.trim_start_matches('\u{feff}'))?;
        let clean_up_spaces = std::fs::read_to_string(dir.join("tokenizer_config.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(s.trim_start_matches('\u{feff}')).ok())
            .and_then(|v| {
                v.get("clean_up_tokenization_spaces")
                    .and_then(Value::as_bool)
            })
            .unwrap_or(true);
        Self::from_json(&json, clean_up_spaces)
    }

    pub fn from_json(json: &Value, clean_up_spaces: bool) -> Result<Self> {
        let vocab = json
            .pointer("/model/vocab")
            .and_then(Value::as_object)
            .ok_or_else(|| anyhow!("tokenizer.json has no BPE vocab"))?;
        let mut id_to_token: HashMap<u32, String> = vocab
            .iter()
            .filter_map(|(tok, id)| Some((id.as_u64()? as u32, tok.clone())))
            .collect();
        let mut special = HashSet::new();
        for added in json
            .get("added_tokens")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let (Some(id), Some(content)) = (added["id"].as_u64(), added["content"].as_str())
            else {
                continue;
            };
            id_to_token.insert(id as u32, content.to_string());
            if added["special"].as_bool().unwrap_or(false) {
                special.insert(id as u32);
            }
        }
        Ok(Self {
            id_to_token,
            special,
            byte_decoder: byte_decoder(),
            clean_up_spaces,
        })
    }

    pub fn token_id(&self, content: &str) -> Option<u32> {
        self.id_to_token
            .iter()
            .find(|(_, t)| t.as_str() == content)
            .map(|(id, _)| *id)
    }

    /// Equivalent of `tokenizer.decode(ids, skip_special_tokens=True)`.
    pub fn decode(&self, ids: &[u32]) -> String {
        let mut bytes = Vec::new();
        for id in ids {
            if self.special.contains(id) {
                continue;
            }
            let Some(token) = self.id_to_token.get(id) else {
                continue;
            };
            for ch in token.chars() {
                match self.byte_decoder.get(&ch) {
                    Some(b) => bytes.push(*b),
                    None => bytes.extend(ch.to_string().as_bytes()),
                }
            }
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        if self.clean_up_spaces {
            clean_up_tokenization(&text)
        } else {
            text
        }
    }
}

/// English punctuation cleanup applied by the original TrOCR tokenizer.
pub fn clean_up_tokenization(text: &str) -> String {
    text.replace(" .", ".")
        .replace(" ?", "?")
        .replace(" !", "!")
        .replace(" ,", ",")
        .replace(" ' ", "'")
        .replace(" n't", "n't")
        .replace(" 'm", "'m")
        .replace(" 's", "'s")
        .replace(" 've", "'ve")
        .replace(" 're", "'re")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Tokenizer {
        let json = json!({
            "added_tokens": [
                {"id": 0, "content": "<pad>", "special": true},
                {"id": 2, "content": "</s>", "special": true}
            ],
            "model": {"type": "BPE", "vocab": {
                "<pad>": 0, "</s>": 2, "\\frac": 5, "Ġ{": 6, "a": 7, "}": 8, "Ġ,": 9, "Ã©": 10
            }}
        });
        Tokenizer::from_json(&json, true).unwrap()
    }

    #[test]
    fn decodes_byte_level_spaces_and_skips_specials() {
        let t = sample();
        assert_eq!(t.decode(&[5, 6, 7, 8, 2, 0]), "\\frac {a}");
    }

    #[test]
    fn applies_cleanup_and_utf8() {
        let t = sample();
        assert_eq!(t.decode(&[7, 9]), "a,");
        assert_eq!(t.decode(&[10]), "é");
    }

    #[test]
    fn looks_up_ids() {
        assert_eq!(sample().token_id("</s>"), Some(2));
        assert_eq!(sample().token_id("nope"), None);
    }

    #[test]
    fn byte_table_is_bijective() {
        let table = byte_decoder();
        assert_eq!(table.len(), 256);
        assert_eq!(table[&'Ġ'], b' ');
        assert_eq!(table[&'!'], b'!');
    }
}
