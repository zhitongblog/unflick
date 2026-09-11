//! Playback control: chapters, A-B loop, timing offsets, subtitle styling,
//! playlist order, and the resume-point policy.
//!
//! Every case here drives the real binary against real media. The two
//! regressions these were written for — a file loading paused at 0:00, and
//! a finished file reopening on its last frame — both compile fine and both
//! made the player look broken.

mod common;

use common::{Daemon, fixtures, mcp_roundtrip};
use serde_json::json;

#[test]
fn reports_status_for_a_loaded_file() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    let status = d.status();
    assert_eq!(status["state"], "playing");
    assert!(
        (status["duration"].as_f64().unwrap() - 30.0).abs() < 1.0,
        "expected ~30s duration, got {}",
        status["duration"]
    );
}

// ─── Chapters ─────────────────────────────────────────────────────────────

#[test]
fn reads_container_chapters() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    let chapters = d.send("chapter_list", json!({})).expect_ok().data();
    let list = chapters.as_array().expect("chapter list is an array");
    assert_eq!(list.len(), 3);
    assert_eq!(list[0]["title"], "Opening");
    assert_eq!(list[1]["title"], "Middle Part");
    assert_eq!(list[2]["title"], "Finale");
    assert_eq!(list[0]["time"].as_f64().unwrap(), 0.0);
}

#[test]
fn chapter_navigation_moves_and_clamps() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);
    // `play` waits for a duration, which does not imply mpv has finished
    // parsing chapters - seeking too early fails with "property unavailable".
    // Rare when this test ran alone; routine once the suite grew enough
    // parallel daemons to slow each one down.
    d.wait_for(
        |d| {
            d.send("chapter_list", json!({}))
                .data()
                .as_array()
                .map(|c| c.len() >= 3)
                .unwrap_or(false)
        },
        "chapters to be parsed",
    );

    d.send("chapter_seek", json!({ "index": 2 })).expect_ok();
    d.wait_for(|d| d.position() >= 19.0, "seek to the third chapter");

    d.send("chapter_prev", json!({})).expect_ok();
    d.wait_for(|d| d.position() < 19.0, "step back a chapter");

    // Stepping past the last chapter clamps rather than erroring.
    d.send("chapter_seek", json!({ "index": 2 })).expect_ok();
    let reply = d.send("chapter_next", json!({}));
    reply.expect_ok();
    assert_eq!(reply.data()["index"], 2, "chapter_next should clamp at the end");
}

#[test]
fn rejects_out_of_range_chapter() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    d.send("chapter_seek", json!({ "index": 99 }))
        .expect_err_containing("out of range");
}

#[test]
fn a_file_without_chapters_reports_none() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.plain);

    let chapters = d.send("chapter_list", json!({})).expect_ok().data();
    assert!(chapters.as_array().unwrap().is_empty());
}

// ─── A-B loop ─────────────────────────────────────────────────────────────

#[test]
fn ab_loop_sets_reports_and_clears_bounds() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    let a = d.send("ab_loop", json!({ "action": "a", "position": 3.0 }));
    a.expect_ok();
    assert_eq!(a.data()["a"], 3.0);
    assert_eq!(a.data()["active"], false, "one bound is not an active loop");

    let b = d.send("ab_loop", json!({ "action": "b", "position": 6.0 }));
    b.expect_ok();
    assert_eq!(b.data()["b"], 6.0);
    assert_eq!(b.data()["active"], true);

    let status = d.send("ab_loop", json!({ "action": "status" }));
    status.expect_ok();
    assert_eq!(status.data()["a"], 3.0);

    let cleared = d.send("ab_loop", json!({ "action": "clear" }));
    cleared.expect_ok();
    assert!(cleared.data()["a"].is_null());
    assert!(cleared.data()["b"].is_null());
    assert_eq!(cleared.data()["active"], false);
}

#[test]
fn ab_loop_rejects_unknown_action() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    d.send("ab_loop", json!({ "action": "sideways" }))
        .expect_err_containing("unknown ab_loop action");
}

// ─── Timing offsets ───────────────────────────────────────────────────────

#[test]
fn subtitle_delay_reads_sets_and_nudges() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);

    let initial = d.send("subtitle_delay", json!({}));
    initial.expect_ok();
    assert_eq!(initial.data()["seconds"], 0.0);

    let absolute = d.send("subtitle_delay", json!({ "seconds": 0.5 }));
    absolute.expect_ok();
    assert!((absolute.data()["seconds"].as_f64().unwrap() - 0.5).abs() < 1e-6);

    let relative = d.send("subtitle_delay", json!({ "seconds": 0.25, "relative": true }));
    relative.expect_ok();
    assert!(
        (relative.data()["seconds"].as_f64().unwrap() - 0.75).abs() < 1e-6,
        "relative should add to the current delay, got {}",
        relative.data()["seconds"]
    );
}

#[test]
fn audio_delay_accepts_negative_values() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    let reply = d.send("audio_delay", json!({ "seconds": -0.3 }));
    reply.expect_ok();
    assert!((reply.data()["seconds"].as_f64().unwrap() + 0.3).abs() < 1e-6);
}

// ─── Now playing / window modes ───────────────────────────────────────────

/// The tags are the point: a path tells you the file name, not who is
/// singing. And an embedded cover must not be mistaken for video, or every
/// tagged mp3 opens the video layout.
#[test]
fn now_playing_reads_tags_and_knows_a_cover_is_not_video() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.audio).expect_ok();

    let np = d.send("nowplaying", json!({ "cover": true })).expect_ok().data();
    assert_eq!(np["title"], "Test Track");
    assert_eq!(np["artist"], "Fixture Ensemble");
    assert_eq!(np["album"], "Integration Suite");
    assert_eq!(np["has_video"], false, "an attached picture is not video");

    let cover = np["cover"].as_str().expect("the fixture has cover art");
    assert!(
        std::path::Path::new(cover).exists(),
        "cover art should be extracted to disk, got {}",
        cover
    );
}

#[test]
fn now_playing_reports_video_for_a_video_file() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.plain).expect_ok();

    let np = d.send("nowplaying", json!({})).expect_ok().data();
    assert_eq!(np["has_video"], true);
    assert!(np["cover"].is_null(), "cover extraction is opt-in");
}

/// There is no window in the headless daemon, and reporting a mode change
/// nothing performed is the same class of lie `play` used to tell.
#[test]
fn window_modes_are_refused_without_a_window() {
    let d = Daemon::start();

    d.send("window_mode", json!({ "mode": "music" }))
        .expect_err_containing("no window");
    d.send("window_mode", json!({})).expect_err_containing("no window");

    // A typo is named as a typo even here: "no window" would point at the
    // wrong problem for someone who simply misspelled the mode.
    d.send("window_mode", json!({ "mode": "tiny" }))
        .expect_err_containing("unknown window mode");
}

// ─── Load reporting ───────────────────────────────────────────────────────

/// `loadfile` is asynchronous and never fails on its own, so for a long time
/// `play` reported success for a path nobody could open — and `status` then
/// claimed to be playing it. A network share that is down is the everyday
/// version of this.
#[test]
fn play_reports_a_source_that_cannot_be_opened() {
    let d = Daemon::start();

    let missing = d.data_dir().join("no-such-file.mkv");
    d.send("play", json!({ "file": missing.to_str().unwrap() }))
        .expect_err_containing("could not open");

    let status = d.status();
    assert_eq!(status["state"], "stopped", "nothing opened, so nothing plays");
    assert!(
        status["file"].is_null(),
        "a file that never opened must not be reported as the current one, got {}",
        status["file"]
    );
}

/// A successful load must say so explicitly, so a caller watching a slow
/// source can tell "on screen" from "still opening".
#[test]
fn play_reports_a_loaded_source_as_loaded() {
    let f = fixtures();
    let d = Daemon::start();

    let reply = d.send("play", json!({ "file": f.plain.to_str().unwrap() }));
    reply.expect_ok();
    assert_eq!(reply.data()["loaded"], true);
}

/// Replacing a playing file must not be read as that file failing: mpv ends
/// the outgoing file before it starts the new one.
#[test]
fn switching_files_while_playing_still_reports_success() {
    let f = fixtures();
    let d = Daemon::start();

    d.play(&f.plain).expect_ok();
    let reply = d.send("play", json!({ "file": f.with_chapters.to_str().unwrap() }));
    reply.expect_ok();
    assert_eq!(reply.data()["loaded"], true);
}

/// `smb://` is what people type for a share. Our bundled mpv has no such
/// protocol, and the useful answer is "mount it first" — which nobody guesses
/// from mpv's own silence.
///
/// Whether a given build speaks smb is a property of the libmpv it loaded:
/// the bundled Windows one does not, a distro's is often built against an
/// ffmpeg that does. Both are correct behaviour and the test accepts either —
/// what it will not accept is the share URL disappearing into mpv and coming
/// back as nothing the user can act on.
#[test]
fn share_urls_are_refused_with_a_way_forward() {
    let d = Daemon::start();

    for (url, kind) in [
        ("smb://server/share/film.mkv", "SMB"),
        ("nfs://server/export/film.mkv", "NFS"),
    ] {
        let reply = d.send("play", json!({ "file": url }));
        let message = reply.message().to_string();
        assert!(!reply.success(), "{} must not report success", url);

        if message.contains("not supported") {
            assert!(message.contains(kind), "{} refused as something else: {}", url, message);
            // The advice is platform-specific, so match the verb rather than
            // the sentence: mount, or the GUI that does the mounting.
            let says_how = message.contains("mount")
                || message.contains("Explorer")
                || message.contains("Finder");
            assert!(says_how, "a refusal has to say what to do instead, got: {}", message);
        } else {
            // This build does speak the protocol, so the attempt reached mpv
            // and failed on the server that is not there — which is the right
            // error for a host nobody can reach.
            assert!(
                message.contains("could not open"),
                "either refuse with advice or report mpv's own failure, got: {}",
                message
            );
        }
    }
}

/// Windows drive letters must not be mistaken for URL schemes on the way in.
#[test]
fn a_plain_path_is_never_read_as_a_protocol() {
    let f = fixtures();
    let d = Daemon::start();

    d.send("play", json!({ "file": f.plain.to_str().unwrap() })).expect_ok();
}

// ─── Playback speed ───────────────────────────────────────────────────────

#[test]
fn speed_reads_sets_and_nudges() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.plain);

    let initial = d.send("speed", json!({}));
    initial.expect_ok();
    assert_eq!(initial.data()["rate"], 1.0);

    let absolute = d.send("speed", json!({ "rate": 1.5 }));
    absolute.expect_ok();
    assert!((absolute.data()["rate"].as_f64().unwrap() - 1.5).abs() < 1e-6);

    let relative = d.send("speed", json!({ "rate": -0.25, "relative": true }));
    relative.expect_ok();
    assert!(
        (relative.data()["rate"].as_f64().unwrap() - 1.25).abs() < 1e-6,
        "relative should offset the current rate, got {}",
        relative.data()["rate"]
    );
}

/// Holding the "slower" key down must bottom out, not start erroring.
#[test]
fn relative_speed_clamps_instead_of_failing() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.plain);

    let reply = d.send("speed", json!({ "rate": -100.0, "relative": true }));
    reply.expect_ok();
    assert!((reply.data()["rate"].as_f64().unwrap() - 0.01).abs() < 1e-9);
}

#[test]
fn absolute_speed_outside_the_range_is_rejected() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.plain);

    d.send("speed", json!({ "rate": 0.0 })).expect_err_containing("between");
    d.send("speed", json!({ "rate": 500.0 })).expect_err_containing("between");

    // The rejected calls must not have moved anything.
    assert_eq!(d.send("speed", json!({})).data()["rate"], 1.0);
}

// ─── Subtitle styling ─────────────────────────────────────────────────────

#[test]
fn subtitle_style_round_trips_each_type() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);

    // A number, an integer, and a boolean — the three shapes the CLI has to
    // coerce correctly on the way in.
    d.send("subtitle_style_set", json!({ "name": "scale", "value": 1.4 })).expect_ok();
    d.send("subtitle_style_set", json!({ "name": "pos", "value": 90 })).expect_ok();
    d.send("subtitle_style_set", json!({ "name": "bold", "value": true })).expect_ok();

    let style = d.send("subtitle_style_get", json!({})).expect_ok().data();
    assert!((style["scale"].as_f64().unwrap() - 1.4).abs() < 1e-6);
    assert_eq!(style["pos"].as_i64().unwrap(), 90);
    assert_eq!(style["bold"], true);
}

#[test]
fn subtitle_style_clamps_out_of_range_values() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);

    // Scale 0 would make subtitles vanish with no obvious way back.
    d.send("subtitle_style_set", json!({ "name": "scale", "value": 0.0 })).expect_ok();
    let style = d.send("subtitle_style_get", json!({})).expect_ok().data();
    assert!(
        style["scale"].as_f64().unwrap() >= 0.1,
        "scale should clamp above zero, got {}",
        style["scale"]
    );
}

#[test]
fn subtitle_style_rejects_unknown_property() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);

    d.send("subtitle_style_set", json!({ "name": "nope", "value": 1 }))
        .expect_err_containing("unknown subtitle style");
}

// ─── Bilingual subtitles ──────────────────────────────────────────────────
//
// Two tracks on screen at once. Most of what is asserted here exists because
// mpv answers a write it cannot honour with success and then ignores it:
// setting `secondary-sid` to the id already in `sid`, or to a track that is
// not there, both look like they worked. Every one of those would have this
// feature reporting a second line that is not on screen.

fn settings_json(d: &Daemon) -> String {
    std::fs::read_to_string(d.data_dir().join("settings.json")).unwrap_or_default()
}

/// Load the translation as a second track and turn bilingual on.
fn two_tracks(d: &Daemon, translation: &std::path::Path) -> serde_json::Value {
    d.send("subtitle_load", json!({ "file": translation.to_string_lossy() }))
        .expect_ok();
    let reply = d.send("subtitle_bilingual", json!({ "enabled": true }));
    reply.expect_ok();
    reply.data()
}

#[test]
fn bilingual_reads_as_off_before_anything_is_asked_for() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);

    let reply = d.send("subtitle_bilingual", json!({}));
    reply.expect_ok();
    assert_eq!(reply.data()["enabled"], false);
    assert_eq!(reply.message(), "bilingual off");
    // A read is a read. `subtitle delay` set that shape and a bare read that
    // wrote a preference would turn "what is this doing" into a change.
    assert!(
        !settings_json(&d).contains("subtitle_bilingual"),
        "a read must not write the preference: {}",
        settings_json(&d)
    );
}

#[test]
fn bilingual_shows_two_tracks_at_once() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);

    let data = two_tracks(&d, &f.translation);
    assert_eq!(data["enabled"], true);
    let primary = data["primary"]["id"].as_i64().unwrap();
    let secondary = data["secondary"]["id"].as_i64().unwrap();
    assert_ne!(primary, secondary, "one track cannot be both lines");

    let tracks = d.send("subtitle_list", json!({})).expect_ok().data();
    let list = tracks.as_array().unwrap();
    let selected: Vec<i64> = list
        .iter()
        .filter(|t| t["selected"] == true)
        .map(|t| t["id"].as_i64().unwrap())
        .collect();
    let seconds: Vec<i64> = list
        .iter()
        .filter(|t| t["secondary"] == true)
        .map(|t| t["id"].as_i64().unwrap())
        .collect();
    assert_eq!(selected, vec![primary]);
    assert_eq!(seconds, vec![secondary]);
}

#[test]
fn bilingual_loads_a_subtitle_file_named_as_the_second_track() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);

    let count = |d: &Daemon| {
        d.send("subtitle_list", json!({})).expect_ok().data().as_array().unwrap().len()
    };
    assert_eq!(count(&d), 1, "the fixture starts with its own sidecar only");

    let args = json!({ "enabled": true, "secondary": f.translation.to_string_lossy() });
    let first = d.send("subtitle_bilingual", args.clone());
    first.expect_ok();
    assert_eq!(count(&d), 2);
    let used = first.data()["secondary"]["id"].as_i64().unwrap();

    // Bare `sub-add` adds the same file twice and the copy is
    // indistinguishable from the original in the menu.
    let second = d.send("subtitle_bilingual", args);
    second.expect_ok();
    assert_eq!(count(&d), 2, "naming the same file twice must not add a duplicate");
    assert_eq!(second.data()["secondary"]["id"].as_i64().unwrap(), used);
}

#[test]
fn bilingual_refuses_to_use_one_track_for_both_lines() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);
    d.send("subtitle_load", json!({ "file": f.translation.to_string_lossy() })).expect_ok();

    // mpv answers this write with success and leaves both properties alone.
    d.send("subtitle_bilingual", json!({ "enabled": true, "primary": 1, "secondary": 1 }))
        .expect_err_containing("two different tracks");
}

#[test]
fn bilingual_refuses_a_track_that_does_not_exist() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);
    d.send("subtitle_load", json!({ "file": f.translation.to_string_lossy() })).expect_ok();

    // The other silent no-op: `secondary-sid = 99` reports success and stays
    // at `no`, so without the check this would claim a second line.
    d.send("subtitle_bilingual", json!({ "enabled": true, "secondary": 99 }))
        .expect_err_containing("99");
    assert_eq!(
        d.send("subtitle_bilingual", json!({})).expect_ok().data()["enabled"],
        false
    );
}

#[test]
fn bilingual_refuses_when_there_is_nothing_to_pair() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);

    d.send("subtitle_bilingual", json!({ "enabled": true }))
        .expect_err_containing("subtitle load");
}

#[test]
fn the_two_lines_never_share_a_position() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);
    two_tracks(&d, &f.translation);

    // A libmpv without `secondary-sub-pos` cannot be asked to stack the two
    // lines, and `bilingual on` says so rather than pretending: it reports the
    // layout as `top`, which is where mpv puts a secondary subtitle on its own.
    // That is a different arrangement, not a broken one — the two lines are at
    // opposite edges instead of adjacent — so assert the fallback and stop.
    let state = d.send("subtitle_bilingual", json!({})).expect_ok().data();
    if state["secondary_sub_pos"].is_null() {
        assert_eq!(
            state["layout"], "top",
            "no secondary-sub-pos, so the layout has to be the one mpv does by itself: {state}"
        );
        assert!(
            state["secondary"].is_object(),
            "the second line must still be on: {state}"
        );
        return;
    }

    let gap = |d: &Daemon| {
        let s = d.send("subtitle_bilingual", json!({})).expect_ok().data();
        let primary = s["sub_pos"].as_f64().unwrap();
        let secondary = s["secondary_sub_pos"].as_f64().expect("secondary position");
        (primary - secondary).abs()
    };
    assert!(gap(&d) >= 4.0, "the two lines start on top of each other: {}", gap(&d));

    // Moving the subtitles up must take the second line with them: mpv
    // collapses both onto one row the moment the positions match.
    d.send("subtitle_style_set", json!({ "name": "pos", "value": 60 })).expect_ok();
    assert!(gap(&d) >= 4.0, "the layout did not follow `pos`: {}", gap(&d));

    // A bigger line needs a bigger gap, or the two touch.
    d.send("subtitle_style_set", json!({ "name": "scale", "value": 1.5 })).expect_ok();
    assert!(gap(&d) >= 4.0 * 1.5, "the layout did not follow `scale`: {}", gap(&d));
}

#[test]
fn subtitle_delay_moves_both_lines_together() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);
    two_tracks(&d, &f.translation);

    d.send("subtitle_delay", json!({ "seconds": 0.5 })).expect_ok();
    let state = d.send("subtitle_bilingual", json!({})).expect_ok().data();
    assert!((state["delay"].as_f64().unwrap() - 0.5).abs() < 1e-6);
    // `secondary-sub-delay` arrived in the same mpv release as
    // `secondary-sub-pos`. Where it is missing there is no second delay to
    // drift: mpv applies the one delay to both lines, which is the behaviour
    // this test is about in the first place.
    if state["secondary_delay"].is_null() {
        assert!(
            state["secondary"].is_object(),
            "the second line must still be on: {state}"
        );
        return;
    }
    assert!(
        (state["secondary_delay"].as_f64().unwrap() - 0.5).abs() < 1e-6,
        "the translation drifted away from the original: {state}"
    );
}

#[test]
fn choosing_a_single_track_turns_bilingual_off() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);
    let primary = two_tracks(&d, &f.translation)["primary"]["id"].as_i64().unwrap();

    d.send("subtitle_select", json!({ "id": primary })).expect_ok();
    assert_eq!(
        d.send("subtitle_bilingual", json!({})).expect_ok().data()["enabled"],
        false,
        "picking one track has to mean one track"
    );
}

#[test]
fn turning_subtitles_off_takes_the_second_line_with_them() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);
    two_tracks(&d, &f.translation);

    // Measured: with a secondary set, `sid = no` leaves the translation
    // rendering. "Off" would turn nothing off.
    d.send("subtitle_select", json!({ "id": 0 })).expect_ok();
    let tracks = d.send("subtitle_list", json!({})).expect_ok().data();
    for track in tracks.as_array().unwrap() {
        assert_eq!(track["selected"], false, "{track}");
        assert_eq!(track["secondary"], false, "{track}");
    }
}

#[test]
fn turning_bilingual_off_leaves_the_first_line_playing() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);
    let primary = two_tracks(&d, &f.translation)["primary"]["id"].as_i64().unwrap();

    d.send("subtitle_bilingual", json!({ "enabled": false })).expect_ok();
    let tracks = d.send("subtitle_list", json!({})).expect_ok().data();
    let list = tracks.as_array().unwrap();
    assert_eq!(list.iter().filter(|t| t["selected"] == true).count(), 1);
    assert_eq!(list.iter().filter(|t| t["secondary"] == true).count(), 0);
    assert_eq!(
        list.iter().find(|t| t["selected"] == true).unwrap()["id"].as_i64(),
        Some(primary)
    );
}

#[test]
fn bilingual_comes_back_on_the_next_file() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);
    two_tracks(&d, &f.translation);

    // A file load resets `secondary-sid` and drops external subs. Without
    // the re-arm hook the persisted preference is a setting that does
    // nothing from the second file onward.
    d.play(&f.with_subtitles);
    d.send("subtitle_load", json!({ "file": f.translation.to_string_lossy() })).expect_ok();

    d.wait_for(
        |d| d.send("subtitle_bilingual", json!({})).data()["enabled"] == json!(true),
        "the second line to come back on the next file",
    );
}

#[test]
fn bilingual_survives_a_restart() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);

    // An unrelated key, to prove the preference is merged in rather than
    // written over the whole file — the bug that silently deleted every
    // keybinding the settings panel had never heard of.
    d.send("settings_set", json!({ "key": "volume_step", "value": 7 })).expect_ok();
    two_tracks(&d, &f.translation);
    d.send("subtitle_bilingual", json!({ "layout": "top" })).expect_ok();

    let d = d.restart();
    let settings: serde_json::Value =
        serde_json::from_str(&settings_json(&d)).expect("settings.json is valid JSON");
    assert_eq!(settings["subtitle_bilingual"]["enabled"], true);
    assert_eq!(settings["subtitle_bilingual"]["layout"], "top");
    assert_eq!(settings["volume_step"], 7, "merge dropped an unrelated key");
}

#[test]
fn mcp_exposes_the_bilingual_tool() {
    let d = Daemon::start();
    let replies = mcp_roundtrip(
        &[json!({ "jsonrpc": "2.0", "id": 41, "method": "tools/list", "params": {} })],
        &d,
    );
    let tools = replies[&41]["result"]["tools"].as_array().unwrap().clone();
    let tool = tools
        .iter()
        .find(|t| t["name"] == "subtitle_bilingual")
        .expect("subtitle_bilingual is missing from tools/list");

    // It has to be callable bare, because that is how the state is read.
    assert!(tool["inputSchema"].get("required").is_none());
    assert_eq!(
        tool["inputSchema"]["properties"]["layout"]["enum"],
        json!(["stacked", "top"])
    );
}

#[test]
fn mcp_can_turn_bilingual_on_end_to_end() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);

    let replies = mcp_roundtrip(
        &[json!({
            "jsonrpc": "2.0", "id": 42, "method": "tools/call",
            "params": {
                "name": "subtitle_bilingual",
                "arguments": { "enabled": true, "secondary": f.translation.to_string_lossy() }
            }
        })],
        &d,
    );
    let result = &replies[&42]["result"];
    assert_ne!(result["isError"], true, "{result}");

    // The same player the CLI talks to.
    let state = d.send("subtitle_bilingual", json!({})).expect_ok().data();
    assert_eq!(state["enabled"], true);
    assert!(state["secondary"]["id"].as_i64().is_some(), "{state}");
}

// ─── Playlist order ───────────────────────────────────────────────────────

#[test]
fn repeat_mode_round_trips_and_rejects_junk() {
    let d = Daemon::start();

    assert_eq!(d.send("playlist_repeat", json!({})).expect_ok().data()["mode"], "off");
    assert_eq!(
        d.send("playlist_repeat", json!({ "mode": "all" })).expect_ok().data()["mode"],
        "all"
    );
    d.send("playlist_repeat", json!({ "mode": "sideways" }))
        .expect_err_containing("unknown repeat mode");
}

#[test]
fn shuffle_keeps_entry_numbering_stable() {
    let f = fixtures();
    let d = Daemon::start();
    for path in [&f.with_chapters, &f.plain, &f.with_subtitles] {
        d.send("playlist_add", json!({ "file": path.to_string_lossy() })).expect_ok();
    }

    let before = d.send("playlist_list", json!({})).expect_ok().data();
    d.send("playlist_shuffle", json!({ "enabled": true })).expect_ok();
    let after = d.send("playlist_list", json!({})).expect_ok().data();

    // Shuffle changes playback order, not the list the user is looking at.
    assert_eq!(before, after, "shuffle must not renumber playlist entries");
    assert_eq!(
        d.send("playlist_shuffle", json!({})).expect_ok().data()["enabled"],
        true
    );
}

#[test]
fn playlist_advances_to_the_next_entry_at_end_of_file() {
    let f = fixtures();
    let d = Daemon::start();
    d.send("playlist_add", json!({ "file": f.with_chapters.to_string_lossy() })).expect_ok();
    d.send("playlist_add", json!({ "file": f.plain.to_string_lossy() })).expect_ok();

    d.play(&f.with_chapters);
    d.send("seek", json!({ "seconds": 28.5 })).expect_ok();

    let expected = f.plain.file_name().unwrap().to_string_lossy().into_owned();
    d.wait_for(
        |d| {
            d.status()["file"]
                .as_str()
                .map(|s| s.ends_with(expected.as_str()))
                .unwrap_or(false)
        },
        "playlist to advance at end of file",
    );

    // Regression: mpv's `pause` is global and `keep-open` leaves it set at
    // EOF, so the next file used to load paused at 0:00.
    d.wait_for(|d| d.status()["state"] == "playing", "the next entry to actually play");
}

// ─── Keyboard bindings ────────────────────────────────────────────────────

#[test]
fn lists_every_action_with_its_effective_key() {
    let d = Daemon::start();
    let rows = d.send("keybind_list", json!({})).expect_ok().data();
    let rows = rows.as_array().expect("binding list");

    assert!(rows.len() >= 25, "expected a full catalogue, got {}", rows.len());
    let play = rows
        .iter()
        .find(|r| r["id"] == "play_pause")
        .expect("play_pause should be bindable");
    assert_eq!(play["key"], "Space");
    assert_eq!(play["default"], "Space");
    assert_eq!(play["customized"], false);
}

#[test]
fn rebinding_persists_and_marks_the_action_as_customized() {
    let d = Daemon::start();
    let set = d.send("keybind_set", json!({ "action": "play_pause", "key": "k" }));
    set.expect_ok();
    assert_eq!(set.data()["key"], "k");

    let rows = d.send("keybind_list", json!({})).expect_ok().data();
    let play = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "play_pause")
        .unwrap()
        .clone();
    assert_eq!(play["key"], "k");
    assert_eq!(play["customized"], true);
    assert_eq!(play["default"], "Space", "the default must still be reported");
}

#[test]
fn rebinding_normalizes_modifier_order_and_case() {
    let d = Daemon::start();
    let set = d.send("keybind_set", json!({ "action": "screenshot", "key": "shift+CTRL+G" }));
    set.expect_ok();
    assert_eq!(set.data()["key"], "Mod+Shift+g");
}

#[test]
fn a_key_already_in_use_is_refused_by_name() {
    let d = Daemon::start();
    // `l` opens the library. Taking it for something else silently would
    // leave the library shortcut dead with no indication why.
    let clash = d.send("keybind_set", json!({ "action": "screenshot", "key": "l" }));
    clash.expect_err_containing("already bound");
    assert!(
        clash.message().contains("Library"),
        "the refusal should name the conflicting action: {}",
        clash.message()
    );

    // And the original binding is untouched.
    let rows = d.send("keybind_list", json!({})).expect_ok().data();
    let library = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "toggle_library")
        .unwrap()
        .clone();
    assert_eq!(library["key"], "l");
}

#[test]
fn rebinding_to_the_default_clears_the_override() {
    let d = Daemon::start();
    d.send("keybind_set", json!({ "action": "mute", "key": "k" })).expect_ok();
    d.send("keybind_set", json!({ "action": "mute", "key": "m" })).expect_ok();

    let rows = d.send("keybind_list", json!({})).expect_ok().data();
    let mute = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "mute")
        .unwrap()
        .clone();
    assert_eq!(mute["key"], "m");
    assert_eq!(
        mute["customized"], false,
        "setting a key back to its default should stop counting as customized"
    );
}

#[test]
fn reset_restores_one_action_and_then_all_of_them() {
    let d = Daemon::start();
    d.send("keybind_set", json!({ "action": "play_pause", "key": "k" })).expect_ok();
    d.send("keybind_set", json!({ "action": "screenshot", "key": "F2" })).expect_ok();

    let one = d.send("keybind_reset", json!({ "action": "play_pause" }));
    one.expect_ok();
    assert_eq!(one.data()["reset"], 1);

    let all = d.send("keybind_reset", json!({}));
    all.expect_ok();
    assert_eq!(all.data()["reset"], 1, "only the still-customized one remains");

    let rows = d.send("keybind_list", json!({})).expect_ok().data();
    assert!(
        rows.as_array().unwrap().iter().all(|r| r["customized"] == false),
        "nothing should be customized after a full reset"
    );
}

#[test]
fn rejects_unknown_actions_and_malformed_keys() {
    let d = Daemon::start();
    d.send("keybind_set", json!({ "action": "fly_to_the_moon", "key": "k" }))
        .expect_err_containing("unknown action");
    d.send("keybind_set", json!({ "action": "play_pause", "key": "Ctrl" }))
        .expect_err_containing("needs a key");
    d.send("keybind_set", json!({ "action": "play_pause", "key": "" }))
        .expect_err_containing("empty");
    d.send("keybind_reset", json!({ "action": "nope" }))
        .expect_err_containing("unknown action");
}

// ─── Mouse bindings ───────────────────────────────────────────────────────

#[test]
fn lists_mouse_triggers_with_their_actions() {
    let d = Daemon::start();
    let rows = d.send("mouse_list", json!({})).expect_ok().data();
    let rows = rows.as_array().expect("trigger list");

    // Wheel, three click kinds, four gesture directions.
    assert!(rows.len() >= 9, "expected the full trigger set, got {}", rows.len());
    let wheel_up = rows.iter().find(|r| r["id"] == "wheel_up").expect("wheel_up");
    assert_eq!(wheel_up["action"], "volume_up");
    assert_eq!(wheel_up["action_label"], "Volume up");
    assert_eq!(wheel_up["customized"], false);
}

#[test]
fn mouse_triggers_may_share_an_action() {
    let d = Daemon::start();
    // Unlike keys, this is not a conflict: the inputs are distinct, so
    // wanting both wheel-up and drag-up to raise the volume is reasonable.
    d.send("mouse_set", json!({ "trigger": "gesture_up", "action": "volume_up" }))
        .expect_ok();
    d.send("mouse_set", json!({ "trigger": "wheel_up", "action": "volume_up" }))
        .expect_ok();
}

#[test]
fn a_trigger_can_be_disabled() {
    let d = Daemon::start();
    let off = d.send("mouse_set", json!({ "trigger": "click", "action": "none" }));
    off.expect_ok();
    assert_eq!(off.data()["action"], "none");

    let rows = d.send("mouse_list", json!({})).expect_ok().data();
    let click = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "click")
        .unwrap()
        .clone();
    assert_eq!(click["action"], "none");
    assert_eq!(click["customized"], true);
}

#[test]
fn mouse_bindings_reject_unknown_triggers_and_actions() {
    let d = Daemon::start();
    d.send("mouse_set", json!({ "trigger": "elbow_nudge", "action": "play_pause" }))
        .expect_err_containing("unknown mouse trigger");
    // Pointing a trigger at a non-existent action would leave it silently
    // dead, so it's refused rather than stored.
    d.send("mouse_set", json!({ "trigger": "wheel_up", "action": "fly_to_the_moon" }))
        .expect_err_containing("unknown action");
}

#[test]
fn mouse_reset_restores_defaults() {
    let d = Daemon::start();
    d.send("mouse_set", json!({ "trigger": "middle_click", "action": "screenshot" }))
        .expect_ok();
    d.send("mouse_set", json!({ "trigger": "double_click", "action": "pip" }))
        .expect_ok();

    let one = d.send("mouse_reset", json!({ "trigger": "middle_click" }));
    one.expect_ok();
    assert_eq!(one.data()["reset"], 1);

    d.send("mouse_reset", json!({})).expect_ok();
    let rows = d.send("mouse_list", json!({})).expect_ok().data();
    assert!(
        rows.as_array().unwrap().iter().all(|r| r["customized"] == false),
        "nothing should stay customized after a full reset"
    );
}

#[test]
fn setting_a_trigger_back_to_its_default_clears_the_override() {
    let d = Daemon::start();
    d.send("mouse_set", json!({ "trigger": "wheel_down", "action": "seek_back" }))
        .expect_ok();
    d.send("mouse_set", json!({ "trigger": "wheel_down", "action": "volume_down" }))
        .expect_ok();

    let rows = d.send("mouse_list", json!({})).expect_ok().data();
    let wheel = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "wheel_down")
        .unwrap()
        .clone();
    assert_eq!(wheel["action"], "volume_down");
    assert_eq!(wheel["customized"], false);
}

// ─── Timeline previews ────────────────────────────────────────────────────

#[test]
fn thumbnail_returns_a_jpeg_for_a_position() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);

    let out = d.data_dir().join("thumb.jpg");
    d.send(
        "thumbnail",
        json!({ "position": 30.0, "output": out.to_string_lossy() }),
    )
    .expect_ok();

    let bytes = std::fs::read(&out).expect("thumbnail file");
    assert!(bytes.len() > 300, "preview is suspiciously small: {} bytes", bytes.len());
    assert_eq!(&bytes[..3], &[0xff, 0xd8, 0xff], "output is not a JPEG");
}

#[test]
fn nearby_positions_share_one_preview_bucket() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);

    // The whole point of bucketing: scrubbing pixel by pixel must not mean
    // one ffmpeg process per pixel. Two positions a fraction of a second
    // apart should resolve to the same cached frame.
    let a = d.send("thumbnail", json!({ "position": 30.0 }));
    a.expect_ok();
    let b = d.send("thumbnail", json!({ "position": 30.2 }));
    b.expect_ok();

    assert_eq!(
        a.data()["position"], b.data()["position"],
        "positions within one bucket should return the same preview"
    );
    assert_eq!(a.data()["base64"], b.data()["base64"]);
}

#[test]
fn distant_positions_return_different_previews() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);

    let early = d.send("thumbnail", json!({ "position": 2.0 }));
    early.expect_ok();
    let late = d.send("thumbnail", json!({ "position": 50.0 }));
    late.expect_ok();

    assert_ne!(
        early.data()["position"], late.data()["position"],
        "positions this far apart must land in different buckets"
    );
}

#[test]
fn thumbnail_requires_a_position() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.plain);

    d.send("thumbnail", json!({}))
        .expect_err_containing("position is required");
}

#[test]
fn thumbnail_reports_a_clear_error_with_nothing_playing() {
    let d = Daemon::start();
    d.send("thumbnail", json!({ "position": 5.0 }))
        .expect_err_containing("nothing is playing");
}

// ─── Resume points ────────────────────────────────────────────────────────

#[test]
fn stopping_mid_file_saves_a_resume_point() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    d.send("seek", json!({ "seconds": 12.0 })).expect_ok();
    d.wait_for(|d| d.position() >= 12.0, "seek to the middle");
    d.send("stop", json!({})).expect_ok();

    let saved = d.send(
        "get_position",
        json!({ "path": f.with_chapters.to_string_lossy() }),
    );
    saved.expect_ok();
    let position = saved.data()["position"].as_f64().unwrap_or(0.0);
    assert!(position > 10.0, "expected a resume point past 10s, got {position}");
}

#[test]
fn finishing_a_file_forgets_its_resume_point() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    // Watch to the end, then stop. Saving a position here would make the
    // next play resume on the last frame and land straight back on EOF —
    // which reads as a player that refuses to play the file.
    d.send("seek", json!({ "seconds": 29.9 })).expect_ok();
    d.wait_for(|d| d.position() >= 29.0, "reach the end");
    d.send("stop", json!({})).expect_ok();

    let saved = d.send(
        "get_position",
        json!({ "path": f.with_chapters.to_string_lossy() }),
    );
    saved.expect_ok();
    let position = saved.data()["position"].as_f64();
    assert!(
        position.is_none() || position == Some(0.0),
        "a finished file should start over, but a resume point of {position:?} was kept"
    );
}

// ─── Recently played ──────────────────────────────────────────────────────

#[test]
fn playing_a_file_records_it_even_when_it_was_never_scanned() {
    let f = fixtures();
    let d = Daemon::start();

    // The regression this guards: `record_play` used to be a bare UPDATE,
    // so a file opened directly — which is most of what anyone watches —
    // matched no library row and left no history at all.
    // Playing is enough — history is written by the play itself, not left
    // to each caller to remember.
    d.play(&f.with_chapters);

    let recent = d.send("recent_list", json!({})).expect_ok().data();
    let rows = recent.as_array().expect("recent list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["title"], "chapters");
    assert_eq!(rows[0]["play_count"], 1);
}

/// A source that mpv can open but never reads a frame from is not a play, and
/// must not be offered back on the first screen.
///
/// Unix only: the repro is a FIFO, which Windows has no equivalent of. The rule
/// it guards is platform-independent and the code under test is one branch for
/// every platform, so covering it here covers it everywhere.
///
/// The bug this guards was found by driving the window: the history row was
/// written at the play call site, before `loaded` had been decided, so anything
/// that did not fail *fast* got recorded — a share that had gone away, a
/// mistyped path behind a mount, or the FIFO used here, which is the same shape
/// and deterministic. The entry was permanent and failed again when clicked.
///
/// A FIFO with no writer is the only reliable way to sit in "still opening":
/// mpv opens it, then waits forever for bytes that never come.
#[cfg(unix)]
#[test]
fn a_source_that_never_opens_stays_out_of_the_history() {
    let f = fixtures();
    let d = Daemon::start();

    let stall = d.data_dir().join("stall.mkv");
    let path = std::ffi::CString::new(stall.to_str().unwrap()).unwrap();
    // SAFETY: a path inside this test's own data dir, which no other test
    // and no user shares.
    let made = unsafe { libc::mkfifo(path.as_ptr(), 0o600) };
    assert_eq!(made, 0, "could not create the FIFO the test needs");

    let reply = d.send("play", json!({ "file": stall.to_str().unwrap() }));
    reply.expect_ok();
    assert_eq!(
        reply.data()["loaded"],
        false,
        "a FIFO nobody writes to cannot report itself loaded"
    );

    let recent = d.send("recent_list", json!({})).expect_ok().data();
    assert_eq!(
        recent.as_array().expect("recent list").len(),
        0,
        "something still opening was written to the history"
    );

    // The other half of the rule, and the reason the fix is not "only record
    // Loaded": a file that does open is still recorded, immediately.
    d.play(&f.with_chapters);
    let recent = d.send("recent_list", json!({})).expect_ok().data();
    let rows = recent.as_array().expect("recent list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["title"], "chapters");
}

#[test]
fn recent_is_ordered_newest_first_and_counts_repeats() {
    let f = fixtures();
    let d = Daemon::start();

    for path in [&f.with_chapters, &f.plain] {
        d.send("record_play", json!({ "path": path.to_string_lossy() }))
            .expect_ok();
        // SQLite's datetime('now') has one-second resolution, so two plays
        // in the same second would tie and order arbitrarily.
        std::thread::sleep(std::time::Duration::from_millis(1100));
    }
    d.send("record_play", json!({ "path": f.with_chapters.to_string_lossy() }))
        .expect_ok();

    let recent = d.send("recent_list", json!({})).expect_ok().data();
    let rows = recent.as_array().unwrap();
    assert_eq!(rows.len(), 2, "same file played twice is still one entry");
    assert_eq!(rows[0]["title"], "chapters", "most recent comes first");
    assert_eq!(rows[0]["play_count"], 2);
}

#[test]
fn recent_honours_its_limit() {
    let f = fixtures();
    let d = Daemon::start();
    for path in [&f.with_chapters, &f.plain, &f.with_subtitles] {
        d.send("record_play", json!({ "path": path.to_string_lossy() }))
            .expect_ok();
    }
    let capped = d.send("recent_list", json!({ "limit": 2 })).expect_ok().data();
    assert_eq!(capped.as_array().unwrap().len(), 2);
}

#[test]
fn clearing_history_keeps_scanned_metadata() {
    let f = fixtures();
    let d = Daemon::start();
    let dir = f.with_chapters.parent().unwrap();
    d.send("library_scan", json!({ "dir": dir.to_string_lossy() })).expect_ok();
    d.send("record_play", json!({ "path": f.with_chapters.to_string_lossy() }))
        .expect_ok();

    d.send("recent_clear", json!({})).expect_ok();

    assert!(
        d.send("recent_list", json!({})).expect_ok().data().as_array().unwrap().is_empty(),
        "history should be empty after a clear"
    );
    // Forgetting *when* you watched something shouldn't discard the scan.
    let library = d.send("library_list", json!({})).expect_ok().data();
    assert!(
        !library.as_array().unwrap().is_empty(),
        "clearing history must not empty the library"
    );
}

#[test]
fn incognito_keeps_a_cli_play_out_of_the_history() {
    let f = fixtures();
    let d = Daemon::start();

    let on = d.send("incognito", json!({ "enabled": true }));
    on.expect_ok();
    assert_eq!(on.data()["enabled"], true);

    // The leak this guards: since the window hosts the control server, a
    // play started from a terminal reaches the same player the user has
    // incognito switched on for.
    d.play(&f.with_chapters);
    d.send("record_play", json!({ "path": f.plain.to_string_lossy() }))
        .expect_ok();

    assert!(
        d.send("recent_list", json!({})).expect_ok().data().as_array().unwrap().is_empty(),
        "nothing should be recorded while incognito is on"
    );

    // And switching it back off resumes recording.
    d.send("incognito", json!({ "enabled": false })).expect_ok();
    d.play(&f.plain);
    assert_eq!(
        d.send("recent_list", json!({})).expect_ok().data().as_array().unwrap().len(),
        1
    );
}

// ─── Picture geometry ─────────────────────────────────────────────────────

#[test]
fn video_transform_reports_defaults_for_an_untouched_file() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    let v = d.send("video_get", json!({})).expect_ok().data();
    assert_eq!(v["aspect"], "auto");
    assert_eq!(v["rotate"], 0);
    assert!((v["zoom"].as_f64().unwrap() - 1.0).abs() < 1e-6, "zoom starts at 1x");
    assert_eq!(v["deinterlace"], false);
}

#[test]
fn aspect_accepts_a_ratio_and_returns_to_auto() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    let set = d.send("video_set", json!({ "name": "aspect", "value": "16:9" }));
    set.expect_ok();
    let ratio: f64 = set.data()["aspect"].as_str().unwrap().parse().unwrap();
    assert!((ratio - 16.0 / 9.0).abs() < 1e-3, "got {ratio}");

    let auto = d.send("video_set", json!({ "name": "aspect", "value": "auto" }));
    auto.expect_ok();
    assert_eq!(auto.data()["aspect"], "auto");
}

#[test]
fn rotation_takes_right_angles_and_wraps() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    let r = d.send("video_set", json!({ "name": "rotate", "value": 90 }));
    r.expect_ok();
    assert_eq!(r.data()["rotate"], 90);

    // 360 is 0, and -90 is 270 — both are things a caller may compute.
    let wrapped = d.send("video_set", json!({ "name": "rotate", "value": -90 }));
    wrapped.expect_ok();
    assert_eq!(wrapped.data()["rotate"], 270);

    d.send("video_set", json!({ "name": "rotate", "value": 45 }))
        .expect_err_containing("0, 90, 180 or 270");
}

#[test]
fn zoom_is_a_plain_multiplier_and_is_clamped() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    // mpv stores log2 of this; the API deliberately doesn't make callers
    // think in logarithms.
    let z = d.send("video_set", json!({ "name": "zoom", "value": 2.0 }));
    z.expect_ok();
    assert!((z.data()["zoom"].as_f64().unwrap() - 2.0).abs() < 1e-6);

    let huge = d.send("video_set", json!({ "name": "zoom", "value": 500.0 }));
    huge.expect_ok();
    assert!(
        huge.data()["zoom"].as_f64().unwrap() <= 8.0,
        "zoom should clamp rather than blow the picture up beyond recovery"
    );

    d.send("video_set", json!({ "name": "zoom", "value": 0.0 }))
        .expect_err_containing("positive");
}

#[test]
fn reset_restores_every_geometry_knob_at_once() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    d.send("video_set", json!({ "name": "aspect", "value": "4:3" })).expect_ok();
    d.send("video_set", json!({ "name": "rotate", "value": 180 })).expect_ok();
    d.send("video_set", json!({ "name": "zoom", "value": 1.5 })).expect_ok();
    d.send("video_set", json!({ "name": "panscan", "value": 0.5 })).expect_ok();
    d.send("video_set", json!({ "name": "deinterlace", "value": true })).expect_ok();

    let v = d.send("video_reset", json!({})).expect_ok().data();
    assert_eq!(v["aspect"], "auto");
    assert_eq!(v["rotate"], 0);
    assert!((v["zoom"].as_f64().unwrap() - 1.0).abs() < 1e-6);
    assert!((v["panscan"].as_f64().unwrap()).abs() < 1e-6);
    assert_eq!(v["deinterlace"], false);
}

#[test]
fn video_transform_rejects_unknown_properties() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    d.send("video_set", json!({ "name": "skew", "value": 1 }))
        .expect_err_containing("unknown video transform");
    d.send("video_set", json!({ "name": "aspect", "value": "widescreen" }))
        .expect_err_containing("bad aspect");
}

// --- Audio processing (v0.12) ---------------------------------------------
//
// The equaliser is the one feature here whose effect can't be asserted from
// the outside: nothing reports "the 500 Hz band is 6 dB louder now". What can
// be asserted is that mpv accepted the filters and is running the ones we
// asked for, which is exactly where this breaks in practice - a filter name
// the bundled ffmpeg lacks, or a number formatted in a way its parser rejects.
// So these tests read mpv's own `af` back.

#[test]
fn equalizer_starts_flat_and_off() {
    let d = Daemon::start();
    let data = d.send("audio_eq_get", json!({})).expect_ok().data();

    assert_eq!(data["enabled"], false);
    assert_eq!(data["normalize"], false);
    assert_eq!(data["flat"], true);
    assert_eq!(data["bands"].as_array().unwrap().len(), 10);
    assert_eq!(
        data["frequencies"].as_array().unwrap().len(),
        10,
        "callers label sliders from this rather than hardcoding our table"
    );
    assert_eq!(data["chain"], "", "nothing enabled means no filters in mpv");
}

#[test]
fn enabling_the_equalizer_installs_filters_mpv_actually_runs() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    d.send("audio_eq_set", json!({"enabled": true})).expect_ok();
    let chain = d.send("audio_eq_get", json!({})).data()["chain"]
        .as_str()
        .unwrap_or("")
        .to_string();

    // Ten bands, each a real filter in mpv's chain. If the bundled ffmpeg
    // lacked `equalizer` this is where it would show up.
    assert_eq!(
        chain.matches("equalizer=").count(),
        10,
        "mpv reports: {}",
        chain
    );
}

#[test]
fn a_band_change_reaches_mpv_with_the_right_frequency() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    d.send("audio_eq_set", json!({"enabled": true, "band": 4, "gain": -6.5}))
        .expect_ok();

    let chain = mpv_unescape(
        d.send("audio_eq_get", json!({})).data()["chain"]
            .as_str()
            .unwrap_or(""),
    );
    // Band 4 is 500 Hz. A negative, fractional gain covers the two formatting
    // traps at once: a leading minus and a decimal point.
    assert!(
        chain.contains("f=500") && chain.contains("g=-6.5"),
        "mpv reports: {}",
        chain
    );
}

#[test]
fn bands_are_clamped_rather_than_passed_through() {
    let d = Daemon::start();
    let data = d
        .send("audio_eq_set", json!({"enabled": true, "band": 0, "gain": 999.0}))
        .expect_ok()
        .data();
    assert_eq!(data["bands"][0], 12.0);
}

#[test]
fn an_out_of_range_band_says_what_the_range_is() {
    let d = Daemon::start();
    d.send("audio_eq_set", json!({"band": 10, "gain": 1.0}))
        .expect_err_containing("band must be 0-9");
}

#[test]
fn setting_a_band_without_a_gain_is_refused() {
    // Otherwise it silently reads as "set band 3 to 0 dB", which is a
    // destructive interpretation of a typo.
    let d = Daemon::start();
    d.send("audio_eq_set", json!({"band": 3}))
        .expect_err_containing("gain required");
}

#[test]
fn an_empty_set_is_refused_rather_than_treated_as_a_reset() {
    let d = Daemon::start();
    d.send("audio_eq_set", json!({}))
        .expect_err_containing("nothing to set");
}

#[test]
fn turning_the_equalizer_off_keeps_the_curve_but_clears_the_filters() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    d.send("audio_eq_set", json!({"enabled": true, "band": 2, "gain": 7.0}))
        .expect_ok();
    let data = d
        .send("audio_eq_set", json!({"enabled": false}))
        .expect_ok()
        .data();

    // The A/B case: the user wants to compare, not to start over.
    assert_eq!(data["bands"][2], 7.0, "the curve must survive the toggle");
    assert_eq!(data["chain"], "", "but nothing should still be filtering");
}

#[test]
fn presets_apply_a_curve_and_switch_the_equalizer_on() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    let data = d
        .send("audio_eq_preset", json!({"name": "speech"}))
        .expect_ok()
        .data();

    assert_eq!(data["enabled"], true, "choosing a curve means wanting to hear it");
    assert_eq!(data["flat"], false);
    assert!(data["chain"].as_str().unwrap().contains("equalizer="));
}

#[test]
fn an_unknown_preset_lists_the_real_ones() {
    let d = Daemon::start();
    d.send("audio_eq_preset", json!({"name": "loudness-war"}))
        .expect_err_containing("speech");
}

#[test]
fn presets_are_listed_with_descriptions() {
    let d = Daemon::start();
    let data = d.send("audio_eq_presets", json!({})).expect_ok().data();
    let list = data.as_array().expect("preset array");

    assert!(list.len() >= 5);
    for p in list {
        assert!(!p["name"].as_str().unwrap_or("").is_empty());
        assert!(
            !p["description"].as_str().unwrap_or("").is_empty(),
            "a preset nobody can tell apart from the next one is not useful"
        );
        assert_eq!(p["bands"].as_array().unwrap().len(), 10);
    }
}

#[test]
fn normalization_works_on_its_own() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    let data = d
        .send("audio_eq_set", json!({"normalize": true}))
        .expect_ok()
        .data();
    let chain = data["chain"].as_str().unwrap();

    assert!(chain.contains("dynaudnorm"), "mpv reports: {}", chain);
    assert!(
        !chain.contains("equalizer="),
        "normalisation shouldn't drag the equaliser in: {}",
        chain
    );
}

#[test]
fn reset_clears_everything_in_mpv_and_in_our_state() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    d.send("audio_eq_preset", json!({"name": "bass"})).expect_ok();
    d.send("audio_eq_set", json!({"normalize": true, "preamp": -4.0}))
        .expect_ok();

    let data = d.send("audio_eq_reset", json!({})).expect_ok().data();
    assert_eq!(data["enabled"], false);
    assert_eq!(data["normalize"], false);
    assert_eq!(data["preamp"], 0.0);
    assert_eq!(data["flat"], true);
    assert_eq!(data["chain"], "");
}

#[test]
fn the_equalizer_survives_a_restart() {
    // The whole point of persisting it. A curve that silently resets when the
    // player restarts is worse than one that was never saved, because the
    // user only finds out much later.
    let d = Daemon::start();
    d.send("audio_eq_preset", json!({"name": "night"})).expect_ok();
    let before = d.send("audio_eq_get", json!({})).data()["bands"].clone();

    let d = d.restart();
    let after = d.send("audio_eq_get", json!({})).expect_ok().data();

    assert_eq!(after["bands"], before);
    assert_eq!(after["enabled"], true);
}

#[test]
fn a_restored_curve_is_applied_to_the_next_file() {
    // State surviving isn't enough - it has to reach mpv. The settings are
    // loaded when the Player is built, long before there is an audio chain to
    // put filters in, so the apply happens on play.
    let f = fixtures();
    let d = Daemon::start();
    d.send("audio_eq_preset", json!({"name": "bass"})).expect_ok();

    let d = d.restart();
    d.play(&f.with_chapters);

    let chain = d.send("audio_eq_get", json!({})).data()["chain"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(
        chain.contains("equalizer="),
        "restored curve never reached mpv: {:?}",
        chain
    );
}

#[test]
fn pitch_correction_reads_and_toggles() {
    let d = Daemon::start();
    // On by default is what makes 1.5x speech listenable; if this ever flips
    // it is a regression, not a preference.
    assert_eq!(d.send("audio_pitch", json!({})).expect_ok().data()["enabled"], true);

    let off = d.send("audio_pitch", json!({"enabled": false}));
    off.expect_ok();
    assert_eq!(off.data()["enabled"], false);

    assert_eq!(
        d.send("audio_pitch", json!({})).data()["enabled"],
        false,
        "the setting should stick"
    );
}

#[test]
fn mcp_exposes_the_audio_tools() {
    let d = Daemon::start();
    let replies = mcp_roundtrip(
        &[json!({ "jsonrpc": "2.0", "id": 11, "method": "tools/list", "params": {} })],
        &d,
    );
    let tools = replies[&11]["result"]["tools"].as_array().unwrap().clone();
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();

    for expected in [
        "equalizer_get",
        "equalizer_set",
        "equalizer_preset",
        "equalizer_presets",
        "equalizer_reset",
        "pitch_correction",
    ] {
        assert!(names.contains(&expected), "MCP is missing `{expected}`");
    }

    let set = tools.iter().find(|t| t["name"] == "equalizer_set").unwrap();
    // Callable with any single knob, so nothing may be required.
    assert!(set["inputSchema"].get("required").is_none());
    let preset = tools.iter().find(|t| t["name"] == "equalizer_preset").unwrap();
    assert_eq!(preset["inputSchema"]["required"][0], "name");
}

#[test]
fn mcp_can_drive_the_equalizer_end_to_end() {
    let d = Daemon::start();
    let replies = mcp_roundtrip(
        &[json!({
            "jsonrpc": "2.0", "id": 12, "method": "tools/call",
            "params": { "name": "equalizer_preset", "arguments": { "name": "speech" } }
        })],
        &d,
    );
    let result = &replies[&12]["result"];
    assert_ne!(result["isError"], true, "{result}");

    // And the change reached the same player the CLI talks to.
    let data = d.send("audio_eq_get", json!({})).expect_ok().data();
    assert_eq!(data["enabled"], true);
    assert_eq!(data["flat"], false);
}

// ─── Bookmarks ────────────────────────────────────────────────────────────

#[test]
fn a_bookmark_defaults_to_where_playback_is() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    d.send("seek", json!({ "seconds": 12.0 })).expect_ok();
    d.wait_for(|d| d.position() >= 12.0, "seek to the middle");

    let saved = d.send("bookmark_add", json!({})).expect_ok().data();
    let position = saved["position"].as_f64().unwrap();
    assert!(
        (12.0..15.0).contains(&position),
        "expected the bookmark near 12s, got {position}"
    );
    assert!(saved["name"].is_null(), "an unnamed add must not invent a name");
    assert_eq!(saved["path"], f.with_chapters.to_string_lossy().as_ref());
}

#[test]
fn a_second_bookmark_at_the_same_spot_corrects_the_first() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    // A held key, or a user unsure the first press registered. Two entries
    // a fraction of a second apart are not two places in the film.
    let first = d
        .send("bookmark_add", json!({ "position": 8.0 }))
        .expect_ok()
        .data();
    let second = d
        .send("bookmark_add", json!({ "position": 8.4, "name": "Here" }))
        .expect_ok()
        .data();

    assert_eq!(first["id"], second["id"]);
    assert_eq!(second["name"], "Here");
    let list = d.send("bookmark_list", json!({})).expect_ok().data();
    assert_eq!(list.as_array().unwrap().len(), 1);
}

#[test]
fn an_unnamed_add_never_strips_a_name_already_there() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    d.send("bookmark_add", json!({ "position": 8.0, "name": "The bit" }))
        .expect_ok();
    let again = d
        .send("bookmark_add", json!({ "position": 8.2 }))
        .expect_ok()
        .data();
    assert_eq!(again["name"], "The bit");
}

#[test]
fn bookmarks_come_back_in_timeline_order() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    for position in [22.0, 4.0, 13.0] {
        d.send("bookmark_add", json!({ "position": position })).expect_ok();
    }

    let list = d.send("bookmark_list", json!({})).expect_ok().data();
    let positions: Vec<f64> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["position"].as_f64().unwrap())
        .collect();
    assert_eq!(positions, vec![4.0, 13.0, 22.0]);
}

#[test]
fn bookmarking_a_file_that_isnt_playing_needs_a_position() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    // "Now" means nothing for some other file, and 0 would be a lie
    // dressed as a value.
    d.send("bookmark_add", json!({ "file": f.plain.to_string_lossy() }))
        .expect_err_containing("position");

    d.send(
        "bookmark_add",
        json!({ "file": f.plain.to_string_lossy(), "position": 3.0 }),
    )
    .expect_ok();
}

#[test]
fn goto_seeks_within_the_file_already_playing() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    let id = d
        .send("bookmark_add", json!({ "position": 21.0, "name": "Finale" }))
        .expect_ok()
        .data()["id"]
        .as_i64()
        .unwrap();
    d.send("seek", json!({ "seconds": 2.0 })).expect_ok();
    d.wait_for(|d| d.position() < 5.0, "seek back to the start");

    d.send("bookmark_goto", json!({ "id": id })).expect_ok();
    d.wait_for(|d| d.position() >= 21.0, "jump to the bookmark");
    assert_eq!(d.status()["file"], f.with_chapters.to_string_lossy().as_ref());
}

#[test]
fn goto_opens_the_bookmarked_file_when_it_is_a_different_one() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    let id = d
        .send(
            "bookmark_add",
            json!({ "file": f.plain.to_string_lossy(), "position": 9.0 }),
        )
        .expect_ok()
        .data()["id"]
        .as_i64()
        .unwrap();

    d.send("bookmark_goto", json!({ "id": id })).expect_ok();
    d.wait_for(
        |d| d.status()["file"] == f.plain.to_string_lossy().as_ref(),
        "the other file to load",
    );
    d.wait_for(|d| d.position() >= 9.0, "open at the bookmarked position");

    // Switching files through `play` also leaves the outgoing one resumable.
    d.send(
        "get_position",
        json!({ "path": f.with_chapters.to_string_lossy() }),
    )
    .expect_ok();
}

#[test]
fn renaming_with_no_name_takes_the_label_back_off() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    let id = d
        .send("bookmark_add", json!({ "position": 6.0, "name": "Typo" }))
        .expect_ok()
        .data()["id"]
        .as_i64()
        .unwrap();

    let renamed = d
        .send("bookmark_rename", json!({ "id": id, "name": "Fixed" }))
        .expect_ok()
        .data();
    assert_eq!(renamed["name"], "Fixed");

    let cleared = d
        .send("bookmark_rename", json!({ "id": id }))
        .expect_ok()
        .data();
    assert!(cleared["name"].is_null());
}

#[test]
fn clearing_without_a_scope_only_touches_the_current_file() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    d.send("bookmark_add", json!({ "position": 5.0 })).expect_ok();
    d.send(
        "bookmark_add",
        json!({ "file": f.plain.to_string_lossy(), "position": 5.0 }),
    )
    .expect_ok();

    d.send("bookmark_clear", json!({})).expect_ok();
    assert_eq!(
        d.send("bookmark_list", json!({}))
            .expect_ok()
            .data()
            .as_array()
            .unwrap()
            .len(),
        0
    );
    let all = d
        .send("bookmark_list", json!({ "all": true }))
        .expect_ok()
        .data();
    assert_eq!(
        all.as_array().unwrap().len(),
        1,
        "the other file kept its bookmark"
    );
}

#[test]
fn with_nothing_playing_the_wide_scope_has_to_be_asked_for() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);
    d.send("bookmark_add", json!({ "position": 5.0 })).expect_ok();
    d.send("stop", json!({})).expect_ok();
    // mpv keeps reporting the path for a moment after `stop` is accepted,
    // so wait for the player to actually be empty rather than racing it.
    d.wait_for(|d| d.status()["file"].is_null(), "playback to stop");

    // `bookmark clear` guessing "everything" on a mistimed call would
    // delete the lot, so the wide scope is never reached by default.
    d.send("bookmark_clear", json!({}))
        .expect_err_containing("nothing is playing");
    d.send("bookmark_list", json!({}))
        .expect_err_containing("nothing is playing");
    assert_eq!(
        d.send("bookmark_list", json!({ "all": true }))
            .expect_ok()
            .data()
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn an_unknown_bookmark_id_says_so_rather_than_reporting_success() {
    let d = Daemon::start();
    d.send("bookmark_remove", json!({ "id": 4242 }))
        .expect_err_containing("4242");
    d.send("bookmark_goto", json!({ "id": 4242 }))
        .expect_err_containing("4242");
    d.send("bookmark_rename", json!({ "id": 4242, "name": "x" }))
        .expect_err_containing("4242");
}

#[test]
fn bookmarks_survive_a_restart() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);
    d.send("bookmark_add", json!({ "position": 17.0, "name": "Kept" }))
        .expect_ok();

    let d = d.restart();
    let list = d
        .send("bookmark_list", json!({ "all": true }))
        .expect_ok()
        .data();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["name"], "Kept");
    assert_eq!(list[0]["position"], 17.0);
}

#[test]
fn mcp_exposes_the_bookmark_tools() {
    let d = Daemon::start();
    let replies = mcp_roundtrip(
        &[json!({ "jsonrpc": "2.0", "id": 20, "method": "tools/list" })],
        &d,
    );
    let tools = replies[&20]["result"]["tools"].as_array().unwrap().clone();
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    for expected in [
        "bookmark_add",
        "bookmark_list",
        "bookmark_goto",
        "bookmark_rename",
        "bookmark_remove",
        "bookmark_clear",
    ] {
        assert!(names.contains(&expected), "MCP is missing `{expected}`");
    }

    // Adding must be callable with no arguments at all — "bookmark this
    // moment" is the whole point of it.
    let add = tools.iter().find(|t| t["name"] == "bookmark_add").unwrap();
    assert!(add["inputSchema"].get("required").is_none());
    let goto = tools.iter().find(|t| t["name"] == "bookmark_goto").unwrap();
    assert_eq!(goto["inputSchema"]["required"][0], "id");
}

#[test]
fn mcp_can_bookmark_and_jump_back() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    let replies = mcp_roundtrip(
        &[json!({
            "jsonrpc": "2.0", "id": 21, "method": "tools/call",
            "params": {
                "name": "bookmark_add",
                "arguments": { "position": 19.0, "name": "From MCP" }
            }
        })],
        &d,
    );
    assert_ne!(replies[&21]["result"]["isError"], true, "{}", replies[&21]);

    // The same player the CLI talks to, not a second invisible one.
    let list = d.send("bookmark_list", json!({})).expect_ok().data();
    assert_eq!(list[0]["name"], "From MCP");
    let id = list[0]["id"].as_i64().unwrap();

    d.send("seek", json!({ "seconds": 1.0 })).expect_ok();
    d.wait_for(|d| d.position() < 5.0, "seek back to the start");
    let replies = mcp_roundtrip(
        &[json!({
            "jsonrpc": "2.0", "id": 22, "method": "tools/call",
            "params": { "name": "bookmark_goto", "arguments": { "id": id } }
        })],
        &d,
    );
    assert_ne!(replies[&22]["result"]["isError"], true, "{}", replies[&22]);
    d.wait_for(|d| d.position() >= 19.0, "MCP jump to the bookmark");
}

// ─── A disc has an identity ───────────────────────────────────────────────
//
// Everything unflick remembers used to be keyed by the path it was given,
// and a mounted disc's path is the drive it is in. So a bookmark left on one
// DVD was offered on the next one put in that drive, and its resume point
// was applied to it. These drive the real binary against a directory whose
// contents get swapped, which is what a drive is.
//
// None of these *play* a disc: the fixtures are a VIDEO_TS with a plausible
// index in it, not a real DVD, and libdvdnav would rightly refuse. What is
// under test is the bookkeeping either side of that — which is where the bug
// was, and which is the part that has no disc-shaped hardware requirement.

/// Put the first film in the drive.
fn insert_first(drive: &common::FakeDrive) {
    drive.insert_dvd(b"VMG for the first film -- 1999", 4096);
}

/// Take it out and put a different one in. Same path, different disc.
fn insert_second(drive: &common::FakeDrive) {
    drive.insert_dvd(b"VMG for the second film -- 2003", 8192);
}

#[test]
fn two_discs_in_one_drive_do_not_see_each_others_bookmarks() {
    let drive = common::FakeDrive::new("disc-bookmarks");
    let d = Daemon::start();

    insert_first(&drive);
    d.send(
        "bookmark_add",
        json!({ "file": drive.path(), "position": 300.0, "name": "the good bit" }),
    )
    .expect_ok();

    insert_second(&drive);
    let list = d
        .send("bookmark_list", json!({ "file": drive.path() }))
        .expect_ok()
        .data();
    assert_eq!(
        list.as_array().unwrap().len(),
        0,
        "the second disc was offered the first one's bookmarks: {}",
        list
    );

    d.send(
        "bookmark_add",
        json!({ "file": drive.path(), "position": 120.0, "name": "on the other disc" }),
    )
    .expect_ok();

    insert_first(&drive);
    let list = d
        .send("bookmark_list", json!({ "file": drive.path() }))
        .expect_ok()
        .data();
    let names: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|b| b["name"].as_str())
        .collect();
    assert_eq!(names, vec!["the good bit"], "got {}", list);

    // Both are still there, sharing one path and separated by their keys —
    // which is the shape of the fix, visible from outside.
    let all = d
        .send("bookmark_list", json!({ "all": true }))
        .expect_ok()
        .data();
    let all = all.as_array().unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0]["path"], all[1]["path"], "same drive");
    assert_ne!(all[0]["key"], all[1]["key"], "different discs");
    for b in all {
        assert!(
            b["key"].as_str().unwrap().starts_with("disc:dvd:"),
            "expected a disc identity, got {}",
            b["key"]
        );
    }

    // Scoping by an identity outright must not claim its own rows are
    // orphans: `--key` names no drive, so there is nothing filed under one.
    // Caught by driving this by hand — the note counted the very bookmarks
    // it had just listed.
    let key = all[0]["key"].as_str().unwrap().to_string();
    let by_key = d.send("bookmark_list", json!({ "key": key }));
    by_key.expect_ok();
    assert_eq!(by_key.message(), "1 bookmark(s)", "{}", by_key.message());
}

#[test]
fn a_resume_point_is_not_offered_to_the_next_disc_in_the_drive() {
    let drive = common::FakeDrive::new("disc-resume");
    let d = Daemon::start();

    insert_first(&drive);
    d.send("save_position", json!({ "path": drive.path(), "position": 640.0 }))
        .expect_ok();

    insert_second(&drive);
    let pos = d
        .send("get_position", json!({ "path": drive.path() }))
        .expect_ok()
        .data();
    assert!(
        pos["position"].is_null(),
        "the next disc was dropped 10 minutes into someone else's film: {}",
        pos
    );

    insert_first(&drive);
    let pos = d
        .send("get_position", json!({ "path": drive.path() }))
        .expect_ok()
        .data();
    assert_eq!(pos["position"], 640.0, "the first disc lost its place");
}

#[test]
fn two_discs_from_one_drive_are_two_entries_in_the_history() {
    let drive = common::FakeDrive::new("disc-history");
    let d = Daemon::start();

    insert_first(&drive);
    d.send("record_play", json!({ "path": drive.path() })).expect_ok();
    insert_second(&drive);
    d.send("record_play", json!({ "path": drive.path() })).expect_ok();

    let recent = d.send("recent_list", json!({})).expect_ok().data();
    let rows = recent.as_array().expect("recent is a list");
    assert_eq!(rows.len(), 2, "two discs collapsed into one entry: {}", recent);
    assert_eq!(rows[0]["path"], rows[1]["path"], "same drive");
    assert_ne!(rows[0]["key"], rows[1]["key"], "different discs");
}

#[test]
fn a_bookmark_on_another_disc_is_refused_before_anything_is_loaded() {
    let drive = common::FakeDrive::new("disc-goto");
    let d = Daemon::start();

    insert_first(&drive);
    let b = d
        .send(
            "bookmark_add",
            json!({ "file": drive.path(), "position": 300.0, "name": "the good bit" }),
        )
        .expect_ok()
        .data();
    let id = b["id"].as_i64().unwrap();

    insert_second(&drive);
    let reply = d.send("bookmark_goto", json!({ "id": id }));
    reply.expect_err_containing("another disc");
    assert!(
        reply.message().contains(&drive.path()),
        "the refusal should say which drive to put it back in: {}",
        reply.message()
    );

    // And nothing started playing on the way to saying so.
    assert!(
        d.status()["file"].is_null(),
        "the wrong disc was loaded anyway: {}",
        d.status()
    );
}

#[test]
fn an_ordinary_file_keeps_the_whole_bookmark_lifecycle() {
    // The other half of the promise: a file behaves exactly as it did, and
    // its key is its path.
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    let b = d
        .send("bookmark_add", json!({ "position": 21.0, "name": "Finale" }))
        .expect_ok()
        .data();
    assert_eq!(b["key"], b["path"]);
    assert_eq!(b["path"], f.with_chapters.to_string_lossy().into_owned());

    let list = d.send("bookmark_list", json!({})).expect_ok().data();
    assert_eq!(list.as_array().unwrap().len(), 1);
    // A plain file has nothing orphaned under it, so nothing is appended.
    let listed = d.send("bookmark_list", json!({}));
    assert_eq!(listed.message(), "1 bookmark(s)");

    d.send("seek", json!({ "seconds": 1.0 })).expect_ok();
    d.wait_for(|d| d.position() < 5.0, "seek back to the start");
    d.send("bookmark_goto", json!({ "id": b["id"].as_i64().unwrap() }))
        .expect_ok();
    d.wait_for(|d| d.position() >= 21.0, "jump to the bookmark");

    let cleared = d.send("bookmark_clear", json!({})).expect_ok().data();
    assert_eq!(cleared["cleared"], 1);
}

#[test]
fn an_image_is_still_keyed_by_its_path() {
    // A `.iso` has a path that means one thing forever — the case the DVD
    // work was already correct about, and must stay correct about.
    let dir = common::FakeDrive::new("disc-images");
    let one = std::path::Path::new(&dir.path()).join("first.iso");
    let two = std::path::Path::new(&dir.path()).join("second.iso");
    std::fs::write(&one, b"not really an image").unwrap();
    std::fs::write(&two, b"nor is this one").unwrap();

    let d = Daemon::start();
    for (path, name) in [(&one, "in the first"), (&two, "in the second")] {
        d.send(
            "bookmark_add",
            json!({ "file": path.to_string_lossy(), "position": 10.0, "name": name }),
        )
        .expect_ok();
    }

    for (path, name) in [(&one, "in the first"), (&two, "in the second")] {
        let list = d
            .send("bookmark_list", json!({ "file": path.to_string_lossy() }))
            .expect_ok()
            .data();
        let rows = list.as_array().unwrap();
        assert_eq!(rows.len(), 1, "images leaked into each other: {}", list);
        assert_eq!(rows[0]["name"], name);
        assert_eq!(rows[0]["key"], path.to_string_lossy().into_owned());
    }
}

#[test]
fn bookmarks_left_under_a_drive_letter_are_migrated_and_announced() {
    // The rows that already exist in a real user's database: several discs'
    // bookmarks piled under one drive path, unattributable. They are not
    // deleted, not merged into an invented "unknown disc", and not offered
    // to whatever disc is in the drive now — but they are reachable, and
    // the answer says so.
    let drive = common::FakeDrive::new("disc-migration");
    insert_first(&drive);
    let drive_path = drive.path();

    let d = Daemon::start_seeded(|data_dir| {
        let conn = rusqlite::Connection::open(data_dir.join("library.db"))
            .expect("write a pre-migration database");
        conn.execute_batch(
            "
            CREATE TABLE media (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL,
                duration REAL, width INTEGER, height INTEGER,
                video_codec TEXT, audio_codec TEXT, file_size INTEGER,
                added_at TEXT NOT NULL DEFAULT (datetime('now')),
                last_played TEXT,
                play_count INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE playback_position (
                path TEXT PRIMARY KEY,
                position REAL NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE bookmark (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL,
                position REAL NOT NULL,
                name TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE session (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                path TEXT NOT NULL,
                position REAL NOT NULL,
                duration REAL NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            ",
        )
        .expect("legacy schema");
        conn.execute(
            "INSERT INTO bookmark (path, position, name) VALUES (?1, 300.0, 'from before')",
            [&drive_path],
        )
        .expect("legacy bookmark");
    });

    // Not offered to the disc in the drive.
    let scoped = d.send("bookmark_list", json!({ "file": drive.path() }));
    scoped.expect_ok();
    assert_eq!(scoped.data().as_array().unwrap().len(), 0, "{}", scoped.data());
    // But announced, with the command that reaches them.
    assert!(
        scoped.message().contains("before discs had an identity")
            && scoped.message().contains("bookmark list --key"),
        "the orphaned bookmark vanished without a word: {}",
        scoped.message()
    );

    // Still there, still reachable by the key it was migrated under.
    let by_key = d.send("bookmark_list", json!({ "key": drive.path() }));
    by_key.expect_ok();
    let by_key = by_key.data();
    assert_eq!(by_key.as_array().unwrap().len(), 1, "{}", by_key);
    assert_eq!(by_key[0]["name"], "from before");
    assert_eq!(by_key[0]["key"], drive.path());

    let all = d
        .send("bookmark_list", json!({ "all": true }))
        .expect_ok()
        .data();
    assert_eq!(all.as_array().unwrap().len(), 1);

    // And deletable on purpose, which is the other half of orphaning
    // rather than hiding.
    let cleared = d
        .send("bookmark_clear", json!({ "key": drive.path() }))
        .expect_ok()
        .data();
    assert_eq!(cleared["cleared"], 1);
}

#[test]
fn mcp_can_reach_a_disc_identity_and_the_bookmarks_under_a_drive() {
    let drive = common::FakeDrive::new("disc-mcp");
    insert_first(&drive);
    let d = Daemon::start();

    let replies = mcp_roundtrip(
        &[
            json!({
                "jsonrpc": "2.0", "id": 30, "method": "tools/call",
                "params": { "name": "disc_list", "arguments": { "path": drive.path() } }
            }),
            json!({ "jsonrpc": "2.0", "id": 31, "method": "tools/list" }),
        ],
        &d,
    );

    let text = replies[&30]["result"]["content"][0]["text"]
        .as_str()
        .expect("disc_list returns text");
    let payload: serde_json::Value = serde_json::from_str(text).expect("disc_list JSON");
    let key = payload["key"].as_str().expect("a disc reports its key");
    assert!(key.starts_with("disc:dvd:"), "{}", key);
    assert_eq!(payload["label"], "disc-mcp");

    // And the scoping argument that reaches orphaned rows is advertised.
    let tools = replies[&31]["result"]["tools"].as_array().unwrap();
    for name in ["bookmark_list", "bookmark_clear"] {
        let tool = tools.iter().find(|t| t["name"] == name).unwrap();
        assert!(
            tool["inputSchema"]["properties"]["key"].is_object(),
            "`{name}` should take a key"
        );
    }
}

/// Strip mpv's `%<len>%` length prefixes from a filter string.
///
/// mpv escapes any option value it considers ambiguous - anything with a
/// leading `-`, for one - as `%4%-6.5`. That is mpv's own quoting of a value
/// it stored correctly, not a sign the value is wrong, so tests that read
/// `af` back have to see through it.
fn mpv_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '%' {
            let mut j = i + 1;
            let mut digits = String::new();
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                digits.push(bytes[j]);
                j += 1;
            }
            if !digits.is_empty() && j < bytes.len() && bytes[j] == '%' {
                // Skip the prefix; the payload that follows is literal.
                i = j + 1;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}
