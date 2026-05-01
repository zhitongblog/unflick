use std::sync::{Mutex, OnceLock};
use std::sync::Arc;

use crate::core::player::Player;
use crate::core::playlist::Playlist;
use crate::core::render_loop::RenderLoop;
use crate::db::Database;

pub struct GuiPlayer {
    /// Render-context-backed Player (vo=libmpv). Created at app startup in
    /// the Tauri setup hook, shared across the render thread (which holds
    /// the GL context current) and command handlers (which call play/pause/
    /// seek). The OnceLock makes startup ordering explicit — any caller
    /// before setup completes gets the "not initialised" error from `mpv()`.
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
            render_player: OnceLock::new(),
            render_loop: OnceLock::new(),
            playlist: Playlist::new(),
            db: Mutex::new(db),
        }
    }

    /// Borrow the active mpv Player. Returns an error before the render
    /// pipeline finishes booting — every command handler should propagate
    /// the error so the frontend gets a clear message instead of a silent
    /// no-op.
    pub fn mpv(&self) -> Result<&Player, &'static str> {
        self.render_player
            .get()
            .map(|arc| arc.as_ref())
            .ok_or("video pipeline not initialised")
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
