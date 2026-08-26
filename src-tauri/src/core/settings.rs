//! Settings persistence: a single JSON file at `<config>/unflick/settings.json`.
//! Supports full-blob writes (used by GUI) and per-key updates (used by CLI/MCP).

use std::path::PathBuf;

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

/// Environment override for the settings directory.
///
/// The integration tests need this for the same reason they need
/// `UNFLICK_DATA_DIR`: settings.json holds keybindings, subtitle styling
/// and SponsorBlock preferences, and a test run must not rewrite the ones
/// the developer is actually using — nor race another test doing the same.
pub const CONFIG_DIR_ENV: &str = "UNFLICK_CONFIG_DIR";

/// Directory holding unflick's settings file.
pub fn config_dir() -> PathBuf {
    if let Some(dir) = std::env::var(CONFIG_DIR_ENV)
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return PathBuf::from(dir);
    }
    dirs_next::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("unflick")
}

/// Absolute path to the settings file.
pub fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
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

/// Merge a JSON object into the stored settings, key by key.
///
/// This exists because settings.json has three writers. The GUI's settings
/// panel sends a blob built from the fields it models; the CLI and MCP server
/// write keys it has never heard of (keybindings, mouse bindings, the
/// OpenSubtitles key). Writing the blob wholesale deleted every one of those
/// the first time a user changed any setting in the window - silently, and
/// without anything to point at afterwards.
pub fn merge(incoming: &Value) -> Result<()> {
    let mut all = read_all()?;
    merge_into(&mut all, incoming)?;
    write_all(&all)
}

/// The merge itself, split out so it can be tested without a config
/// directory - `CONFIG_DIR_ENV` is process-global and the unit tests run in
/// parallel against one process.
///
/// Top-level only. Nothing stored here needs deep merging, and a deep merge
/// would make removing a key from a nested object impossible: the settings
/// panel could then never turn a keybinding off.
pub fn merge_into(base: &mut Value, incoming: &Value) -> Result<()> {
    let incoming = incoming
        .as_object()
        .ok_or_else(|| anyhow!("settings payload must be a JSON object"))?;
    let target = base
        .as_object_mut()
        .ok_or_else(|| anyhow!("settings is not an object"))?;
    for (key, value) in incoming {
        target.insert(key.clone(), value.clone());
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_preserves_keys_the_caller_does_not_know_about() {
        // The regression: the GUI sends the fields it models, and the CLI's
        // keybindings used to vanish with the first save.
        let mut stored = json!({
            "keybindings": {"space": "play_pause"},
            "opensubtitles_api_key": "secret",
            "volume": 40
        });
        let from_gui = json!({"volume": 80, "theme": "dark"});

        merge_into(&mut stored, &from_gui).unwrap();

        assert_eq!(stored["keybindings"]["space"], "play_pause");
        assert_eq!(stored["opensubtitles_api_key"], "secret");
        assert_eq!(stored["volume"], 80, "incoming value should win");
        assert_eq!(stored["theme"], "dark", "new keys should be added");
    }

    #[test]
    fn merge_replaces_nested_objects_wholesale() {
        // Deliberate: a deep merge would make it impossible to remove a
        // single binding, since an absent key would just keep the old value.
        let mut stored = json!({"keybindings": {"space": "play_pause", "f": "fullscreen"}});
        merge_into(&mut stored, &json!({"keybindings": {"space": "play_pause"}})).unwrap();

        assert!(stored["keybindings"].get("f").is_none());
    }

    #[test]
    fn merge_rejects_non_objects() {
        let mut stored = json!({});
        assert!(merge_into(&mut stored, &json!([1, 2, 3])).is_err());
        assert!(merge_into(&mut stored, &json!("nope")).is_err());
    }

    #[test]
    fn merge_into_empty_settings_is_just_the_payload() {
        let mut stored = json!({});
        merge_into(&mut stored, &json!({"a": 1})).unwrap();
        assert_eq!(stored, json!({"a": 1}));
    }
}
