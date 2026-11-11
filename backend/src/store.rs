//! On-disk state: settings, history and favorites.
//!
//! `history.json` / `favorites.json` keep the exact schema of the original
//! Qt app so existing user data under `~/Library/Application Support/LaTeXSnipper`
//! loads without migration.

use crate::ocr::Mode;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub mode: Mode,
    /// "builtin" (MathCraft ONNX) or "external" (OpenAI-compatible endpoint)
    pub backend: String,
    /// "auto" | "light" | "dark"
    pub theme: String,
    /// "zh" | "en"
    pub language: String,
    pub hotkey: String,
    pub history_reverse: bool,
    pub pdf_dpi: u32,
    pub external_base_url: String,
    pub external_model: String,
    pub external_api_key: String,
    pub external_timeout_sec: u64,
    pub external_custom_prompt: String,
    pub onboarding_done: bool,
    /// Schema version. A missing field must read as 0 (not the struct
    /// default) so files written before migrations existed get upgraded.
    #[serde(default)]
    pub version: u32,
}

/// ⌘F, matching the original app's macOS default.
pub const DEFAULT_HOTKEY: &str = "CommandOrControl+F";
/// Default of pre-release builds; never chosen by the user, so it is migrated.
const LEGACY_DEFAULT_HOTKEY: &str = "CommandOrControl+Alt+F";
pub const SETTINGS_VERSION: u32 = 1;

/// Upgrade settings read from disk. Returns whether anything changed.
pub fn migrate(s: &mut Settings) -> bool {
    if s.version >= SETTINGS_VERSION {
        return false;
    }
    if s.hotkey == LEGACY_DEFAULT_HOTKEY {
        s.hotkey = DEFAULT_HOTKEY.into();
    }
    s.version = SETTINGS_VERSION;
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: Mode::Formula,
            backend: "builtin".into(),
            theme: "auto".into(),
            language: "zh".into(),
            hotkey: DEFAULT_HOTKEY.into(),
            history_reverse: true,
            pdf_dpi: 200,
            external_base_url: "http://localhost:11434/v1".into(),
            external_model: String::new(),
            external_api_key: String::new(),
            external_timeout_sec: 60,
            external_custom_prompt: String::new(),
            onboarding_done: false,
            version: SETTINGS_VERSION,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<()> {
        if !["builtin", "external"].contains(&self.backend.as_str()) {
            bail!("unknown backend {:?}", self.backend);
        }
        if !["auto", "light", "dark"].contains(&self.theme.as_str()) {
            bail!("unknown theme {:?}", self.theme);
        }
        if !["zh", "en"].contains(&self.language.as_str()) {
            bail!("unknown language {:?}", self.language);
        }
        if !(72..=600).contains(&self.pdf_dpi) {
            bail!("PDF DPI must be between 72 and 600");
        }
        if self.external_timeout_sec == 0 || self.external_timeout_sec > 600 {
            bail!("timeout must be between 1 and 600 seconds");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Entry {
    pub text: String,
    #[serde(default)]
    pub name: String,
    /// "mathcraft" | "mathcraft_mixed" | "mathcraft_text"
    pub kind: String,
}

pub fn kind_for(mode: Mode) -> &'static str {
    match mode {
        Mode::Formula => "mathcraft",
        Mode::Mixed => "mathcraft_mixed",
        Mode::Text => "mathcraft_text",
    }
}

fn normalize_kind(kind: &str) -> String {
    match kind.trim().to_lowercase().as_str() {
        "mathcraft_mixed" => "mathcraft_mixed",
        "mathcraft_text" => "mathcraft_text",
        _ => "mathcraft",
    }
    .into()
}

pub const HISTORY_LIMIT: usize = 500;

pub struct Store {
    dir: PathBuf,
    pub settings: Settings,
    pub history: Vec<Entry>,
    pub favorites: Vec<Entry>,
}

/// Write via a temp file + rename so a crash never leaves truncated JSON.
fn write_atomic(path: &Path, contents: &str, private: bool) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, contents)?;
    #[cfg(not(unix))]
    let _ = private;
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(tmp, path)?;
    Ok(())
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn load_entries(path: &Path, list_key: &str, names_key: &str, types_key: &str) -> Vec<Entry> {
    let Some(v) = read_json(path) else {
        return vec![];
    };
    let names = v
        .get(names_key)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let types = v
        .get(types_key)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    v.get(list_key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|t| t.as_str().map(str::to_string))
        .map(|text| Entry {
            name: names
                .get(&text)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            kind: normalize_kind(
                types
                    .get(&text)
                    .and_then(Value::as_str)
                    .unwrap_or("mathcraft"),
            ),
            text,
        })
        .collect()
}

fn dump_entries(entries: &[Entry], list_key: &str, names_key: &str, types_key: &str) -> String {
    let mut names = Map::new();
    let mut types = Map::new();
    for e in entries {
        if !e.name.is_empty() {
            names.insert(e.text.clone(), json!(e.name));
        }
        types.insert(e.text.clone(), json!(e.kind));
    }
    let list: Vec<&str> = entries.iter().map(|e| e.text.as_str()).collect();
    serde_json::to_string_pretty(&json!({ list_key: list, names_key: names, types_key: types }))
        .unwrap()
}

impl Store {
    pub fn open(dir: &Path) -> Self {
        let mut settings = read_json(&dir.join("settings.json"))
            .and_then(|v| serde_json::from_value::<Settings>(v).ok())
            .filter(|s| s.validate().is_ok())
            .unwrap_or_default();
        if migrate(&mut settings) {
            // Best effort: the in-memory value is already correct for this run.
            let _ = serde_json::to_string_pretty(&settings)
                .map_err(anyhow::Error::from)
                .and_then(|json| write_atomic(&dir.join("settings.json"), &json, true));
        }
        Self {
            dir: dir.to_path_buf(),
            settings,
            history: load_entries(
                &dir.join("history.json"),
                "history",
                "formula_names",
                "formula_types",
            ),
            favorites: load_entries(&dir.join("favorites.json"), "favorites", "names", "types"),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn models_dir(&self) -> PathBuf {
        self.dir.join("MathCraft").join("models")
    }

    pub fn save_settings(&mut self, s: Settings) -> Result<()> {
        s.validate()?;
        write_atomic(
            &self.dir.join("settings.json"),
            &serde_json::to_string_pretty(&s)?,
            true,
        )?;
        self.settings = s;
        Ok(())
    }

    fn save_history(&self) -> Result<()> {
        write_atomic(
            &self.dir.join("history.json"),
            &dump_entries(&self.history, "history", "formula_names", "formula_types"),
            false,
        )
    }

    fn save_favorites(&self) -> Result<()> {
        write_atomic(
            &self.dir.join("favorites.json"),
            &dump_entries(&self.favorites, "favorites", "names", "types"),
            false,
        )
    }

    /// Append (moving an existing duplicate to the end, keeping its name).
    pub fn add_history(&mut self, text: &str, kind: &str) -> Result<()> {
        let text = text.trim();
        if text.is_empty() {
            bail!("empty content");
        }
        let name = match self.history.iter().position(|e| e.text == text) {
            Some(i) => self.history.remove(i).name,
            None => String::new(),
        };
        self.history.push(Entry {
            text: text.into(),
            name,
            kind: normalize_kind(kind),
        });
        if self.history.len() > HISTORY_LIMIT {
            let excess = self.history.len() - HISTORY_LIMIT;
            self.history.drain(..excess);
        }
        self.save_history()
    }

    pub fn remove_history(&mut self, index: usize) -> Result<()> {
        if index >= self.history.len() {
            bail!("history index out of range");
        }
        self.history.remove(index);
        self.save_history()
    }

    pub fn rename_history(&mut self, index: usize, name: &str) -> Result<()> {
        let e = self
            .history
            .get_mut(index)
            .ok_or_else(|| anyhow::anyhow!("history index out of range"))?;
        e.name = name.trim().to_string();
        self.save_history()
    }

    pub fn clear_history(&mut self) -> Result<()> {
        self.history.clear();
        self.save_history()
    }

    pub fn add_favorite(&mut self, text: &str, kind: &str, name: &str) -> Result<bool> {
        let text = text.trim();
        if text.is_empty() {
            bail!("empty content");
        }
        if self.favorites.iter().any(|e| e.text == text) {
            return Ok(false);
        }
        self.favorites.push(Entry {
            text: text.into(),
            name: name.trim().into(),
            kind: normalize_kind(kind),
        });
        self.save_favorites()?;
        Ok(true)
    }

    pub fn remove_favorite(&mut self, index: usize) -> Result<()> {
        if index >= self.favorites.len() {
            bail!("favorite index out of range");
        }
        self.favorites.remove(index);
        self.save_favorites()
    }

    pub fn rename_favorite(&mut self, index: usize, name: &str) -> Result<()> {
        let e = self
            .favorites
            .get_mut(index)
            .ok_or_else(|| anyhow::anyhow!("favorite index out of range"))?;
        e.name = name.trim().to_string();
        self.save_favorites()
    }

    pub fn clear_favorites(&mut self) -> Result<()> {
        self.favorites.clear();
        self.save_favorites()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_legacy_qt_history_format() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("history.json"),
            r#"{"history":["a+b","x"],"formula_names":{"x":"ex"},"formula_types":{"a+b":"mathcraft_mixed","x":"bogus"}}"#,
        )
        .unwrap();
        let store = Store::open(dir.path());
        assert_eq!(store.history.len(), 2);
        assert_eq!(store.history[0].kind, "mathcraft_mixed");
        assert_eq!(
            store.history[1],
            Entry {
                text: "x".into(),
                name: "ex".into(),
                kind: "mathcraft".into()
            }
        );
    }

    #[test]
    fn history_roundtrip_dedupes_and_limits() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path());
        s.add_history("a", "mathcraft").unwrap();
        s.add_history("b", "mathcraft_text").unwrap();
        s.rename_history(0, " first ").unwrap();
        s.add_history("a", "mathcraft").unwrap();
        assert_eq!(
            s.history
                .iter()
                .map(|e| e.text.as_str())
                .collect::<Vec<_>>(),
            ["b", "a"]
        );
        assert_eq!(s.history[1].name, "first");
        assert!(s.add_history("  ", "mathcraft").is_err());
        let reloaded = Store::open(dir.path());
        assert_eq!(reloaded.history, s.history);
        for i in 0..HISTORY_LIMIT + 5 {
            s.add_history(&i.to_string(), "mathcraft").unwrap();
        }
        assert_eq!(s.history.len(), HISTORY_LIMIT);
        s.remove_history(0).unwrap();
        assert!(s.remove_history(9999).is_err());
        s.clear_history().unwrap();
        assert!(Store::open(dir.path()).history.is_empty());
    }

    #[test]
    fn favorites_reject_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path());
        assert!(s.add_favorite("e^x", "mathcraft", "exp").unwrap());
        assert!(!s.add_favorite("e^x", "mathcraft", "").unwrap());
        s.rename_favorite(0, "Euler").unwrap();
        let raw: Value = read_json(&dir.path().join("favorites.json")).unwrap();
        assert_eq!(raw["names"]["e^x"], "Euler");
        s.remove_favorite(0).unwrap();
        assert!(s.favorites.is_empty());
        s.add_favorite("y", "mathcraft", "").unwrap();
        s.clear_favorites().unwrap();
        assert!(Store::open(dir.path()).favorites.is_empty());
    }

    #[test]
    fn settings_are_validated_and_private() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path());
        assert_eq!(s.settings, Settings::default());
        let bad = Settings {
            theme: "neon".into(),
            ..Settings::default()
        };
        assert!(s.save_settings(bad).is_err());
        let good = Settings {
            mode: Mode::Mixed,
            external_api_key: "sk".into(),
            ..Settings::default()
        };
        s.save_settings(good.clone()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let path = dir.path().join("settings.json");
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_eq!(Store::open(dir.path()).settings, good);
    }

    #[test]
    fn default_hotkey_is_command_f() {
        assert_eq!(Settings::default().hotkey, "CommandOrControl+F");
    }

    #[test]
    fn migrates_the_pre_release_default_hotkey_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(
            &path,
            r#"{"hotkey":"CommandOrControl+Alt+F","onboarding_done":true}"#,
        )
        .unwrap();
        let s = Store::open(dir.path());
        assert_eq!(s.settings.hotkey, DEFAULT_HOTKEY);
        assert!(s.settings.onboarding_done);
        let raw: Value = read_json(&path).unwrap();
        assert_eq!(raw["hotkey"], DEFAULT_HOTKEY);
        assert_eq!(raw["version"], SETTINGS_VERSION);

        // A user who deliberately picks ⌥⌘F afterwards keeps it.
        let mut store = Store::open(dir.path());
        let chosen = Settings {
            hotkey: LEGACY_DEFAULT_HOTKEY.into(),
            ..store.settings.clone()
        };
        store.save_settings(chosen).unwrap();
        assert_eq!(
            Store::open(dir.path()).settings.hotkey,
            LEGACY_DEFAULT_HOTKEY
        );
    }

    #[test]
    fn migration_keeps_custom_hotkeys() {
        let mut s = Settings {
            hotkey: "Control+Shift+K".into(),
            version: 0,
            ..Settings::default()
        };
        assert!(migrate(&mut s));
        assert_eq!(s.hotkey, "Control+Shift+K");
        assert!(!migrate(&mut s));
    }

    #[test]
    fn corrupt_files_fall_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("settings.json"), "{not json").unwrap();
        std::fs::write(dir.path().join("history.json"), "[]").unwrap();
        let s = Store::open(dir.path());
        assert_eq!(s.settings, Settings::default());
        assert!(s.history.is_empty());
    }
}
