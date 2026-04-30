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
