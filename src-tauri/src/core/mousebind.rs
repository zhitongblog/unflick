//! Mouse bindings.
//!
//! Not everyone drives a player from the keyboard. Wheel-for-volume,
//! middle-click-to-pause and right-drag gestures are how a large share of
//! PotPlayer and MPC users actually operate one, and unflick had none of
//! them — single-click and double-click were hardcoded in `App.tsx`, the
//! same way the shortcuts used to be.
//!
//! Triggers reuse `keybind::ACTIONS`, so a mouse gesture and a key run the
//! same code and appear under the same names. Unlike keys, the trigger set
//! is fixed — there is no equivalent of "any chord you can press" — which
//! is why these are a closed enum rather than a parsed string.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::keybind;

/// Something the mouse can do that a binding can hang off.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trigger {
    /// Stable identifier. Overrides are keyed by this.
    pub id: &'static str,
    /// Action id from `keybind::ACTIONS`, or `""` for "do nothing".
    pub default: &'static str,
    pub label: &'static str,
}

/// Bindable mouse triggers, in the order the settings panel shows them.
///
/// A gesture is a press-drag-release of the right button: the stroke's
/// dominant direction picks the trigger. Right-click still opens the
/// context menu when the pointer barely moves — the frontend only treats
/// it as a gesture past a distance threshold, and suppresses the menu in
/// that case.
pub const TRIGGERS: &[Trigger] = &[
    Trigger { id: "wheel_up",       default: "volume_up",    label: "Wheel up" },
    Trigger { id: "wheel_down",     default: "volume_down",  label: "Wheel down" },
    Trigger { id: "click",          default: "play_pause",   label: "Click" },
    Trigger { id: "double_click",   default: "fullscreen",   label: "Double click" },
    Trigger { id: "middle_click",   default: "play_pause",   label: "Middle click" },
    Trigger { id: "gesture_left",   default: "seek_back",    label: "Drag left" },
    Trigger { id: "gesture_right",  default: "seek_forward", label: "Drag right" },
    Trigger { id: "gesture_up",     default: "volume_up",    label: "Drag up" },
    Trigger { id: "gesture_down",   default: "volume_down",  label: "Drag down" },
];

const SETTINGS_KEY: &str = "mousebindings";

/// Sentinel meaning "this trigger does nothing".
pub const NONE: &str = "none";

pub fn find_trigger(id: &str) -> Option<&'static Trigger> {
    TRIGGERS.iter().find(|t| t.id == id)
}

/// Every trigger with its effective action.
pub fn list() -> Result<Value> {
    let overrides = read_overrides()?;
    let rows: Vec<Value> = TRIGGERS
        .iter()
        .map(|t| {
            let custom = overrides.get(t.id).and_then(|v| v.as_str());
            let action = custom.unwrap_or(t.default);
            json!({
                "id": t.id,
                "label": t.label,
                "action": action,
                // The action's own label, so a UI doesn't need to join
                // against the key catalogue to render a readable row.
                "action_label": keybind::find_action(action).map(|a| a.label).unwrap_or("None"),
                "default": t.default,
                "customized": custom.is_some(),
            })
        })
        .collect();
    Ok(Value::Array(rows))
}

/// Point a trigger at an action, or at `"none"` to disable it.
///
/// Unlike keys, two triggers may share an action: wheel-up and drag-up
/// both raising the volume is a reasonable thing to want, and there's no
/// ambiguity to resolve because the inputs are distinct.
pub fn set(trigger: &str, action: &str) -> Result<String> {
    let Some(target) = find_trigger(trigger) else {
        bail!("unknown mouse trigger: {} (see `unflick mouse list`)", trigger);
    };
    let action = action.trim();
    let normalized = if action.is_empty() || action.eq_ignore_ascii_case(NONE) {
        NONE.to_string()
    } else if keybind::find_action(action).is_some() {
        action.to_string()
    } else {
        bail!("unknown action: {} (see `unflick keybind list`)", action);
    };

    let mut overrides = read_overrides()?;
    if normalized == target.default {
        overrides.remove(trigger);
    } else {
        overrides.insert(trigger.to_string(), Value::String(normalized.clone()));
    }
    write_overrides(overrides)?;
    Ok(normalized)
}

/// Reset one trigger, or all of them when `trigger` is `None`.
pub fn reset(trigger: Option<&str>) -> Result<usize> {
    match trigger {
        Some(id) => {
            if find_trigger(id).is_none() {
                bail!("unknown mouse trigger: {}", id);
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
        let _ = super::settings::unset(SETTINGS_KEY);
        return Ok(());
    }
    super::settings::set(SETTINGS_KEY, Value::Object(overrides))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_trigger_has_a_unique_id_and_a_real_default_action() {
        let mut ids = std::collections::HashSet::new();
        for trigger in TRIGGERS {
            assert!(ids.insert(trigger.id), "duplicate trigger id: {}", trigger.id);
            assert!(
                keybind::find_action(trigger.default).is_some(),
                "{} defaults to {}, which is not an action",
                trigger.id,
                trigger.default
            );
        }
    }

    #[test]
    fn the_four_directions_all_exist() {
        // A gesture set missing a direction would leave a stroke doing
        // nothing at all, which reads as the feature being broken.
        for dir in ["gesture_left", "gesture_right", "gesture_up", "gesture_down"] {
            assert!(find_trigger(dir).is_some(), "missing {dir}");
        }
    }
}
