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

    /// Shared with the embedded control server (see `core::daemon`) so a
    /// `playlist_add` from the CLI or an AI agent lands in the same list the
    /// window is showing. `Arc` derefs to `Playlist`, so existing
    /// `gui_player.playlist.add(..)` call sites are unchanged.
    pub playlist: Arc<Playlist>,
    pub db: Mutex<Option<Database>>,
    /// Shared with the embedded control server so a CLI or MCP `play`
    /// respects the window's incognito switch. See `ControlContext`.
    pub incognito: Arc<std::sync::atomic::AtomicBool>,
}

impl GuiPlayer {
    pub fn new() -> Self {
        // Try to open the database at startup; if it fails we store None and
        // surface the error per-command.
        let db = Database::open().ok();
        Self {
            render_player: OnceLock::new(),
            render_loop: OnceLock::new(),
            playlist: Arc::new(Playlist::new()),
            db: Mutex::new(db),
            incognito: Arc::new(std::sync::atomic::AtomicBool::new(false)),
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
/// UNFLICK_OPEN_FILE env var set in main.rs).
///
/// Two readers, in this order. The backend takes it as soon as mpv exists
/// and starts it — the file does not have to wait for the WebView to boot,
/// which used to be a third of the way to the first frame. The frontend
/// then asks what happened via `consume_pending_file` and adopts whatever
/// is already playing rather than opening it a second time.
pub struct PendingFile {
    path: Mutex<Option<String>>,
    /// Set once the backend has issued the open, so the frontend knows not
    /// to repeat it. A reload of the page returns `None` from here, which
    /// is what keeps a refresh from replaying the launch.
    outcome: Mutex<Option<StartupOpen>>,
}

/// What the backend did with the file the shell handed us.
#[derive(Clone, serde::Serialize)]
pub struct StartupOpen {
    pub path: String,
    /// `None` when it opened. The frontend surfaces the message; a launch
    /// that fails silently looks like the app ignored the double-click.
    pub error: Option<String>,
}

impl PendingFile {
    pub fn from_env() -> Self {
        Self {
            path: Mutex::new(
                std::env::var("UNFLICK_OPEN_FILE").ok().filter(|s| !s.is_empty()),
            ),
            outcome: Mutex::new(None),
        }
    }

    /// Backend side: take the path to open. Single-shot.
    pub fn take_path(&self) -> Option<String> {
        self.path.lock().ok().and_then(|mut g| g.take())
    }

    /// Backend side: record how the open went.
    pub fn set_outcome(&self, outcome: StartupOpen) {
        if let Ok(mut g) = self.outcome.lock() {
            *g = Some(outcome);
        }
    }

    /// Frontend side: take the result. Single-shot, so a page refresh does
    /// not re-trigger anything.
    pub fn take_outcome(&self) -> Option<StartupOpen> {
        self.outcome.lock().ok().and_then(|mut g| g.take())
    }
}
