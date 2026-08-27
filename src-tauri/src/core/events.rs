//! Telling the window that something changed underneath it.
//!
//! Since v0.10 the CLI and MCP drive the very player the user is looking at,
//! and most of what they change rides the frontend's 250 ms status poll —
//! position, volume, the A-B loop, the chapter count. Two things do not:
//! the playlist and the bookmark list. Both live outside mpv, both are too
//! big to poll, and both were only ever fetched when the panel mounted or
//! the file changed. So `unflick bookmark add` from a script put a pin on a
//! progress bar nobody would see until the file was reopened, and
//! `unflick playlist add` never showed up in an open playlist panel at all.
//!
//! This is the seam that closes it: the control server says what changed,
//! the GUI turns that into an event, and the frontend refetches exactly the
//! list that moved. The headless daemon leaves it `None` — there is nobody
//! to tell.

/// Something the window may be displaying a stale copy of.
///
/// Deliberately coarse. The receiver refetches the whole list, because a
/// playlist or a file's bookmarks is a few dozen rows and the alternative —
/// shipping deltas — is a synchronisation protocol nobody asked for.
pub mod topic {
    pub const BOOKMARKS: &str = "bookmarks";
    pub const PLAYLIST: &str = "playlist";
}

pub trait EventSink: Send + Sync {
    fn notify(&self, topic: &str);
}
