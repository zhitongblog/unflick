//! DVD and Blu-ray sources, through the real binary.
//!
//! What can be tested here without a disc in the machine is the part that
//! is ours: recognising one, and deciding how mpv should be told to open
//! it. A `.iso` handed to mpv as a plain path gets demuxed as if it were a
//! container and fails; the fix is a protocol and a device, and choosing
//! those correctly is a decision that can be checked.
//!
//! What is *not* covered here is playback off a real disc. That needs a
//! disc. `disc` reporting what it would do with a path exists partly so
//! that the untestable half is as small as possible: everything up to the
//! moment libdvdnav takes over is observable from outside.

mod common;

use common::Daemon;
use serde_json::json;

/// An ISO9660 image with one directory in its root — enough structure for
/// the probe to read, laid out by hand so the test does not depend on
/// having a disc-authoring tool.
fn iso_with_root_entry(name: &str) -> Vec<u8> {
    const S: usize = 2048;
    let mut img = vec![0u8; 20 * S];

    // Primary volume descriptor at sector 16, root directory at 18.
    let pvd = 16 * S;
    img[pvd] = 1;
    img[pvd + 1..pvd + 6].copy_from_slice(b"CD001");
    img[pvd + 156] = 34;
    img[pvd + 158..pvd + 162].copy_from_slice(&18u32.to_le_bytes());
    img[pvd + 166..pvd + 170].copy_from_slice(&(S as u32).to_le_bytes());

    // Terminator at 17.
    img[17 * S] = 255;
    img[17 * S + 1..17 * S + 6].copy_from_slice(b"CD001");

    // Root directory: "." then the entry under test.
    let root = 18 * S;
    img[root] = 34;
    img[root + 32] = 1;
    let e = root + 34;
    img[e] = (33 + name.len()) as u8;
    img[e + 25] = 0x02;
    img[e + 32] = name.len() as u8;
    img[e + 33..e + 33 + name.len()].copy_from_slice(name.as_bytes());

    img
}

struct TempImage(std::path::PathBuf);

impl TempImage {
    fn new(name: &str, marker: &str) -> Self {
        let p = std::env::temp_dir().join(name);
        std::fs::write(&p, iso_with_root_entry(marker)).expect("write image");
        Self(p)
    }
    fn path(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for TempImage {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn a_dvd_image_is_reported_as_a_dvd_and_opened_as_one() {
    let img = TempImage::new("unflick-it-dvd.iso", "VIDEO_TS");
    let d = Daemon::start();

    let reply = d.send("disc_list", json!({ "path": img.path() }));
    reply.expect_ok();
    let data = reply.data();
    assert_eq!(data["kind"], "dvd");
    // The two things that make the difference between playing and failing.
    assert_eq!(data["url"], "dvd://");
    assert_eq!(data["device"], img.path());
}

#[test]
fn a_bluray_image_is_reported_as_a_bluray() {
    let img = TempImage::new("unflick-it-bd.iso", "BDMV");
    let d = Daemon::start();

    let reply = d.send("disc_list", json!({ "path": img.path() }));
    reply.expect_ok();
    assert_eq!(reply.data()["kind"], "bluray");
    assert_eq!(reply.data()["url"], "bd://");
}

#[test]
fn an_iso_of_something_else_stays_an_ordinary_file() {
    // The rule that keeps this feature from breaking every other one: an
    // .iso of someone's backups must not become an unplayable "DVD".
    let img = TempImage::new("unflick-it-data.iso", "BACKUPS");
    let d = Daemon::start();

    let reply = d.send("disc_list", json!({ "path": img.path() }));
    reply.expect_ok();
    assert!(
        reply.data().is_null(),
        "a data image should not be claimed as a disc, got {}",
        reply.data()
    );
    assert!(reply.message().contains("not a video disc"), "{}", reply.message());
}

#[test]
fn an_ordinary_video_file_is_not_a_disc() {
    let f = common::fixtures();
    let d = Daemon::start();
    let reply = d.send("disc_list", json!({ "path": f.plain.to_string_lossy() }));
    reply.expect_ok();
    assert!(reply.data().is_null());
}

#[test]
fn a_folder_of_video_ts_is_a_dvd() {
    let dir = std::env::temp_dir().join("unflick-it-disc-folder");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("VIDEO_TS")).unwrap();

    let d = Daemon::start();
    let reply = d.send("disc_list", json!({ "path": dir.to_string_lossy() }));
    reply.expect_ok();
    assert_eq!(reply.data()["kind"], "dvd");
    assert_eq!(reply.data()["device"], dir.to_string_lossy().into_owned());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_explicit_disc_url_is_passed_through_untouched() {
    let d = Daemon::start();

    // Someone naming a title means that title, not the default one.
    let reply = d.send("disc_list", json!({ "path": "dvd://3" }));
    reply.expect_ok();
    assert_eq!(reply.data()["url"], "dvd://3");

    // And the menus are reachable by asking for them.
    let reply = d.send("disc_list", json!({ "path": "dvdnav://" }));
    reply.expect_ok();
    assert_eq!(reply.data()["url"], "dvdnav://");
    assert_eq!(reply.data()["kind"], "dvd");
}

#[test]
fn listing_says_whether_this_build_can_play_discs_at_all() {
    // The v0.12 lesson about smb:// — a build without the right libraries
    // should say so, not hand back a drive list that errors on every entry.
    let d = Daemon::start();
    let reply = d.send("disc_list", json!({}));
    reply.expect_ok();

    let supports = &reply.data()["supports"];
    assert!(supports["dvd"].is_boolean(), "got {}", supports);
    assert!(supports["bluray"].is_boolean(), "got {}", supports);
    assert!(reply.data()["drives"].is_array());

    // This is asserted rather than merely reported because it is a property
    // of what we ship: the bundled libmpv lists dvd, dvdnav, bd and bluray
    // among its protocols, and a build that lost them would take the
    // feature with it silently.
    assert_eq!(supports["dvd"], true, "the bundled libmpv should play DVDs");
    assert_eq!(supports["bluray"], true, "the bundled libmpv should play Blu-rays");
}

// ─── What a disc cannot do ────────────────────────────────────────────────
//
// Both of these go through ffmpeg, which reads files. Handed a disc it says
// "No such file or directory" — true, and useless, because the user did not
// mistype a path. Found by pointing the player at a mounted DVD and trying
// everything on it.

/// A folder that looks like a mounted disc, for the paths that must refuse.
fn video_ts_folder(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("VIDEO_TS")).unwrap();
    dir
}

#[test]
fn a_clip_cannot_be_cut_from_a_disc_and_says_why() {
    let dir = video_ts_folder("unflick-it-clip-disc");
    let d = Daemon::start();

    let reply = d.send(
        "clip",
        json!({
            "file": dir.to_string_lossy(),
            "start": 1.0,
            "end": 3.0,
            "output": std::env::temp_dir().join("unflick-it-clip.mp4").to_string_lossy(),
        }),
    );
    reply.expect_err_containing("disc");
    assert!(
        !reply.message().contains("ffmpeg version"),
        "the raw ffmpeg banner is not an explanation: {}",
        reply.message()
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn thumbnail_previews_are_refused_for_a_disc() {
    // Refused once, rather than spawning an ffmpeg that cannot succeed
    // every time the pointer crosses the progress bar.
    let dir = video_ts_folder("unflick-it-thumb-disc");
    let err = match unflick_lib::core::thumbnail::thumbnail_at(&dir.to_string_lossy(), 5.0, 60.0, 160) {
        Err(e) => e,
        Ok(_) => panic!("a disc has no thumbnails to give"),
    };
    assert!(err.to_string().contains("disc"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}
