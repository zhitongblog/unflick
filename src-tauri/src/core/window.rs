//! Window modes, and the seam that lets a headless command reach the window.
//!
//! The player window is one of three shapes. Normal is the video player.
//! Picture-in-picture is a small always-on-top video window. Music is the
//! compact layout for a file with no picture in it — cover, tags, transport —
//! which is what makes unflick usable as an audio player rather than a video
//! player someone left an mp3 in.
//!
//! Modes live here, and not in `gui/`, because CLI and MCP have to be able to
//! ask for them: PiP shipped as a button and nothing else, so a script could
//! start playback, seek, and screenshot, but not put the window where the
//! user wanted it. The GUI supplies a [`WindowHost`]; the headless daemon has
//! none and says so.

use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowMode {
    /// The full player.
    Normal,
    /// Small, always on top, video still playing.
    Pip,
    /// Compact audio layout: cover art, tags, transport, playlist.
    Music,
}

impl WindowMode {
    pub fn as_str(self) -> &'static str {
        match self {
            WindowMode::Normal => "normal",
            WindowMode::Pip => "pip",
            WindowMode::Music => "music",
        }
    }
}

impl FromStr for WindowMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "normal" | "full" | "player" => Ok(WindowMode::Normal),
            "pip" | "mini" => Ok(WindowMode::Pip),
            "music" | "audio" => Ok(WindowMode::Music),
            other => Err(format!(
                "unknown window mode {:?} — expected normal, pip or music",
                other
            )),
        }
    }
}

/// The real window, when one exists.
///
/// Implemented by the GUI over its Tauri window. The headless daemon leaves
/// this `None`: there is nothing to resize, and answering "ok" would be a
/// lie a script has no way to catch.
pub trait WindowHost: Send + Sync {
    fn mode(&self) -> WindowMode;
    fn set_mode(&self, mode: WindowMode) -> Result<(), String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_round_trip_through_their_names() {
        for mode in [WindowMode::Normal, WindowMode::Pip, WindowMode::Music] {
            assert_eq!(WindowMode::from_str(mode.as_str()).unwrap(), mode);
        }
    }

    #[test]
    fn common_synonyms_are_accepted() {
        // "mini" is what the window is called everywhere except our own
        // source, and "audio" is what someone reaching for music mode is
        // likely to type first.
        assert_eq!(WindowMode::from_str("mini").unwrap(), WindowMode::Pip);
        assert_eq!(WindowMode::from_str("audio").unwrap(), WindowMode::Music);
        assert_eq!(WindowMode::from_str("  MUSIC ").unwrap(), WindowMode::Music);
    }

    #[test]
    fn an_unknown_mode_names_the_ones_that_exist() {
        let err = WindowMode::from_str("tiny").unwrap_err();
        assert!(err.contains("normal"), "{}", err);
        assert!(err.contains("pip"), "{}", err);
        assert!(err.contains("music"), "{}", err);
    }
}
