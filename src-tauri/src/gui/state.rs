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

/// Holds a file path Explorer asked us to open at launch (via the
/// UNFLICK_OPEN_FILE env var set in main.rs). The frontend pulls and
/// clears it via the `consume_pending_file` command on init.
pub struct PendingFile(pub Mutex<Option<String>>);

impl PendingFile {
    pub fn from_env() -> Self {
        Self(Mutex::new(std::env::var("UNFLICK_OPEN_FILE").ok().filter(|s| !s.is_empty())))
    }
}
