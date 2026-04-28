use std::sync::Mutex;
use crate::core::player::Player;
use crate::core::playlist::Playlist;
use crate::db::Database;

pub struct GuiPlayer {
    pub player: Mutex<Option<Player>>,
    pub playlist: Playlist,
    pub db: Mutex<Option<Database>>,
}

impl GuiPlayer {
    pub fn new() -> Self {
        // Try to open the database at startup; if it fails we store None and
        // surface the error per-command.
        let db = Database::open().ok();
        Self {
            player: Mutex::new(None),
            playlist: Playlist::new(),
            db: Mutex::new(db),
        }
    }
}
