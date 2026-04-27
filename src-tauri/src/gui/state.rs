use std::sync::Mutex;
use crate::core::player::Player;

/// Holds the mpv Player instance for the GUI process.
/// The player is created lazily when `player_init` is called,
/// after the window handle is available.
pub struct GuiPlayer {
    pub player: Mutex<Option<Player>>,
}

impl GuiPlayer {
    pub fn new() -> Self {
        Self {
            player: Mutex::new(None),
        }
    }
}
