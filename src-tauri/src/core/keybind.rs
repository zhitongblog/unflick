//! Keyboard bindings: the catalogue of bindable actions, and the user's
//! overrides on top of it.
//!
//! Shortcuts used to be a `switch` in `App.tsx`, which meant they couldn't
//! be listed, couldn't be changed, and existed only in the GUI. A PotPlayer
//! user — where a hundred-plus keys are rebindable — hits that wall on
//! their first wrong keypress.
//!
//! The catalogue lives here rather than in the frontend so `unflick keybind
//! list` and the settings panel describe the same set, and so the defaults
//! have one definition. Overrides are stored in `settings.json` under
//! `keybindings` as `{ action_id: "key" }` — only what the user changed,
//! so a later release can move a default without stranding anyone on the
//! old one.
//!
//! ## Key syntax
//!
//! `Mod+Alt+Shift+key`, in that order. `Mod` is Ctrl on Windows and Linux,
//! Cmd on macOS — one binding set covers all three rather than shipping a
//! per-platform table that drifts.
//!
//! Single-character keys are lower-case and Shift is explicit, so `z` and
//! `Shift+z` are distinct and neither is ambiguous. Named keys keep their
//! DOM spelling (`ArrowLeft`, `PageUp`, `Space`).

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// One bindable action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    /// Stable identifier. Never renamed — it's what overrides are keyed by.
    pub id: &'static str,
    /// Default key, in the syntax above.
    pub default: &'static str,
    /// English label. The GUI prefers its own translation keyed by `id` and
    /// falls back to this.
    pub label: &'static str,
    /// Grouping for the settings UI.
    pub group: &'static str,
}

/// Every action a key can be bound to.
///
/// `Escape` is deliberately absent: it means "dismiss whatever is open",
/// which is a UI convention rather than a player action, and rebinding it
/// would leave dialogs with no way out.
pub const ACTIONS: &[Action] = &[
    // ── Playback ──
    Action { id: "play_pause",        default: "Space",       label: "Play / pause",            group: "playback" },
    Action { id: "seek_back",         default: "ArrowLeft",   label: "Back 5 seconds",          group: "playback" },
    Action { id: "seek_forward",      default: "ArrowRight",  label: "Forward 5 seconds",       group: "playback" },
    Action { id: "frame_back",        default: ",",           label: "Previous frame",          group: "playback" },
    Action { id: "frame_forward",     default: ".",           label: "Next frame",              group: "playback" },
    Action { id: "speed_down",        default: "Shift+,",     label: "Slower",                  group: "playback" },
    Action { id: "speed_up",          default: "Shift+.",     label: "Faster",                  group: "playback" },
    Action { id: "speed_down_fine",   default: "Alt+,",       label: "Slower (fine)",           group: "playback" },
    Action { id: "speed_up_fine",     default: "Alt+.",       label: "Faster (fine)",           group: "playback" },
    Action { id: "speed_reset",       default: "Backspace",   label: "Normal speed",            group: "playback" },
    // ── Volume ──
    Action { id: "volume_up",         default: "ArrowUp",     label: "Volume up",               group: "volume" },
    Action { id: "volume_down",       default: "ArrowDown",   label: "Volume down",             group: "volume" },
    Action { id: "mute",              default: "m",           label: "Mute",                    group: "volume" },
    // ── Chapters and loops ──
    Action { id: "chapter_next",      default: "PageUp",      label: "Next chapter",            group: "navigation" },
    Action { id: "chapter_prev",      default: "PageDown",    label: "Previous chapter",        group: "navigation" },
    Action { id: "loop_a",            default: "[",           label: "Set loop start",          group: "navigation" },
    Action { id: "loop_b",            default: "]",           label: "Set loop end",            group: "navigation" },
    Action { id: "loop_clear",        default: "\\",          label: "Clear loop",              group: "navigation" },
    Action { id: "bookmark_add",      default: "b",           label: "Bookmark this moment",    group: "navigation" },
    Action { id: "toggle_bookmarks",  default: "Shift+b",     label: "Bookmarks",               group: "navigation" },
    // ── Subtitles and audio ──
    Action { id: "sub_delay_down",    default: "z",           label: "Subtitle delay −",        group: "tracks" },
    Action { id: "sub_delay_up",      default: "Shift+z",     label: "Subtitle delay +",        group: "tracks" },
    Action { id: "audio_delay_down",  default: "Mod+-",       label: "Audio delay −",           group: "tracks" },
    Action { id: "audio_delay_up",    default: "Mod+=",       label: "Audio delay +",           group: "tracks" },
    // ── Window ──
    Action { id: "fullscreen",        default: "f",           label: "Fullscreen",              group: "window" },
    Action { id: "pip",               default: "p",           label: "Picture in picture",      group: "window" },
    Action { id: "music_mode",        default: "Mod+m",       label: "Music mode",              group: "window" },
    Action { id: "toggle_library",    default: "l",           label: "Library",                 group: "window" },
    Action { id: "toggle_playlist",   default: "n",           label: "Playlist",                group: "window" },
    // ── Capture ──
    Action { id: "screenshot",        default: "s",           label: "Screenshot",              group: "capture" },
    Action { id: "clip",              default: "c",           label: "Clip",                    group: "capture" },
    // ── Application ──
    Action { id: "open_file",         default: "Mod+o",       label: "Open file",               group: "app" },
    Action { id: "open_url",          default: "Mod+u",       label: "Open URL",                group: "app" },
    Action { id: "settings",          default: "Mod+,",       label: "Settings",                group: "app" },
    Action { id: "incognito",         default: "Mod+Shift+p", label: "Incognito mode",          group: "app" },
];

const SETTINGS_KEY: &str = "keybindings";

pub fn find_action(id: &str) -> Option<&'static Action> {
    ACTIONS.iter().find(|a| a.id == id)
}

/// The full binding table: every action with its effective key, its
/// default, and whether the user has changed it.
pub fn list() -> Result<Value> {
    let overrides = read_overrides()?;
    let rows: Vec<Value> = ACTIONS
        .iter()
        .map(|a| {
            let custom = overrides.get(a.id).and_then(|v| v.as_str());
            json!({
                "id": a.id,
                "label": a.label,
                "group": a.group,
                "key": custom.unwrap_or(a.default),
                "default": a.default,
                "customized": custom.is_some(),
            })
        })
        .collect();
    Ok(Value::Array(rows))
}

/// Bind `key` to `action`.
///
/// Rejects a key already taken by a different action: silently stealing it
/// would leave the previous one dead with no indication why.
pub fn set(action: &str, key: &str) -> Result<String> {
    let Some(target) = find_action(action) else {
        bail!("unknown action: {} (see `unflick keybind list`)", action);
    };
    let normalized = normalize(key)?;

    let table = list()?;
    if let Some(rows) = table.as_array() {
        for row in rows {
            let id = row["id"].as_str().unwrap_or("");
            if id != action && row["key"].as_str() == Some(normalized.as_str()) {
                bail!(
                    "{} is already bound to \"{}\" — rebind or reset that first",
                    normalized,
                    row["label"].as_str().unwrap_or(id)
                );
            }
        }
    }

    let mut overrides = read_overrides()?;
    if normalized == target.default {
        // Back to the default: drop the override rather than storing a
        // value equal to it, so `customized` stays truthful.
        overrides.remove(action);
    } else {
        overrides.insert(action.to_string(), Value::String(normalized.clone()));
    }
    write_overrides(overrides)?;
    Ok(normalized)
}

/// Reset one action, or every action when `action` is `None`.
pub fn reset(action: Option<&str>) -> Result<usize> {
    match action {
        Some(id) => {
            if find_action(id).is_none() {
                bail!("unknown action: {}", id);
            }
            let mut overrides = read_overrides()?;
            let existed = overrides.remove(id).is_some();
            write_overrides(overrides)?;
            Ok(usize::from(existed))
        }
        None => {
            let count = read_overrides()?.len();
            write_overrides(Map::new())?;
            Ok(count)
        }
    }
}

fn read_overrides() -> Result<Map<String, Value>> {
    let all = super::settings::read_all()?;
    Ok(all
        .get(SETTINGS_KEY)
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

fn write_overrides(overrides: Map<String, Value>) -> Result<()> {
    if overrides.is_empty() {
        // Don't leave an empty object behind — an untouched install should
        // have no `keybindings` key at all.
        let _ = super::settings::unset(SETTINGS_KEY);
        return Ok(());
    }
    super::settings::set(SETTINGS_KEY, Value::Object(overrides))
}

/// Canonicalise a key string, or explain why it isn't one.
///
/// Accepts the modifiers in any order and any case; emits them in a fixed
/// order so `shift+CTRL+A` and `Ctrl+Shift+a` can't both end up stored as
/// separate bindings for the same physical chord.
pub fn normalize(raw: &str) -> Result<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        bail!("key is empty");
    }

    let mut mods = (false, false, false); // (mod, alt, shift)
    let mut key: Option<String> = None;

    // A lone "+" is a key, not a separator, so split carefully.
    let parts: Vec<&str> = if raw == "+" {
        vec!["+"]
    } else {
        raw.split('+').filter(|p| !p.is_empty()).collect()
    };

    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "mod" | "ctrl" | "control" | "cmd" | "command" | "meta" => mods.0 = true,
            "alt" | "option" => mods.1 = true,
            "shift" => mods.2 = true,
            _ => {
                if key.is_some() {
                    bail!("\"{}\" names more than one key", raw);
                }
                key = Some(canonical_key(part));
            }
        }
    }

    let Some(key) = key else {
        bail!("\"{}\" is only modifiers — it needs a key too", raw);
    };

    let mut out = String::new();
    if mods.0 {
        out.push_str("Mod+");
    }
    if mods.1 {
        out.push_str("Alt+");
    }
    if mods.2 {
        out.push_str("Shift+");
    }
    out.push_str(&key);
    Ok(out)
}

/// Single characters go lower-case (Shift is carried as a modifier
/// instead); named keys get their DOM spelling back, so a user typing
/// `pageup` still matches what the browser reports.
fn canonical_key(part: &str) -> String {
    if part.chars().count() == 1 {
        return part.to_lowercase();
    }
    const NAMED: &[&str] = &[
        "ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown",
        "PageUp", "PageDown", "Home", "End", "Insert", "Delete",
        "Backspace", "Enter", "Tab", "Space", "Escape",
        "F1", "F2", "F3", "F4", "F5", "F6",
        "F7", "F8", "F9", "F10", "F11", "F12",
    ];
    NAMED
        .iter()
        .find(|n| n.eq_ignore_ascii_case(part))
        .map(|n| n.to_string())
        .unwrap_or_else(|| part.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifier_order_and_case_are_canonical() {
        assert_eq!(normalize("Ctrl+o").unwrap(), "Mod+o");
        assert_eq!(normalize("shift+CTRL+A").unwrap(), "Mod+Shift+a");
        assert_eq!(normalize("Cmd+Shift+P").unwrap(), "Mod+Shift+p");
        // Any spelling of the platform modifier folds to the same chord.
        assert_eq!(normalize("meta+u").unwrap(), normalize("ctrl+u").unwrap());
    }

    #[test]
    fn named_keys_get_their_dom_spelling() {
        assert_eq!(normalize("pageup").unwrap(), "PageUp");
        assert_eq!(normalize("arrowleft").unwrap(), "ArrowLeft");
        assert_eq!(normalize("space").unwrap(), "Space");
    }

    #[test]
    fn punctuation_keys_survive_intact() {
        assert_eq!(normalize("[").unwrap(), "[");
        assert_eq!(normalize("\\").unwrap(), "\\");
        assert_eq!(normalize("+").unwrap(), "+");
        assert_eq!(normalize("Mod+=").unwrap(), "Mod+=");
    }

    #[test]
    fn rejects_input_that_isnt_a_chord() {
        assert!(normalize("").is_err());
        assert!(normalize("  ").is_err());
        assert!(normalize("Ctrl").is_err(), "modifiers alone are not a binding");
        assert!(normalize("Shift+Alt").is_err());
        assert!(normalize("a+b").is_err(), "two keys is not a chord");
    }

    #[test]
    fn every_action_has_a_unique_id_and_a_valid_default() {
        let mut ids = std::collections::HashSet::new();
        let mut keys = std::collections::HashMap::new();
        for action in ACTIONS {
            assert!(ids.insert(action.id), "duplicate action id: {}", action.id);
            let normalized = normalize(action.default)
                .unwrap_or_else(|e| panic!("{} has an invalid default: {e}", action.id));
            assert_eq!(
                normalized, action.default,
                "{}'s default is not in canonical form",
                action.id
            );
            // Two actions sharing a default would make one unreachable.
            if let Some(other) = keys.insert(normalized.clone(), action.id) {
                panic!("{} and {} both default to {}", other, action.id, normalized);
            }
        }
    }
}
