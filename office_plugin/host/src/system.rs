//! Native services that go through macOS command-line tools: Word document
//! variables (AppleScript, from the original host's `word_variables.py`) and
//! the JSON open/save panels for typography presets (`dialogs.py`).

use crate::api::ApiError;
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const OSASCRIPT: &str = "/usr/bin/osascript";
pub const MAX_VARIABLES: usize = 50;
pub const MAX_VALUE_LENGTH: usize = 60_000;
const WORD_TIMEOUT: Duration = Duration::from_secs(30);

/// Arguments: marker, then name/value pairs. Errors 1001 and -1743 are mapped below.
pub const WORD_VARIABLES_SCRIPT: &str = r#"
on run argv
  set marker to item 1 of argv
  tell application id "com.microsoft.Word"
    repeat with documentIndex from 1 to (count of documents)
      set wordDocument to document documentIndex
      if (exists bookmark marker of wordDocument) then
        repeat with pairIndex from 2 to (count of argv) by 2
          set variableName to item pairIndex of argv
          set variableValue to item (pairIndex + 1) of argv
          if (exists variable variableName of wordDocument) then
            set variable value of variable variableName of wordDocument to variableValue
          else
            make new variable at wordDocument with properties {name:variableName, variable value:variableValue}
          end if
        end repeat
        return "ok"
      end if
    end repeat
  end tell
  error "marker not found" number 1001
end run
"#;

pub const OPEN_JSON_SCRIPT: &str = r#"
on run argv
  activate
  try
    set chosen to choose file with prompt (item 1 of argv) of type {"public.json", "json"} default location (path to documents folder)
  on error number -128
    return ""
  end try
  return POSIX path of chosen
end run
"#;

pub const SAVE_JSON_SCRIPT: &str = r#"
on run argv
  activate
  try
    set chosen to choose file name with prompt (item 1 of argv) default name (item 2 of argv) default location (path to documents folder)
  on error number -128
    return ""
  end try
  return POSIX path of chosen
end run
"#;

pub struct Output {
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// Runs an AppleScript with arguments; `None` status means it timed out.
pub trait ScriptRunner: Send + Sync {
    fn run(
        &self,
        script: &str,
        args: &[String],
        timeout: Option<Duration>,
    ) -> std::io::Result<Output>;
}

pub struct Osascript;

impl ScriptRunner for Osascript {
    fn run(
        &self,
        script: &str,
        args: &[String],
        timeout: Option<Duration>,
    ) -> std::io::Result<Output> {
        let mut child = Command::new(OSASCRIPT)
            .arg("-e")
            .arg(script)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let started = Instant::now();
        loop {
            if child.try_wait()?.is_some() {
                let out = child.wait_with_output()?;
                return Ok(Output {
                    status: out.status.code(),
                    stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
                });
            }
            if timeout.is_some_and(|t| started.elapsed() > t) {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(Output {
                    status: None,
                    stdout: String::new(),
                    stderr: String::new(),
                });
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn is_hex(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn is_marker(m: &str) -> bool {
    m.strip_prefix("_LSLink")
        .is_some_and(|hex| (16..=32).contains(&hex.len()) && is_hex(hex))
}

fn is_variable_name(n: &str) -> bool {
    if n == "LaTeXSnipper.DocumentId" {
        return true;
    }
    let Some(rest) = n.strip_prefix("LS.E.") else {
        return false;
    };
    let Some((id, revision)) = rest.split_once('.') else {
        return false;
    };
    id.len() == 32 && is_hex(id) && revision.len() == 10 && is_hex(revision)
}

fn invalid(message: &str) -> ApiError {
    ApiError::new(400, "invalid_request", message)
}

/// Only plugin variable names, bounded values, a well-formed document marker.
pub fn validate_variables(
    marker: &Value,
    values: &Value,
) -> Result<Vec<(String, String)>, ApiError> {
    marker
        .as_str()
        .filter(|m| is_marker(m))
        .ok_or_else(|| invalid("文档标记无效。"))?;
    let values: &Map<String, Value> = values
        .as_object()
        .filter(|v| (1..=MAX_VARIABLES).contains(&v.len()))
        .ok_or_else(|| invalid("文档变量无效。"))?;
    let mut pairs = Vec::new();
    for (name, value) in values {
        let Some(value) = value.as_str().filter(|_| is_variable_name(name)) else {
            return Err(invalid("文档变量无效。"));
        };
        if value.is_empty() || value.chars().count() > MAX_VALUE_LENGTH || value.contains('\0') {
            return Err(invalid("文档变量内容无效。"));
        }
        pairs.push((name.clone(), value.to_string()));
    }
    Ok(pairs)
}

pub fn write_word_variables(
    runner: &dyn ScriptRunner,
    marker: &Value,
    values: &Value,
) -> Result<Value, ApiError> {
    let pairs = validate_variables(marker, values)?;
    let mut args = vec![marker.as_str().unwrap_or_default().to_string()];
    for (name, value) in &pairs {
        args.push(name.clone());
        args.push(value.clone());
    }
    let out = runner
        .run(WORD_VARIABLES_SCRIPT, &args, Some(WORD_TIMEOUT))
        .map_err(|_| ApiError::new(502, "word_variables_failed", "Word 未能保存公式数据。"))?;
    match out.status {
        None => Err(ApiError::new(504, "word_timeout", "Word 未及时响应，公式数据未写入。")),
        Some(0) => Ok(json!({"written": pairs.len()})),
        Some(_) if out.stderr.contains("-1743") => Err(ApiError::new(
            403,
            "word_automation_denied",
            "macOS 未允许 LaTeXSnipper Office 控制 Microsoft Word。请在“系统设置 › 隐私与安全性 › 自动化”中允许后重试。",
        )),
        Some(_) if out.stderr.contains("1001") => {
            Err(ApiError::new(409, "word_document_unknown", "无法定位当前 Word 文档，请重试。"))
        }
        Some(_) => {
            crate::paths::log(format!("osascript failed: {}", out.stderr.trim()));
            Err(ApiError::new(502, "word_variables_failed", "Word 未能保存公式数据。"))
        }
    }
}

/// Native JSON file panels; `Ok(None)` when the user cancels.
pub trait FileChooser: Send + Sync {
    fn open_json(&self, title: &str) -> Result<Option<PathBuf>, ApiError>;
    fn save_json(&self, title: &str, suggested: &str) -> Result<Option<PathBuf>, ApiError>;
}

pub struct ScriptFileChooser<R: ScriptRunner>(pub R);

fn chosen_path(out: std::io::Result<Output>) -> Result<Option<PathBuf>, ApiError> {
    let unavailable = || ApiError::new(503, "dialog_unavailable", "当前无法打开文件对话框。");
    let out = out.map_err(|_| unavailable())?;
    if out.status != Some(0) {
        return Err(unavailable());
    }
    let path = out.stdout.trim_end_matches(['\n', '\r']);
    Ok((!path.is_empty()).then(|| PathBuf::from(path)))
}

impl<R: ScriptRunner> FileChooser for ScriptFileChooser<R> {
    fn open_json(&self, title: &str) -> Result<Option<PathBuf>, ApiError> {
        chosen_path(self.0.run(OPEN_JSON_SCRIPT, &[title.to_string()], None))
    }

    fn save_json(&self, title: &str, suggested: &str) -> Result<Option<PathBuf>, ApiError> {
        let path = chosen_path(self.0.run(
            SAVE_JSON_SCRIPT,
            &[title.to_string(), suggested.to_string()],
            None,
        ))?;
        Ok(path.map(|p| {
            if p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("json"))
            {
                p
            } else {
                p.with_extension("json")
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct Recorder {
        pub calls: Mutex<Vec<Vec<String>>>,
        pub reply: Mutex<Option<(Option<i32>, String, String)>>,
    }

    impl ScriptRunner for Recorder {
        fn run(
            &self,
            _script: &str,
            args: &[String],
            _t: Option<Duration>,
        ) -> std::io::Result<Output> {
            self.calls.lock().unwrap().push(args.to_vec());
            let (status, stdout, stderr) = self.reply.lock().unwrap().clone().unwrap_or((
                Some(0),
                "ok\n".into(),
                String::new(),
            ));
            Ok(Output {
                status,
                stdout,
                stderr,
            })
        }
    }

    const MARKER: &str = "_LSLink0123456789abcdef";
    fn name() -> String {
        format!("LS.E.{}.{}", "a".repeat(32), "0123456789")
    }

    #[test]
    fn validates_markers_names_and_values() {
        let ok = json!({ name(): "{}", "LaTeXSnipper.DocumentId": "doc" });
        assert_eq!(validate_variables(&json!(MARKER), &ok).unwrap().len(), 2);
        for marker in [
            json!("_LSLinkXYZ"),
            json!("_LSLink0123"),
            json!(1),
            json!(format!("_LSLink{}", "a".repeat(33))),
        ] {
            assert!(validate_variables(&marker, &ok).is_err(), "{marker}");
        }
        for values in [
            json!({}),
            json!({"Other": "x"}),
            json!({ format!("LS.E.{}.0123456789", "A".repeat(32)): "x" }),
            json!({ name(): "" }),
            json!({ name(): 3 }),
            json!({ name(): "x".repeat(MAX_VALUE_LENGTH + 1) }),
        ] {
            assert!(
                validate_variables(&json!(MARKER), &values).is_err(),
                "{values}"
            );
        }
        let many: Map<String, Value> = (0..51)
            .map(|i| (format!("LS.E.{}.{:010}", "b".repeat(32), i), json!("x")))
            .collect();
        assert!(validate_variables(&json!(MARKER), &Value::Object(many)).is_err());
    }

    #[test]
    fn writes_through_osascript_and_maps_errors() {
        let r = Recorder::default();
        let values = json!({ name(): "{\"a\":1}" });
        assert_eq!(
            write_word_variables(&r, &json!(MARKER), &values).unwrap(),
            json!({"written": 1})
        );
        assert_eq!(
            r.calls.lock().unwrap()[0],
            [MARKER.to_string(), name(), "{\"a\":1}".to_string()]
        );
        for (reply, status) in [
            (
                (
                    Some(1),
                    String::new(),
                    "execution error: Not authorized (-1743)".to_string(),
                ),
                403,
            ),
            (
                (
                    Some(1),
                    String::new(),
                    "marker not found (1001)".to_string(),
                ),
                409,
            ),
            ((Some(1), String::new(), "boom".to_string()), 502),
            ((None, String::new(), String::new()), 504),
        ] {
            *r.reply.lock().unwrap() = Some(reply);
            assert_eq!(
                write_word_variables(&r, &json!(MARKER), &values)
                    .unwrap_err()
                    .status,
                status
            );
        }
    }

    #[test]
    fn file_panels_return_paths_and_add_the_json_suffix() {
        let chooser = ScriptFileChooser(Recorder::default());
        *chooser.0.reply.lock().unwrap() =
            Some((Some(0), "/v1ykl8/preset\n".into(), String::new()));
        assert_eq!(
            chooser.save_json("t", "a.json").unwrap(),
            Some(PathBuf::from("/v1ykl8/preset.json"))
        );
        *chooser.0.reply.lock().unwrap() = Some((Some(0), "\n".into(), String::new()));
        assert_eq!(chooser.open_json("t").unwrap(), None);
        *chooser.0.reply.lock().unwrap() = Some((Some(1), String::new(), "err".into()));
        assert_eq!(chooser.open_json("t").unwrap_err().status, 503);
    }

    /// The panel scripts compile. The Word script is not compiled here: resolving
    /// its terminology can launch Word.
    #[cfg(target_os = "macos")]
    #[test]
    fn applescripts_compile() {
        let dir = tempfile::tempdir().unwrap();
        for (i, script) in [OPEN_JSON_SCRIPT, SAVE_JSON_SCRIPT].iter().enumerate() {
            let out = Command::new("/usr/bin/osacompile")
                .arg("-o")
                .arg(dir.path().join(format!("{i}.scpt")))
                .arg("-e")
                .arg(script)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
}
