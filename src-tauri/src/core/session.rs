//! Keeping the resume point true while the file is still playing.
//!
//! Resume points were only ever written at moments unflick controls: a
//! `stop`, a switch to another file, the frontend's own stop path. Nothing
//! wrote one when the window was closed — there is no close handler — so
//! the ordinary way to quit a player lost your place. A crash, a `taskkill`
//! and a power cut lost it the same way. Measured before writing this:
//! play, seek to 0:12, close the window, reopen the file, and it starts
//! from zero.
//!
//! The fix is not another exit hook. Exit hooks do not run when the process
//! dies, and the list of ways a process can die is not something to enumerate
//! and hope to have finished. Instead the position is written *while it is
//! true*, on a tick, so whatever happens next the worst loss is the length
//! of one tick.
//!
//! The same tick keeps the `session` row — which file, how far in — so a
//! later launch can offer to pick it up rather than making the user go and
//! find the file again.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use crate::core::types::PlaybackState;
use crate::db::Database;

use super::player::Player;

/// How often the position is written down.
///
/// This is the most anyone can lose. Five seconds is short enough that
/// nobody notices the gap when they come back, and long enough that the
/// write is nothing next to what playback is already doing — one upsert
/// into a local SQLite file, twelve times a minute.
const TICK: Duration = Duration::from_secs(5);

/// Start writing the resume point and the session row as playback moves.
///
/// Runs for the life of the process. There is nothing to stop it for: when
/// nothing is playing it reads one status and sleeps again.
pub fn spawn_autosave(
    player: Arc<Player>,
    db: Arc<Database>,
    incognito: Arc<std::sync::atomic::AtomicBool>,
) {
    std::thread::Builder::new()
        .name("unflick-session".into())
        .spawn(move || loop {
            std::thread::sleep(TICK);
            record_now(&player, &db, &incognito);
        })
        .map(|_| ())
        .unwrap_or_else(|e| eprintln!("[unflick] session autosave not started: {e}"));
}

/// One pass: write down where playback is, if that means anything yet.
fn record_now(
    player: &Player,
    db: &Database,
    incognito: &std::sync::atomic::AtomicBool,
) {
    // Incognito is a promise not to write down what is being watched, and
    // a resume point is exactly that. Bookmarks are the deliberate
    // exception because the user asked for those by name.
    if incognito.load(Ordering::Relaxed) {
        return;
    }

    let status = player.status();
    if status.state == PlaybackState::Stopped {
        return;
    }
    // Paused counts. It looks like it should not — a pause holds still, and
    // the tick before it recorded that spot already. But jumping to 1:20:00
    // and pausing to go and make tea records nothing new, so closing the
    // window an hour later resumes at wherever the last *playing* tick
    // landed, which can be an hour away. One upsert every five seconds is
    // not a cost worth that hole.
    let Some(path) = status.file else {
        return;
    };

    // `remember_position` owns the rules — too early to bother, close
    // enough to the end that the file counts as watched. Reusing it means
    // an autosave and a stop cannot disagree about what a resume point is.
    let _ = db.remember_position(&path, status.position, status.duration);

    // The session row follows the same "is it worth coming back to" rule,
    // for the same reason: offering to resume a film someone finished is
    // worse than offering nothing.
    if crate::db::is_finished(status.position, status.duration) {
        let _ = db.clear_session();
    } else if status.position > 1.0 {
        let _ = db.set_session(&path, status.position, status.duration);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tick_bounds_what_a_crash_can_lose() {
        // The number is the guarantee, so it is worth one line of test:
        // change it and you are changing how much someone loses.
        assert!(TICK <= Duration::from_secs(5));
    }
}
