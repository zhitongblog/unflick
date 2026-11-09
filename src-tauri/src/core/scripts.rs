//! User scripts for mpv (`.lua` and `.js`).
//!
//! Scripts live in `<config_dir>/scripts/` (respecting `UNFLICK_CONFIG_DIR`).
//! Dropping a `.lua` or `.js` file into that folder enables it by default,
//! matching mpv's own `~/.config/mpv/scripts/` convention. Disabled scripts
//! are recorded by filename under `"disabled_scripts"` in `settings.json` so
//! the user can toggle a script off without deleting the file.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::player::Player;
use super::settings;

const DISABLED_KEY: &str = "disabled_scripts";

/// Metadata for one installed mpv user script.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScriptEntry {
    /// Filename with extension, e.g. `"autocrop.lua"`.
    pub name: String,
    /// Absolute path on disk.
    pub path: String,
    /// `"lua"` or `"js"`.
    pub kind: String,
    /// Whether this script is loaded on startup.
    pub enabled: bool,
    /// File size in bytes.
    pub size: u64,
    /// First comment line at the top of the file, when present.
    pub description: Option<String>,
}

/// Directory holding installed mpv user scripts.
pub fn scripts_dir() -> PathBuf {
    settings::config_dir().join("scripts")
}

fn disabled_set() -> HashSet<String> {
    settings::get(DISABLED_KEY)
        .ok()
        .flatten()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect()
}

fn save_disabled_set(set: &HashSet<String>) -> Result<()> {
    let mut sorted: Vec<&String> = set.iter().collect();
    sorted.sort();
    settings::set(DISABLED_KEY, json!(sorted))
}

fn script_kind(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "lua" => Some("lua"),
        "js" => Some("js"),
        _ => None,
    }
}

/// Extract a human-readable summary from the first leading comment line
/// (`-- ...` in Lua, `// ...` in JS) if present.
fn extract_description(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    for line in text.lines().take(10) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("#!") {
            continue;
        }
        let body = if let Some(rest) = trimmed.strip_prefix("---") {
            Some(rest)
        } else if let Some(rest) = trimmed.strip_prefix("--") {
            Some(rest)
        } else if let Some(rest) = trimmed.strip_prefix("//") {
            Some(rest)
        } else {
            return None;
        };
        if let Some(comment) = body {
            let clean = comment.trim().trim_matches('-').trim_matches('*').trim();
            if !clean.is_empty() {
                let mut desc = clean.to_string();
                if desc.chars().count() > 120 {
                    desc = desc.chars().take(117).collect::<String>() + "…";
                }
                return Some(desc);
            }
        }
    }
    None
}

/// List scripts in `dir` given the set of disabled script filenames.
pub fn list_in(dir: &Path, disabled: &HashSet<String>) -> Result<Vec<ScriptEntry>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    let read_dir = std::fs::read_dir(dir)
        .map_err(|e| anyhow!("failed to read scripts directory {}: {}", dir.display(), e))?;
    for item in read_dir.flatten() {
        let path = item.path();
        if !path.is_file() {
            continue;
        }
        let Some(kind) = script_kind(&path) else {
            continue;
        };
        let Some(name) = path.file_name().and_then(|n| n.to_str()).map(String::from) else {
            continue;
        };
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let description = extract_description(&path);
        let enabled = !disabled.contains(&name);
        entries.push(ScriptEntry {
            name,
            path: path.to_string_lossy().into_owned(),
            kind: kind.to_string(),
            enabled,
            size,
            description,
        });
    }
    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(entries)
}

/// List all user scripts in `<config_dir>/scripts/`.
pub fn list() -> Result<Vec<ScriptEntry>> {
    list_in(&scripts_dir(), &disabled_set())
}

/// Resolve a script name (`"autocrop.lua"` or stem `"autocrop"`) against a
/// slice of installed scripts.
fn find_entry<'a>(entries: &'a [ScriptEntry], name: &str) -> Result<&'a ScriptEntry> {
    let target = name.trim();
    if target.is_empty() {
        bail!("script name is required");
    }
    if let Some(exact) = entries
        .iter()
        .find(|e| e.name.eq_ignore_ascii_case(target))
    {
        return Ok(exact);
    }
    let stem_matches: Vec<&ScriptEntry> = entries
        .iter()
        .filter(|e| {
            Path::new(&e.name)
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case(target))
                .unwrap_or(false)
        })
        .collect();
    match stem_matches.len() {
        1 => Ok(stem_matches[0]),
        0 => bail!("no script named {:?}", target),
        _ => bail!(
            "ambiguous script name {:?} — include the extension (.lua or .js)",
            target
        ),
    }
}

/// Install a `.lua` or `.js` script into `<config_dir>/scripts/`, enable it,
/// and load it into the running player if one is provided.
pub fn add(source_path: &str, player: Option<&Player>) -> Result<ScriptEntry> {
    let src = Path::new(source_path.trim());
    if !src.is_file() {
        bail!("script file not found: {}", source_path);
    }
    if script_kind(src).is_none() {
        bail!(
            "unsupported script extension for {} (expected .lua or .js)",
            source_path
        );
    }
    let file_name = src
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow!("invalid script filename: {}", source_path))?
        .to_string();

    let dir = scripts_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| anyhow!("failed to create scripts dir {}: {}", dir.display(), e))?;
    let dest = dir.join(&file_name);
    if src != dest {
        std::fs::copy(src, &dest).map_err(|e| {
            anyhow!(
                "failed to copy {} to {}: {}",
                src.display(),
                dest.display(),
                e
            )
        })?;
    }

    let mut disabled = disabled_set();
    if disabled.remove(&file_name) {
        let _ = save_disabled_set(&disabled);
    }

    let dest_str = dest.to_string_lossy().into_owned();
    if let Some(p) = player {
        let _ = p.load_script(&dest_str);
    }

    let entries = list_in(&dir, &disabled)?;
    find_entry(&entries, &file_name).cloned()
}

/// Enable or disable an installed script. Enabling a script also loads it into
/// the running player immediately; disabling takes effect on the next launch
/// (mpv has no `unload-script` command).
pub fn set_enabled(name: &str, enabled: bool, player: Option<&Player>) -> Result<ScriptEntry> {
    let dir = scripts_dir();
    let mut disabled = disabled_set();
    let entries = list_in(&dir, &disabled)?;
    let target = find_entry(&entries, name)?.clone();

    if enabled {
        disabled.remove(&target.name);
    } else {
        disabled.insert(target.name.clone());
    }
    save_disabled_set(&disabled)?;

    if enabled {
        if let Some(p) = player {
            let _ = p.load_script(&target.path);
        }
    }

    let mut updated = target;
    updated.enabled = enabled;
    Ok(updated)
}

/// Remove an installed script from `<config_dir>/scripts/`.
pub fn remove(name: &str) -> Result<ScriptEntry> {
    let dir = scripts_dir();
    let mut disabled = disabled_set();
    let entries = list_in(&dir, &disabled)?;
    let target = find_entry(&entries, name)?.clone();

    std::fs::remove_file(&target.path)
        .map_err(|e| anyhow!("failed to remove {}: {}", target.path, e))?;

    if disabled.remove(&target.name) {
        let _ = save_disabled_set(&disabled);
    }

    Ok(target)
}

/// Load every enabled script in `<config_dir>/scripts/` into `player`.
pub fn load_enabled_into(player: &Player) {
    let Ok(entries) = list() else {
        return;
    };
    for entry in entries.into_iter().filter(|e| e.enabled) {
        if let Err(e) = player.load_script(&entry.path) {
            eprintln!("[unflick] failed to load script {}: {}", entry.name, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_lua_and_js_scripts_with_descriptions_and_disabled_state() {
        let dir = std::env::temp_dir().join(format!(
            "unflick-scripts-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(
            dir.join("autocrop.lua"),
            "-- Automatically crops black bars\nmp.register_event('file-loaded', function() end)\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("clock.js"),
            "// Shows wall clock on OSD\nmp.osd_message('12:00');\n",
        )
        .unwrap();
        std::fs::write(dir.join("notes.txt"), "ignored").unwrap();

        let mut disabled = HashSet::new();
        disabled.insert("clock.js".to_string());

        let list = list_in(&dir, &disabled).unwrap();
        assert_eq!(list.len(), 2);

        assert_eq!(list[0].name, "autocrop.lua");
        assert_eq!(list[0].kind, "lua");
        assert!(list[0].enabled);
        assert_eq!(
            list[0].description.as_deref(),
            Some("Automatically crops black bars")
        );

        assert_eq!(list[1].name, "clock.js");
        assert_eq!(list[1].kind, "js");
        assert!(!list[1].enabled);
        assert_eq!(list[1].description.as_deref(), Some("Shows wall clock on OSD"));

        // Stem lookup works alongside exact name.
        assert_eq!(find_entry(&list, "autocrop").unwrap().name, "autocrop.lua");
        assert_eq!(find_entry(&list, "clock.js").unwrap().name, "clock.js");
        assert!(find_entry(&list, "missing").is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
