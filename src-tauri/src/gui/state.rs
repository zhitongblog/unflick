use std::sync::{Mutex, OnceLock};
use std::sync::Arc;

use crate::core::player::Player;
use crate::core::playlist::Playlist;
use crate::core::render_loop::RenderLoop;
use crate::db::Database;

pub struct GuiPlayer {
    /// Legacy headless Player (vo=null) used by the HTML5 path. Will be
    /// removed in P5 once render-context playback is wired up to all GUI
    /// commands.
    pub player: Mutex<Option<Player>>,

    /// New render-context-backed Player (vo=libmpv). Created at app startup
    /// in the Tauri setup hook, shared across the render thread (which holds
    /// the GL context current) and command handlers (which call play/pause/
    /// seek). The OnceLock makes startup ordering explicit — any caller
    /// before setup completes gets None and falls back to the legacy path.
    pub render_player: OnceLock<Arc<Player>>,

    /// Owns the dedicated render thread. Drop this to stop the thread and
    /// release the GL context cleanly. Lives the whole app lifetime.
    pub render_loop: OnceLock<RenderLoop>,

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
            render_player: OnceLock::new(),
            render_loop: OnceLock::new(),
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
