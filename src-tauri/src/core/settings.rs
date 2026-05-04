//! Settings persistence: a single JSON file at `<config>/unflick/settings.json`.
//! Supports full-blob writes (used by GUI) and per-key updates (used by CLI/MCP).

use std::path::PathBuf;

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

/// Absolute path to the settings file.
pub fn settings_path() -> PathBuf {
    dirs_next::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("unflick")
        .join("settings.json")
}

/// Read the entire settings blob. Returns an empty object if the file is missing.
pub fn read_all() -> Result<Value> {
    let path = settings_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            if content.trim().is_empty() {
                return Ok(json!({}));
            }
            serde_json::from_str(&content)
                .map_err(|e| anyhow!("settings.json is not valid JSON: {}", e))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(anyhow!("failed to read settings: {}", e)),
    }
}

/// Replace the entire settings blob. Validates that input is a JSON object.
pub fn write_all(value: &Value) -> Result<()> {
    if !value.is_object() {
        bail!("settings must be a JSON object at the top level");
    }
    let path = settings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow!("failed to create settings dir: {}", e))?;
    }
    let pretty = serde_json::to_string_pretty(value).unwrap();
    std::fs::write(&path, pretty)
        .map_err(|e| anyhow!("failed to write settings: {}", e))?;
    Ok(())
}

/// Get a single key. Returns `None` if absent.
pub fn get(key: &str) -> Result<Option<Value>> {
    let all = read_all()?;
    Ok(all.get(key).cloned())
}

/// Set a single key to the given JSON value.
pub fn set(key: &str, value: Value) -> Result<()> {
    let mut all = read_all()?;
    let obj = all.as_object_mut().ok_or_else(|| anyhow!("settings is not an object"))?;
    obj.insert(key.to_string(), value);
    write_all(&all)
}

/// Remove a single key. Returns whether the key existed.
pub fn unset(key: &str) -> Result<bool> {
    let mut all = read_all()?;
    let obj = all.as_object_mut().ok_or_else(|| anyhow!("settings is not an object"))?;
    let removed = obj.remove(key).is_some();
    write_all(&all)?;
    Ok(removed)
}

// ─── Streaming settings (v0.9 P1) ─────────────────────────────────────────────
//
// `preferred_quality` and `cookies_browser` are user-tunable knobs for the
// URL extractor. They live in the same `settings.json` blob as everything
// else and are read on demand by `extract_stream_url`.
//
// Both are stored as strings; both are optional. Existing settings files
// that pre-date these keys keep working — `read_all` returns whatever is on
// disk, and these helpers gracefully return `None` when the key is missing
// or set to a sentinel value (`"auto"` for quality, `"none"` for cookies).

/// Allowed values for `preferred_quality`. `"auto"` and `None` both mean
/// "let yt-dlp pick". The numeric variants cap by height; `"audio_only"`
/// downloads audio only.
pub const QUALITY_VALUES: &[&str] = &[
    "auto", "2160p", "1440p", "1080p", "720p", "480p", "audio_only",
];

/// Allowed values for `cookies_browser`. `"none"` and `None` both mean
/// "don't pass `--cookies-from-browser`".
pub const COOKIES_BROWSER_VALUES: &[&str] = &[
    "none", "firefox", "chrome", "chromium", "safari", "edge", "brave",
];

/// Read `preferred_quality`. Returns `None` if missing, empty, or `"auto"`.
pub fn preferred_quality() -> Option<String> {
    let v = get("preferred_quality").ok().flatten()?;
    let s = v.as_str()?.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("auto") {
        return None;
    }
    Some(s.to_string())
}

/// Read `cookies_browser`. Returns `None` if missing, empty, or `"none"`.
pub fn cookies_browser() -> Option<String> {
    let v = get("cookies_browser").ok().flatten()?;
    let s = v.as_str()?.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("none") {
        return None;
    }
    Some(s.to_string())
}
