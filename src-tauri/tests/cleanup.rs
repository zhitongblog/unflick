//! `unflick cleanup`, against a fake stale install.
//!
//! Driving the real binary rather than calling the module, because the whole
//! feature is "a person on a machine that already upgraded runs one command":
//! the dry-run default, the JSON, and the exit code are the feature as much
//! as the deletion is.
//!
//! The rule under test is the one with teeth. On Windows the abandoned
//! install directory and the *live* thumbnail/cover caches are the same
//! path, so a cleanup that reaches for the folder instead of its contents
//! deletes cache the running player is still writing.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// A directory laid out like an unflick left behind by an upgrade: the
/// install payload, plus the caches that share the folder with it.
struct FakeStaleInstall {
    dir: PathBuf,
}

impl FakeStaleInstall {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("unflick-cleanup-test-{}", name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create fake install");

        // Install payload.
        std::fs::write(dir.join("unflick.exe"), vec![0u8; 2048]).unwrap();
        std::fs::write(dir.join("uninstall.exe"), vec![0u8; 512]).unwrap();
        std::fs::create_dir_all(dir.join("ffmpeg")).unwrap();
        std::fs::write(dir.join("ffmpeg").join("ffmpeg.exe"), vec![0u8; 4096]).unwrap();
        // Junk an old run left at the top level.
        std::fs::write(dir.join("temp_audio.srt"), b"1\n").unwrap();

        // Live caches, which must survive.
        std::fs::create_dir_all(dir.join("thumbs").join("abcd")).unwrap();
        std::fs::write(dir.join("thumbs").join("abcd").join("160-3.jpg"), b"jpeg").unwrap();
        std::fs::create_dir_all(dir.join("covers")).unwrap();
        std::fs::write(dir.join("covers").join("deadbeef.jpg"), b"jpeg").unwrap();

        Self { dir }
    }

    fn run(&self, args: &[&str]) -> Value {
        let out = Command::new(env!("CARGO_BIN_EXE_unflick"))
            .args(args)
            .env(unflick_lib::core::cleanup::LEGACY_DIR_ENV, &self.dir)
            .output()
            .expect("run unflick");
        // Failures go to stderr, successes to stdout — the CLI's own
        // convention, so a shell pipeline only ever gets the good case.
        let body = if out.stdout.is_empty() { &out.stderr } else { &out.stdout };
        serde_json::from_slice(body).unwrap_or_else(|e| {
            panic!(
                "unflick {:?} did not print JSON: {}\nstdout: {}\nstderr: {}",
                args,
                e,
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
        })
    }

    fn has(&self, rel: &str) -> bool {
        self.dir.join(rel).exists()
    }
}

impl Drop for FakeStaleInstall {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn paths(report: &Value) -> Vec<String> {
    report["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|i| {
            Path::new(i["path"].as_str().unwrap())
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

#[test]
fn scanning_reports_without_removing_anything() {
    let fake = FakeStaleInstall::new("scan");

    let reply = fake.run(&["cleanup"]);
    assert_eq!(reply["success"], true);
    let report = &reply["data"];

    let mut found = paths(report);
    found.sort();
    assert_eq!(found, vec!["ffmpeg", "temp_audio.srt", "unflick.exe", "uninstall.exe"]);
    assert_eq!(report["total_bytes"], 2048 + 512 + 4096 + 2);
    assert_eq!(report["removed"], false);

    // The default has to be harmless: this is the command someone runs to
    // find out whether it is worth running.
    assert!(fake.has("unflick.exe"));
    assert!(fake.has("ffmpeg/ffmpeg.exe"));
    assert!(
        reply["message"].as_str().unwrap().contains("--apply"),
        "the report should say how to act on it, got {}",
        reply["message"]
    );
}

#[test]
fn the_live_caches_are_never_listed_for_removal() {
    let fake = FakeStaleInstall::new("caches-listed");
    let report = fake.run(&["cleanup"])["data"].clone();

    let names = paths(&report);
    assert!(!names.contains(&"thumbs".to_string()), "{:?}", names);
    assert!(!names.contains(&"covers".to_string()), "{:?}", names);

    let kept = report["kept"].as_array().expect("kept");
    assert_eq!(kept.len(), 2, "both caches should be reported as kept");
}

#[test]
fn applying_removes_the_payload_and_leaves_the_caches() {
    let fake = FakeStaleInstall::new("apply");

    let reply = fake.run(&["cleanup", "--apply"]);
    assert_eq!(reply["success"], true, "{}", reply["message"]);
    assert_eq!(reply["data"]["removed"], true);

    assert!(!fake.has("unflick.exe"));
    assert!(!fake.has("uninstall.exe"));
    assert!(!fake.has("ffmpeg"));
    assert!(!fake.has("temp_audio.srt"));

    // The whole point.
    assert!(fake.has("thumbs/abcd/160-3.jpg"), "thumbnail cache was deleted");
    assert!(fake.has("covers/deadbeef.jpg"), "cover cache was deleted");
    assert!(fake.dir.exists(), "the folder still holds live caches");
}

/// A folder with caches and no install in it belongs to the running version.
#[test]
fn a_cache_only_directory_is_left_alone() {
    let fake = FakeStaleInstall::new("cache-only");
    std::fs::remove_file(fake.dir.join("unflick.exe")).unwrap();
    std::fs::remove_file(fake.dir.join("uninstall.exe")).unwrap();

    let reply = fake.run(&["cleanup"]);
    assert_eq!(reply["success"], true);
    assert!(
        reply["data"]["directory"].is_null(),
        "without an install marker there is nothing to claim, got {}",
        reply["data"]["directory"]
    );
    assert!(fake.has("thumbs/abcd/160-3.jpg"));
}

#[test]
fn applying_with_nothing_to_clean_is_an_error_not_a_silent_success() {
    let fake = FakeStaleInstall::new("empty");
    std::fs::remove_file(fake.dir.join("unflick.exe")).unwrap();
    std::fs::remove_file(fake.dir.join("uninstall.exe")).unwrap();

    let reply = fake.run(&["cleanup", "--apply"]);
    assert_eq!(reply["success"], false);
    assert!(
        reply["message"].as_str().unwrap().contains("nothing to clean"),
        "got {}",
        reply["message"]
    );
}
