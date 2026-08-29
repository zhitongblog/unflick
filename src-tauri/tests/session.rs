//! Getting back to what was playing.
//!
//! Measured before any of this was written: play a file, seek to 0:12,
//! close the window, reopen the file — and it starts from zero. Resume
//! points were only written when unflick itself ended playback, and closing
//! a window is not that. Neither is a crash, a `taskkill`, or a power cut.
//!
//! So every case here kills the process rather than stopping it. A test
//! that shuts down cleanly first would be exercising the path that already
//! worked, and would have passed against the broken build.
//!
//! They use the 60-second fixture rather than the 20-second one on purpose:
//! seeking to 0:12 in a 20-second file leaves eight seconds before it ends,
//! and a finished file is deliberately not offered back — so the first
//! draft of these tests timed out waiting for a session the code was right
//! to have cleared.

mod common;

use common::{fixtures, Daemon};
use serde_json::json;

/// The autosave writes on a five second tick, so "has it been recorded yet"
/// is a wait, not an assertion. `wait_for` has its own generous deadline.
fn wait_for_session(d: &Daemon, path: &str, what: &str) {
    d.wait_for(
        |d| d.send("session", json!({})).data()["path"].as_str() == Some(path),
        what,
    );
}

#[test]
fn a_killed_player_still_remembers_where_it_got_to() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);
    d.send("seek", json!({"seconds": 12.0})).expect_ok();

    let path = f.with_subtitles.to_string_lossy().into_owned();
    wait_for_session(&d, &path, "the session to be written down");

    // No stop and no quit: the process dies where it stands, the same as a
    // crash or the window's close button. `restart` keeps the data dir,
    // which is the thing under test.
    let d = d.restart();

    let session = d.send("session", json!({}));
    session.expect_ok();
    assert_eq!(
        session.data()["path"].as_str(),
        Some(path.as_str()),
        "a killed player left nothing to come back to"
    );
    let at = session.data()["position"].as_f64().unwrap_or(0.0);
    assert!(at > 10.0, "recorded position {at} is not where playback had got to");
}

#[test]
fn restoring_lands_where_it_left_off_rather_than_at_the_start() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);
    d.send("seek", json!({"seconds": 12.0})).expect_ok();

    let path = f.with_subtitles.to_string_lossy().into_owned();
    wait_for_session(&d, &path, "the session to be written down");

    let d = d.restart();
    d.send("session", json!({"action": "restore"})).expect_ok();

    // The point of the whole feature: not merely the right file, the right
    // place in it.
    d.wait_for(|d| d.position() > 10.0, "playback to resume where it stopped");
    assert_eq!(d.status()["file"].as_str(), Some(path.as_str()));
}

#[test]
fn stopping_means_there_is_nothing_to_come_back_to() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);
    d.send("seek", json!({"seconds": 12.0})).expect_ok();

    let path = f.with_subtitles.to_string_lossy().into_owned();
    wait_for_session(&d, &path, "the session to be written down");

    d.send("stop", json!({})).expect_ok();

    let session = d.send("session", json!({}));
    session.expect_ok();
    assert!(
        session.data().is_null(),
        "stopping is the user saying they are done — nothing should be \
         waiting to be offered back, got {}",
        session.data()
    );
    // The resume point is a different promise and survives: reopening the
    // file by name still lands where they were.
    d.play(&f.with_subtitles);
    d.wait_for(|d| d.position() > 10.0, "the resume point to still apply");
}

#[test]
fn there_is_nothing_to_restore_before_anything_has_played() {
    let d = Daemon::start();

    let shown = d.send("session", json!({}));
    shown.expect_ok();
    assert!(shown.data().is_null());

    // An error, not a silent success: a caller asking to resume needs to
    // know it did not happen.
    d.send("session", json!({"action": "restore"}))
        .expect_err_containing("no session");
}

#[test]
fn an_unknown_action_is_named_in_the_error() {
    let d = Daemon::start();
    d.send("session", json!({"action": "resume"}))
        .expect_err_containing("resume");
}

#[test]
fn clearing_forgets_it() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);
    d.send("seek", json!({"seconds": 12.0})).expect_ok();

    let path = f.with_subtitles.to_string_lossy().into_owned();
    wait_for_session(&d, &path, "the session to be written down");

    d.send("session", json!({"action": "clear"})).expect_ok();
    assert!(d.send("session", json!({})).data().is_null());
}
