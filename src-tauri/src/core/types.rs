use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerStatus {
    pub state: PlaybackState,
    pub file: Option<String>,
    pub position: f64,
    pub duration: f64,
    pub volume: i64,
    pub speed: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

impl Default for PlayerStatus {
    fn default() -> Self {
        Self {
            state: PlaybackState::Stopped,
            file: None,
            position: 0.0,
            duration: 0.0,
            volume: 100,
            speed: 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,
    pub duration: f64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub fps: Option<f64>,
    pub container: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleTrack {
    pub id: i64,
    pub title: Option<String>,
    pub lang: Option<String>,
    pub external_file: Option<String>,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioTrack {
    pub id: i64,
    pub title: Option<String>,
    pub lang: Option<String>,
    pub codec: Option<String>,
    pub selected: bool,
}

/// One entry from mpv's `chapter-list`. `index` is the 0-based position
/// used by `chapter_seek`; `time` is the chapter's start in seconds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chapter {
    pub index: i64,
    pub title: Option<String>,
    pub time: f64,
    pub current: bool,
}

/// A-B loop state. Either bound may be unset — mpv reports `"no"` for an
/// unset bound, which we normalise to `None`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbLoop {
    pub a: Option<f64>,
    pub b: Option<f64>,
    /// True only when both bounds are set, i.e. the loop is actually active.
    pub active: bool,
}

/// How the playlist behaves when a file ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepeatMode {
    /// Stop after the last entry.
    Off,
    /// Replay the current entry forever.
    One,
    /// Wrap around to the first entry after the last.
    All,
}

impl RepeatMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "off" | "none" => Some(Self::Off),
            "one" | "single" => Some(Self::One),
            "all" | "loop" => Some(Self::All),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::One => "one",
            Self::All => "all",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistEntry {
    pub index: usize,
    pub path: String,
    pub title: String,
    pub current: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CommandResult {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl CommandResult {
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            data: None,
        }
    }

    pub fn ok_with_data(message: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            success: true,
            message: message.into(),
            data: Some(data),
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            data: None,
        }
    }
}
