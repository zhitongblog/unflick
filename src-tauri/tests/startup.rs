//! `unflick startup`, against a log written by hand.
//!
//! The thing worth locking down is not the parsing — that has unit tests —
//! but that the command answers at all without a player. It exists for the
//! machine where startup is the problem, so booting a daemon to report on
//! startup would be both circular and, on a broken install, impossible.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

/// A log with two launches in it. Only the second one is this launch.
const LOG: &str = "\
=== unflick 0.10.0 starting at 1000 ===
[unflick] +    0ms main: gui mode
[unflick] + 4000ms window: shown

=== unflick 0.10.0 starting at 2000 ===
[unflick] +    0ms main: opening a file from the shell
[unflick] +  684ms setup: entered
[unflick] +  926ms pipeline: ready
[unflick-render] render context created
[unflick] + 1149ms open: launch file playing
";

struct FakeLog {
    path: PathBuf,
}

impl FakeLog {
    fn new(name: &str, body: &str) -> Self {
        let path = std::env::temp_dir().join(format!("unflick-startup-test-{}.log", name));
        std::fs::write(&path, body).expect("write fake log");
        Self { path }
    }

    fn run(&self) -> Value {
        let out = Command::new(env!("CARGO_BIN_EXE_unflick"))
            .arg("startup")
            .env(unflick_lib::core::boot::LOG_PATH_ENV, &self.path)
            .output()
            .expect("run unflick");
        let body = if out.stdout.is_empty() { &out.stderr } else { &out.stdout };
        serde_json::from_slice(body).unwrap_or_else(|e| {
            panic!(
                "unflick startup did not print JSON: {}\nstdout: {}\nstderr: {}",
                e,
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
        })
    }
}

impl Drop for FakeLog {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn it_reports_the_phases_of_the_most_recent_launch() {
    let fake = FakeLog::new("phases", LOG);
    let reply = fake.run();
    assert_eq!(reply["success"], true, "{}", reply["message"]);

    let phases = reply["data"]["phases"].as_array().expect("phases");
    let labels: Vec<&str> = phases.iter().map(|p| p["label"].as_str().unwrap()).collect();
    assert_eq!(
        labels,
        vec![
            "main: opening a file from the shell",
            "setup: entered",
            "pipeline: ready",
            "open: launch file playing",
        ],
        "the earlier launch's phases must not be mixed in, and non-mark \
         lines like the render thread's must not become phases"
    );

    // The headline number is when the picture arrived, not when the process
    // started, so it has to come from the last phase.
    assert_eq!(reply["data"]["total_ms"], 1149);
    assert!(
        reply["message"].as_str().unwrap().contains("1149"),
        "the one-line message should carry the number, got {}",
        reply["message"]
    );
}

#[test]
fn a_missing_log_says_so_rather_than_reporting_a_launch_of_zero() {
    let fake = FakeLog::new("missing", "");
    std::fs::remove_file(&fake.path).unwrap();

    let reply = fake.run();
    assert_eq!(reply["success"], false);
    assert!(
        reply["message"]
            .as_str()
            .unwrap()
            .contains("unflick-startup-test-missing"),
        "the message should name the log it looked for, got {}",
        reply["message"]
    );
}

#[test]
fn a_log_with_no_marks_is_a_success_with_nothing_in_it() {
    // An older build, or a launch that died before its first mark. Reading
    // it is not an error — there is simply nothing to report.
    let fake = FakeLog::new("unmarked", "=== unflick 0.9.0 starting at 1 ===\nsome noise\n");
    let reply = fake.run();
    assert_eq!(reply["success"], true);
    assert_eq!(reply["data"]["total_ms"], 0);
    assert_eq!(reply["data"]["phases"].as_array().unwrap().len(), 0);
}
