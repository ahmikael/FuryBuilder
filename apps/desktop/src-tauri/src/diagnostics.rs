//! Local, bounded diagnostics for crash investigation.
//!
//! The log is JSON Lines under the application data directory. It intentionally records
//! stable events and short messages rather than world payloads, tokens, MCP sockets or
//! absolute paths: user-provided text is sanitized and path-like substrings are redacted.
//! Files rotate at a fixed size so diagnostics cannot grow without bound.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{json, Map, Value};

pub const LOG_FILE: &str = "furybuilder.log";
const LOG_PREFIX: &str = "furybuilder";
const LOG_SUFFIX: &str = ".log";
const DEFAULT_MAX_BYTES: u64 = 2 * 1024 * 1024;
const DEFAULT_MAX_FILES: usize = 4;
const MAX_TEXT_CHARS: usize = 1_000;
const MAX_FIELD_CHARS: usize = 64;
const MAX_ARRAY_ITEMS: usize = 32;
const MAX_OBJECT_FIELDS: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticLogFile {
    pub name: String,
    pub bytes: u64,
    pub modified_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsInfo {
    pub app_version: String,
    pub log_dir: String,
    pub file_name: String,
    pub files: Vec<DiagnosticLogFile>,
    pub total_bytes: u64,
    pub max_bytes_per_file: u64,
    pub max_files: usize,
}

/// Thread-safe append-only diagnostics sink. Cloning shares the same directory and lock.
#[derive(Clone)]
pub struct Diagnostics {
    inner: Arc<Inner>,
}

struct Inner {
    dir: PathBuf,
    max_bytes: u64,
    max_files: usize,
    lock: Mutex<()>,
}

impl Diagnostics {
    pub fn new(dir: PathBuf) -> Self {
        Self::with_limits(dir, DEFAULT_MAX_BYTES, DEFAULT_MAX_FILES)
    }

    pub fn with_limits(dir: PathBuf, max_bytes: u64, max_files: usize) -> Self {
        let _ = fs::create_dir_all(&dir);
        Self {
            inner: Arc::new(Inner {
                dir,
                max_bytes: max_bytes.max(256),
                max_files: max_files.max(1),
                lock: Mutex::new(()),
            }),
        }
    }

    /// Appends one structured record. Failures are ignored: diagnostics must never make the
    /// editor or a project operation fail.
    pub fn write(&self, level: &str, event: &str, fields: Value) {
        let record = json!({
            "tsMs": now_ms(),
            "level": normalize_level(level),
            "event": sanitize_token(event),
            "appVersion": env!("CARGO_PKG_VERSION"),
            "data": sanitize_json(&fields, 0),
        });
        let Ok(mut line) = serde_json::to_vec(&record) else {
            return;
        };
        line.push(b'\n');
        let guard = self
            .inner
            .lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _ = self.append_locked(&line);
        drop(guard);
    }

    /// Records a startup marker with no user-specific paths.
    pub fn record_startup(&self) {
        self.write(
            "info",
            "app.start",
            json!({
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "debug": cfg!(debug_assertions),
            }),
        );
    }

    /// Installs a panic hook that appends a final local record before the process exits.
    /// The previous hook is preserved so the console still shows the usual message.
    pub fn install_panic_hook(&self) {
        let diagnostics = self.clone();
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            diagnostics.write(
                "fatal",
                "app.panic",
                json!({
                    "message": info.to_string(),
                    "thread": std::thread::current().name().unwrap_or("unnamed"),
                }),
            );
            previous(info);
        }));
    }

    pub fn info(&self) -> DiagnosticsInfo {
        let mut files = Vec::new();
        let mut total_bytes = 0u64;
        if let Ok(entries) = fs::read_dir(&self.inner.dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                if !is_log_name(name) {
                    continue;
                }
                let Ok(metadata) = entry.metadata() else {
                    continue;
                };
                if !metadata.is_file() {
                    continue;
                }
                let bytes = metadata.len();
                total_bytes = total_bytes.saturating_add(bytes);
                files.push(DiagnosticLogFile {
                    name: name.to_owned(),
                    bytes,
                    modified_at_ms: modified_at_ms(&metadata),
                });
            }
        }
        files.sort_by(|left, right| left.name.cmp(&right.name));
        DiagnosticsInfo {
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            log_dir: self.inner.dir.display().to_string(),
            file_name: LOG_FILE.to_owned(),
            files,
            total_bytes,
            max_bytes_per_file: self.inner.max_bytes,
            max_files: self.inner.max_files,
        }
    }

    /// Removes only FuryBuilder's own log files. Unrelated files in the log directory are
    /// left untouched so this action is safe even if the directory was customized.
    pub fn clear(&self) -> Result<DiagnosticsInfo, String> {
        let guard = self
            .inner
            .lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        fs::create_dir_all(&self.inner.dir)
            .map_err(|error| format!("creating the diagnostics directory failed: {error}"))?;
        let entries = fs::read_dir(&self.inner.dir)
            .map_err(|error| format!("reading the diagnostics directory failed: {error}"))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let is_ours = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(is_log_name);
            if is_ours && path.is_file() {
                fs::remove_file(&path)
                    .map_err(|error| format!("removing a diagnostics file failed: {error}"))?;
            }
        }
        drop(guard);
        Ok(self.info())
    }

    /// Opens the log directory with the platform file manager. Only the fixed application
    /// directory is passed; the caller cannot influence the path.
    pub fn open_folder(&self) -> Result<(), String> {
        fs::create_dir_all(&self.inner.dir)
            .map_err(|error| format!("creating the diagnostics directory failed: {error}"))?;
        #[cfg(target_os = "windows")]
        let mut command = {
            let mut command = Command::new("explorer.exe");
            command.arg(&self.inner.dir);
            command
        };
        #[cfg(target_os = "macos")]
        let mut command = {
            let mut command = Command::new("open");
            command.arg(&self.inner.dir);
            command
        };
        #[cfg(all(unix, not(target_os = "macos")))]
        let mut command = {
            let mut command = Command::new("xdg-open");
            command.arg(&self.inner.dir);
            command
        };
        command
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("opening the diagnostics directory failed: {error}"))
    }

    fn append_locked(&self, line: &[u8]) -> std::io::Result<()> {
        fs::create_dir_all(&self.inner.dir)?;
        self.rotate_if_needed(line.len() as u64)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.inner.dir.join(LOG_FILE))?;
        file.write_all(line)?;
        file.flush()
    }

    fn rotate_if_needed(&self, additional: u64) -> std::io::Result<()> {
        let base = self.inner.dir.join(LOG_FILE);
        let current = fs::metadata(&base).map(|meta| meta.len()).unwrap_or(0);
        if current == 0 || current.saturating_add(additional) <= self.inner.max_bytes {
            return Ok(());
        }
        if self.inner.max_files <= 1 {
            let _ = fs::remove_file(&base);
            return Ok(());
        }
        let oldest = self.archive_path(self.inner.max_files - 1);
        let _ = fs::remove_file(oldest);
        for index in (1..self.inner.max_files - 1).rev() {
            let from = self.archive_path(index);
            let to = self.archive_path(index + 1);
            if from.exists() {
                let _ = fs::rename(&from, &to);
            }
        }
        let first = self.archive_path(1);
        if first.exists() {
            let _ = fs::remove_file(&first);
        }
        fs::rename(&base, &first)
    }

    fn archive_path(&self, index: usize) -> PathBuf {
        self.inner
            .dir
            .join(format!("{LOG_PREFIX}.{index}{LOG_SUFFIX}"))
    }
}

fn normalize_level(level: &str) -> &'static str {
    match level {
        "fatal" => "fatal",
        "error" => "error",
        "warn" | "warning" => "warn",
        "debug" => "debug",
        _ => "info",
    }
}

fn is_log_name(name: &str) -> bool {
    if name == LOG_FILE {
        return true;
    }
    name.strip_prefix(&format!("{LOG_PREFIX}."))
        .and_then(|rest| rest.strip_suffix(LOG_SUFFIX))
        .is_some_and(|digits| !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_digit()))
}

fn modified_at_ms(metadata: &fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

fn sanitize_token(raw: &str) -> String {
    let mut out: String = raw
        .chars()
        .take(MAX_FIELD_CHARS)
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | ':') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() {
        out.push_str("unknown");
    }
    out
}

fn sanitize_json(value: &Value, depth: usize) -> Value {
    if depth > 4 {
        return Value::String("<nested>".to_owned());
    }
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        Value::String(text) => Value::String(sanitize_text(text)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .take(MAX_ARRAY_ITEMS)
                .map(|item| sanitize_json(item, depth + 1))
                .collect(),
        ),
        Value::Object(map) => {
            let mut sanitized = Map::new();
            for (key, item) in map.iter().take(MAX_OBJECT_FIELDS) {
                let key = sanitize_token(key);
                let value = if key == "source" {
                    match item {
                        Value::String(text) => Value::String(sanitize_token(text)),
                        other => sanitize_json(other, depth + 1),
                    }
                } else {
                    sanitize_json(item, depth + 1)
                };
                sanitized.insert(key, value);
            }
            Value::Object(sanitized)
        }
    }
}

fn sanitize_text(raw: &str) -> String {
    let mut cleaned = String::with_capacity(raw.len().min(4_096));
    let mut last_was_space = false;
    for ch in raw.chars().take(MAX_TEXT_CHARS.saturating_mul(2)) {
        if ch.is_control() || ch == '\u{2028}' || ch == '\u{2029}' {
            if !last_was_space {
                cleaned.push(' ');
                last_was_space = true;
            }
        } else {
            cleaned.push(ch);
            last_was_space = false;
        }
    }
    let redacted = redact_paths(cleaned.trim());
    let mut capped: String = redacted.chars().take(MAX_TEXT_CHARS).collect();
    if redacted.chars().count() > MAX_TEXT_CHARS {
        capped.push_str("...");
    }
    capped
}

fn redact_paths(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut output = String::with_capacity(input.len());
    let mut index = 0;
    while index < chars.len() {
        if path_starts_at(&chars, index) {
            output.push_str("<path>");
            index = path_end(&chars, index);
        } else {
            output.push(chars[index]);
            index += 1;
        }
    }
    output
}

fn path_starts_at(chars: &[char], index: usize) -> bool {
    let boundary = index == 0 || is_path_boundary(chars[index - 1]);
    if !boundary {
        return false;
    }
    let rest: String = chars[index..].iter().take(8).collect();
    if rest.starts_with("\\\\") {
        return true;
    }
    if rest.starts_with("/home/") || rest.starts_with("/Users/") || rest.starts_with("/root/") {
        return true;
    }
    let mut iter = chars[index..].iter();
    matches!(
        (iter.next(), iter.next(), iter.next()),
        (Some(drive), Some(':'), Some('/' | '\\')) if drive.is_ascii_alphabetic()
    )
}

fn is_path_boundary(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | ',' | ';' | '=' | ':'
        )
}

fn path_end(chars: &[char], start: usize) -> usize {
    let mut index = start;
    while index < chars.len() {
        let ch = chars[index];
        if ch.is_control()
            || matches!(
                ch,
                '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | ',' | ';' | '='
            )
        {
            break;
        }
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "furybuilder-diagnostics-{label}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn writes_json_lines_with_redacted_paths_and_controls() {
        let dir = temp_dir("sanitize");
        let diagnostics = Diagnostics::with_limits(dir.clone(), 64 * 1024, 3);
        diagnostics.write(
            "error",
            "ui.error",
            json!({
                "message": "failed reading C:\\Users\\Mike\\Private World\\level.dat\nnext line",
                "source": "project open",
            }),
        );
        let raw = fs::read_to_string(dir.join(LOG_FILE)).expect("log");
        assert!(raw.contains("\"event\":\"ui.error\""));
        assert!(raw.contains("<path>"));
        assert!(!raw.contains("Private World"));
        assert!(!raw.contains("\\nnext line"));
        assert!(raw.contains("\"source\":\"project_open\""));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rotates_and_bounds_log_files() {
        let dir = temp_dir("rotate");
        let diagnostics = Diagnostics::with_limits(dir.clone(), 300, 3);
        for index in 0..20 {
            diagnostics.write("info", "app.test", json!({ "index": index }));
        }
        let info = diagnostics.info();
        assert!(info.files.len() <= 3);
        assert!(info.total_bytes <= 300 * 3);
        let newest = fs::read_to_string(dir.join(LOG_FILE)).expect("newest log");
        assert!(newest.contains("\"index\":19"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn clear_removes_only_owned_logs() {
        let dir = temp_dir("clear");
        let diagnostics = Diagnostics::with_limits(dir.clone(), 64 * 1024, 3);
        diagnostics.write("info", "app.test", json!({ "message": "first" }));
        let unrelated = dir.join("notes.txt");
        fs::write(&unrelated, b"keep me").expect("unrelated");
        let info = diagnostics.clear().expect("clear");
        assert!(info.files.is_empty());
        assert!(unrelated.exists());
        let _ = fs::remove_dir_all(dir);
    }
}
