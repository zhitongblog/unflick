//! The startup timeline.
//!
//! "Cold start to picture" is the number that decides whether someone sets
//! unflick as their default player, and it is the one number nothing in the
//! codebase could report. `cargo test` cannot see it, and a stopwatch on the
//! outside cannot say *which* phase is slow — the control port only starts
//! answering near the end, so from outside every launch looks like one
//! opaque three-second block.
//!
//! So the phases mark themselves. Each `mark` writes one line to the same
//! log the rest of startup already writes to, prefixed with milliseconds
//! since the process began:
//!
//! ```text
//! [unflick] +   12ms main: gui mode
//! [unflick] +  247ms pipeline: ready
//! [unflick] +  251ms window: shown
//! ```
//!
//! `unflick startup` reads those back, so the measurement is available to
//! the CLI and to an agent, not just to whoever is watching the terminal.

use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();

/// Record t=0. Called from `main` before anything else.
pub fn start() {
    let _ = START.set(Instant::now());
}

/// Milliseconds since `start`. Zero if `start` was never called, which is
/// the case in unit tests and in the CLI paths that never boot a window.
pub fn elapsed_ms() -> u128 {
    START.get().map(|s| s.elapsed().as_millis()).unwrap_or(0)
}

/// Note that a startup phase has been reached.
///
/// Deliberately unconditional. A trace you have to enable is a trace nobody
/// has on the run that turned out to be slow, and the cost here is one
/// formatted line per phase on a path that is already writing lines.
pub fn mark(label: &str) {
    eprintln!("[unflick] +{:>5}ms {}", elapsed_ms(), label);
}

/// One phase of a recorded launch.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Phase {
    pub at_ms: u64,
    pub label: String,
}

/// Parse the marks out of a log body, newest launch only.
///
/// The log is append-only across runs and `init_file_log` writes a
/// `=== unflick <version> starting at <unix> ===` banner per run, so the
/// last banner is where the most recent launch begins. Reading the whole
/// file and keeping the tail is fine: these logs are kilobytes.
pub fn parse_last_launch(log: &str) -> Vec<Phase> {
    let tail = match log.rfind("=== unflick ") {
        Some(i) => &log[i..],
        None => log,
    };
    tail.lines().filter_map(parse_mark).collect()
}

/// `[unflick] +  247ms pipeline: ready` → `Phase { 247, "pipeline: ready" }`
fn parse_mark(line: &str) -> Option<Phase> {
    let rest = line.strip_prefix("[unflick] +")?;
    let (ms, label) = rest.split_once("ms ")?;
    Some(Phase {
        at_ms: ms.trim().parse().ok()?,
        label: label.trim().to_string(),
    })
}

/// Override for where the log lives. Tests only — same escape hatch as
/// `UNFLICK_DATA_DIR` and `UNFLICK_LEGACY_DIR`, so a test can read a
/// timeline back without depending on whatever launch happened to run last
/// on the developer's machine.
pub const LOG_PATH_ENV: &str = "UNFLICK_LOG";

/// Where `init_file_log` puts the log.
pub fn log_path() -> std::path::PathBuf {
    if let Some(p) = std::env::var_os(LOG_PATH_ENV) {
        if !p.is_empty() {
            return std::path::PathBuf::from(p);
        }
    }
    std::env::temp_dir().join("unflick.log")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mark_line_round_trips() {
        assert_eq!(
            parse_mark("[unflick] +  247ms pipeline: ready"),
            Some(Phase { at_ms: 247, label: "pipeline: ready".into() })
        );
        // Lines the rest of startup writes are not marks.
        assert_eq!(parse_mark("[unflick] bringing up video pipeline..."), None);
        assert_eq!(parse_mark("random stderr from libmpv"), None);
    }

    #[test]
    fn only_the_most_recent_launch_is_reported() {
        let log = "\
=== unflick 0.10.0 starting at 1 ===
[unflick] +    5ms main: gui mode
[unflick] + 9000ms window: shown

=== unflick 0.10.0 starting at 2 ===
[unflick] +    4ms main: gui mode
[unflick] +  250ms window: shown
";
        let phases = parse_last_launch(log);
        // The 9000ms line belongs to the previous run; reporting it would
        // make a fast launch look like a slow one.
        assert_eq!(phases.len(), 2);
        assert_eq!(phases[1], Phase { at_ms: 250, label: "window: shown".into() });
    }

    #[test]
    fn a_log_with_no_banner_still_yields_its_marks() {
        // Console launches attach to the parent terminal and never write a
        // banner. Falling back to the whole body keeps those readable.
        let phases = parse_last_launch("[unflick] +   12ms main: gui mode\n");
        assert_eq!(phases.len(), 1);
    }
}
