//! The dev surface: driving and reading the window from outside the window.
//!
//! Six bugs shipped in the 2026-08-27 batch compiled cleanly and passed the
//! whole headless suite — a list that never reached an open panel, an audio
//! menu that showed "undefined" for every track. None of them were reachable
//! from `core/`: they lived between the state and the pixels. The one method
//! that ever caught them (PrintWindow + PostMessage) existed only on Windows,
//! which is why macOS and Linux have never been verified feature by feature.
//!
//! This is the headless half of closing that gap. It rides the same seam
//! `core::window` uses: a trait here, `Some(...)` in the GUI, `None` in the
//! headless daemon, and a refusal that names the missing thing rather than
//! reporting a click nothing performed.
//!
//! # Only one primitive is platform-specific
//!
//! `eval` is the primitive. `click`, `text`, `snapshot` and `wait` are
//! JavaScript programs written once here and run through it, so they are
//! identical on Windows, macOS and Linux *by construction* rather than by
//! three parallel implementations that drift. A Linux-only snapshot bug
//! becomes impossible because there is no Linux-only snapshot code. Only
//! `capture` needs per-platform work, and it is the one verb that reads
//! pixels rather than the DOM.
//!
//! # What this module does not do
//!
//! It builds the program and shapes the answer. It does not run anything:
//! getting a value back out of a webview is the window's business and lives
//! in `gui::dev`, behind [`DevHost`]. That split is why every refusal in
//! here — the gate, the missing window, a mistyped verb, an out-of-range
//! timeout — is testable against a headless daemon, which is the only host
//! CI can create.

use std::time::Duration;

use serde_json::{json, Value};

use super::types::CommandResult;

/// Refusal when the dev surface was never armed.
///
/// Says how to arm it, because "not permitted" without "here is the flag"
/// is a dead end for the only person who ever sees this message.
pub const GATE_MESSAGE: &str =
    "dev commands are off — start unflick with --allow-dev to drive the window from outside";

/// Refusal when the surface is armed but nothing is on screen.
///
/// Deliberately the same shape as `window_mode`'s "no window — window modes
/// need the unflick GUI running": one wording for "the GUI is the thing that
/// can do this and it is not running".
pub const NO_WINDOW_MESSAGE: &str = "no window — dev commands need the unflick GUI running";

/// The verbs, in the order they are worth reaching for.
pub const VERBS: [&str; 6] = ["eval", "click", "text", "snapshot", "capture", "wait"];

/// Entry points the probe must define on `globalThis.__unflickDev`.
///
/// Named here rather than only spelled out at each call site so the probe
/// and its callers can be checked against one list — a rename on one side
/// only is exactly the kind of break that would otherwise surface as an
/// opaque "undefined is not a function" from inside a webview.
pub const PROBE_ENTRY_POINTS: [&str; 4] = ["click", "text", "snapshot", "waitFor"];

/// Shortest timeout worth honouring, in seconds. Below this the round trip
/// itself is the whole budget and every call would fail.
const MIN_TIMEOUT: f64 = 0.1;
/// Longest. A mistyped `--timeout 6000` would otherwise wedge a terminal on
/// a blocking socket read for an hour and a half.
const MAX_TIMEOUT: f64 = 120.0;

const DEFAULT_EVAL_TIMEOUT: f64 = 5.0;
const DEFAULT_CLICK_TIMEOUT: f64 = 5.0;
const DEFAULT_WAIT_TIMEOUT: f64 = 10.0;

/// How long a read (`text`, `snapshot`) may take. Not exposed as a flag:
/// neither polls for anything, so a slow answer means the webview is wedged
/// and waiting longer will not fix it.
const READ_TIMEOUT: f64 = 5.0;

/// How long to wait for a window snapshot.
///
/// Longer than a read because it is not one: the platform APIs wait for a
/// screen update, and a window that has just been restored can take a beat
/// to produce one.
const CAPTURE_TIMEOUT: f64 = 10.0;

/// Extra time the Rust side allows on top of a timeout the JavaScript is
/// itself counting down.
///
/// `click` and `wait` poll inside the webview. If the socket gave up at
/// exactly the same instant, every genuine timeout would come back as the
/// transport's opaque failure instead of the probe's own message naming the
/// selector — the useful half of the answer lost to a race with itself.
const POLL_SLACK: f64 = 2.0;

const DEFAULT_DEPTH: u32 = 12;
/// A tree deeper than this is a DOM dump, not a snapshot. React apps nest,
/// but not sixty-four levels of *named* nodes.
const MAX_DEPTH: u32 = 64;

/// Longest edge of the inline capture, matching `core::vision`.
const DEFAULT_MAX_EDGE: u32 = 768;
const MIN_MAX_EDGE: u32 = 64;
const MAX_MAX_EDGE: u32 = 2048;

/// A picture of the interface layer.
///
/// PNG because that is what all three platforms' webview snapshot APIs
/// produce, and re-encoding on the way out of the window would throw away
/// the exact pixels this exists to inspect.
pub struct Shot {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// The real window, when one exists — and when someone armed it.
///
/// Implemented by the GUI over its webview. The headless daemon leaves this
/// `None`: there is nothing to click, and answering "ok" to a click nobody
/// performed is the failure this codebase has already been burned by once,
/// when `loadfile` reported success for a file that never opened.
pub trait DevHost: Send + Sync {
    /// Run `js` in the window and return what it evaluated to.
    ///
    /// The implementation prepends the probe source, so `js` may call
    /// `__unflickDev` freely. Splitting it there rather than here keeps
    /// every verb's *meaning* in `core/` — one program per verb, identical
    /// on all three platforms — while the one genuinely window-shaped
    /// problem, getting a value back out of a fire-and-forget eval, stays
    /// in `gui/`.
    fn eval(&self, js: &str, timeout: Duration) -> Result<Value, String>;

    /// A PNG of the interface layer.
    fn capture(&self, timeout: Duration) -> Result<Shot, String>;
}

/// One dev command, with every argument already validated and clamped.
///
/// Parsed before the window is looked for, the same order `window_mode`
/// uses: a typo is always reported as a typo, because "no window" would
/// send someone looking at the wrong problem entirely.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    Eval {
        script: String,
        timeout: Duration,
    },
    Click {
        selector: String,
        index: Option<u32>,
        timeout: Duration,
    },
    Text {
        selector: String,
    },
    Snapshot {
        selector: Option<String>,
        depth: u32,
    },
    Capture {
        max_edge: u32,
        /// Where to write the full-resolution PNG. `None` returns a
        /// downscaled JPEG inline. Same split as `describe_frame`.
        output: Option<String>,
    },
    Wait {
        selector: String,
        gone: bool,
        timeout: Duration,
    },
}

impl Request {
    /// Parse a `dev_*` control command. `cmd` carries the prefix, as it
    /// arrives on the wire.
    pub fn parse(cmd: &str, args: &Value) -> Result<Self, String> {
        let verb = cmd.strip_prefix("dev_").unwrap_or(cmd);
        match verb {
            "eval" => Ok(Request::Eval {
                script: required_str(args, "script")?,
                timeout: timeout_from(args, DEFAULT_EVAL_TIMEOUT),
            }),
            "click" => Ok(Request::Click {
                selector: required_str(args, "selector")?,
                index: args.get("index").and_then(|v| v.as_u64()).map(|v| v as u32),
                timeout: timeout_from(args, DEFAULT_CLICK_TIMEOUT),
            }),
            "text" => Ok(Request::Text {
                selector: required_str(args, "selector")?,
            }),
            "snapshot" => Ok(Request::Snapshot {
                selector: args
                    .get("selector")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .filter(|s| !s.trim().is_empty()),
                depth: args
                    .get("depth")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                    .unwrap_or(DEFAULT_DEPTH)
                    .clamp(1, MAX_DEPTH),
            }),
            "capture" => Ok(Request::Capture {
                max_edge: args
                    .get("max_edge")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                    .unwrap_or(DEFAULT_MAX_EDGE)
                    .clamp(MIN_MAX_EDGE, MAX_MAX_EDGE),
                output: args
                    .get("output")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .filter(|s| !s.trim().is_empty()),
            }),
            "wait" => Ok(Request::Wait {
                selector: required_str(args, "selector")?,
                gone: args.get("gone").and_then(|v| v.as_bool()).unwrap_or(false),
                timeout: timeout_from(args, DEFAULT_WAIT_TIMEOUT),
            }),
            other => Err(format!(
                "unknown dev command {:?} — expected one of: {}",
                other,
                VERBS.join(", ")
            )),
        }
    }
}

fn required_str(args: &Value, key: &str) -> Result<String, String> {
    match args.get(key).and_then(|v| v.as_str()) {
        Some(s) if !s.trim().is_empty() => Ok(s.to_string()),
        // Named rather than "missing argument": the caller is a person at a
        // shell or a model reading a schema, and both act on the name.
        _ => Err(format!("{} is required", key)),
    }
}

fn timeout_from(args: &Value, default: f64) -> Duration {
    let seconds = args
        .get("timeout_seconds")
        .and_then(|v| v.as_f64())
        .filter(|v| v.is_finite())
        .unwrap_or(default)
        .clamp(MIN_TIMEOUT, MAX_TIMEOUT);
    Duration::from_secs_f64(seconds)
}

/// Run a parsed request against a window.
pub fn run(host: &dyn DevHost, request: Request) -> CommandResult {
    match request {
        Request::Eval { script, timeout } => match host.eval(&script, timeout) {
            Ok(value) => CommandResult::ok_with_data(
                describe_value(&value),
                json!({ "value": value }),
            ),
            Err(e) => CommandResult::err(e),
        },
        Request::Click {
            selector,
            index,
            timeout,
        } => {
            let call = probe_call(
                "click",
                json!({
                    "selector": selector,
                    "index": index,
                    "timeoutMs": timeout.as_millis() as u64,
                }),
            );
            probe_result(host.eval(&call, timeout + slack()), |data| {
                data.get("clicked")
                    .and_then(|v| v.as_str())
                    .map(|s| format!("clicked {}", s))
                    .unwrap_or_else(|| format!("clicked {}", selector))
            })
        }
        Request::Text { selector } => {
            let call = probe_call("text", json!({ "selector": selector }));
            probe_result(host.eval(&call, read_timeout()), |data| {
                let n = data
                    .get("matches")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                format!("{} match(es) for {}", n, selector)
            })
        }
        Request::Snapshot { selector, depth } => {
            let call = probe_call(
                "snapshot",
                json!({ "selector": selector, "depth": depth }),
            );
            probe_result(host.eval(&call, read_timeout()), |data| {
                let n = data
                    .get("nodes")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                format!("{} node(s)", n)
            })
        }
        Request::Wait {
            selector,
            gone,
            timeout,
        } => {
            let call = probe_call(
                "waitFor",
                json!({
                    "selector": selector,
                    "gone": gone,
                    "timeoutMs": timeout.as_millis() as u64,
                }),
            );
            probe_result(host.eval(&call, timeout + slack()), |data| {
                let ms = data.get("waited_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                if gone {
                    format!("{} left the page after {} ms", selector, ms)
                } else {
                    format!("{} appeared after {} ms", selector, ms)
                }
            })
        }
        Request::Capture { max_edge, output } => capture(host, max_edge, output),
    }
}

fn capture(host: &dyn DevHost, max_edge: u32, output: Option<String>) -> CommandResult {
    let shot = match host.capture(Duration::from_secs_f64(CAPTURE_TIMEOUT)) {
        Ok(s) => s,
        Err(e) => return CommandResult::err(e),
    };

    // Same split as `describe_frame` and `thumbnail`: a path for the CLI,
    // base64 for callers that want the bytes inline. Printing a megabyte of
    // base64 into a terminal helps nobody, and writing a file into an
    // agent's working directory helps it even less.
    if let Some(path) = output {
        if let Some(parent) = std::path::Path::new(&path)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
        {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return CommandResult::err(format!("failed to create {}: {}", parent.display(), e));
            }
        }
        if let Err(e) = std::fs::write(&path, &shot.png) {
            return CommandResult::err(format!("failed to write {}: {}", path, e));
        }
        return CommandResult::ok_with_data(
            format!("{}×{} interface capture → {}", shot.width, shot.height, path),
            json!({
                "width": shot.width,
                "height": shot.height,
                "bytes": shot.png.len(),
                "mime_type": "image/png",
                "path": path,
            }),
        );
    }

    // Downscaled through the one ffmpeg path the frame captures already use,
    // rather than linking an image crate for a second way to do the same
    // thing.
    let jpeg = match super::vision::shrink_bytes_to_jpeg(&shot.png, "png", max_edge) {
        Ok(b) => b,
        Err(e) => return CommandResult::err(e.to_string()),
    };
    CommandResult::ok_with_data(
        format!("{}×{} interface capture ({} bytes)", shot.width, shot.height, jpeg.len()),
        json!({
            "width": shot.width,
            "height": shot.height,
            "bytes": jpeg.len(),
            "mime_type": "image/jpeg",
            "base64": super::vision::base64_encode(&jpeg),
        }),
    )
}

/// The JavaScript for one probe entry point.
///
/// Arguments go in as a JSON literal rather than string concatenation, so a
/// selector containing a quote, a backslash or a newline is the probe's
/// problem to match and never the program's problem to parse.
fn probe_call(entry: &str, args: Value) -> String {
    debug_assert!(
        PROBE_ENTRY_POINTS.contains(&entry),
        "{} is not one of the probe's entry points",
        entry
    );
    format!("return __unflickDev.{}({});", entry, args)
}

fn slack() -> Duration {
    Duration::from_secs_f64(POLL_SLACK)
}

fn read_timeout() -> Duration {
    Duration::from_secs_f64(READ_TIMEOUT)
}

/// Turn a probe answer into a command result.
///
/// The probe reports its own failures as `{ok: false, error: "..."}` rather
/// than throwing, because a thrown error crossing the webview boundary
/// arrives as a stack trace with the useful sentence buried in it. An `ok:
/// false` is a refusal the probe wrote deliberately — "nothing matches
/// `.play-button`", "a dialog is covering it" — and is passed through
/// unchanged.
fn probe_result(
    outcome: Result<Value, String>,
    summarise: impl FnOnce(&Value) -> String,
) -> CommandResult {
    let value = match outcome {
        Ok(v) => v,
        Err(e) => return CommandResult::err(e),
    };
    if value.get("ok").and_then(|v| v.as_bool()) == Some(false) {
        let message = value
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("the page refused the command without saying why");
        return CommandResult::err(message.to_string());
    }
    CommandResult::ok_with_data(summarise(&value), value)
}

/// A one-line summary of an evaluated value, for the message.
///
/// The full value is in `data` either way; this is what a person reads
/// first, so it says what came back rather than repeating it at length.
fn describe_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) if s.chars().count() <= 120 => s.clone(),
        Value::String(s) => format!("{}…", s.chars().take(120).collect::<String>()),
        Value::Array(a) => format!("{} item(s)", a.len()),
        Value::Object(o) => format!("{} field(s)", o.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(cmd: &str, args: Value) -> Request {
        Request::parse(cmd, &args).unwrap()
    }

    #[test]
    fn an_unknown_verb_names_the_ones_that_exist() {
        let err = Request::parse("dev_frobnicate", &json!({})).unwrap_err();
        for verb in VERBS {
            assert!(err.contains(verb), "{:?} does not mention {}", err, verb);
        }
    }

    #[test]
    fn the_required_argument_is_named_when_it_is_missing() {
        assert_eq!(
            Request::parse("dev_eval", &json!({})).unwrap_err(),
            "script is required"
        );
        assert_eq!(
            Request::parse("dev_click", &json!({})).unwrap_err(),
            "selector is required"
        );
        assert_eq!(
            Request::parse("dev_wait", &json!({ "selector": "   " })).unwrap_err(),
            "selector is required"
        );
    }

    #[test]
    fn timeouts_are_clamped_so_a_typo_cannot_wedge_a_terminal() {
        // A mistyped `--timeout 6000` is an hour and a half on a blocking
        // socket read; a mistyped `0` is a call that can never succeed.
        let Request::Eval { timeout, .. } =
            parse("dev_eval", json!({ "script": "1", "timeout_seconds": 6000 }))
        else {
            panic!("wrong variant");
        };
        assert_eq!(timeout, Duration::from_secs_f64(MAX_TIMEOUT));

        let Request::Wait { timeout, .. } =
            parse("dev_wait", json!({ "selector": "body", "timeout_seconds": 0 }))
        else {
            panic!("wrong variant");
        };
        assert_eq!(timeout, Duration::from_secs_f64(MIN_TIMEOUT));

        // NaN reaches `clamp`, which panics on it. Filtered out before.
        let Request::Click { timeout, .. } = parse(
            "dev_click",
            json!({ "selector": "b", "timeout_seconds": f64::NAN }),
        ) else {
            panic!("wrong variant");
        };
        assert_eq!(timeout, Duration::from_secs_f64(DEFAULT_CLICK_TIMEOUT));
    }

    #[test]
    fn the_defaults_are_the_ones_the_help_text_promises() {
        assert_eq!(
            parse("dev_eval", json!({ "script": "1" })),
            Request::Eval {
                script: "1".into(),
                timeout: Duration::from_secs_f64(5.0)
            }
        );
        assert_eq!(
            parse("dev_wait", json!({ "selector": "body" })),
            Request::Wait {
                selector: "body".into(),
                gone: false,
                timeout: Duration::from_secs_f64(10.0)
            }
        );
        assert_eq!(
            parse("dev_snapshot", json!({})),
            Request::Snapshot {
                selector: None,
                depth: DEFAULT_DEPTH
            }
        );
        assert_eq!(
            parse("dev_capture", json!({})),
            Request::Capture {
                max_edge: DEFAULT_MAX_EDGE,
                output: None
            }
        );
    }

    #[test]
    fn capture_max_edge_is_clamped_the_way_core_vision_clamps_it() {
        let Request::Capture { max_edge, .. } = parse("dev_capture", json!({ "max_edge": 1 }))
        else {
            panic!("wrong variant");
        };
        assert_eq!(max_edge, MIN_MAX_EDGE);

        let Request::Capture { max_edge, .. } = parse("dev_capture", json!({ "max_edge": 99999 }))
        else {
            panic!("wrong variant");
        };
        assert_eq!(max_edge, MAX_MAX_EDGE);
    }

    #[test]
    fn snapshot_depth_is_clamped_to_something_that_is_still_a_snapshot() {
        let Request::Snapshot { depth, .. } = parse("dev_snapshot", json!({ "depth": 0 })) else {
            panic!("wrong variant");
        };
        assert_eq!(depth, 1);

        let Request::Snapshot { depth, .. } = parse("dev_snapshot", json!({ "depth": 4096 })) else {
            panic!("wrong variant");
        };
        assert_eq!(depth, MAX_DEPTH);
    }

    #[test]
    fn a_selector_with_quotes_in_it_survives_into_the_program() {
        // The reason arguments are a JSON literal and not concatenation:
        // `[data-testid="track's row"]` is a perfectly ordinary selector.
        let call = probe_call(
            "click",
            json!({ "selector": "[data-testid=\"track's \\ row\"]" }),
        );
        let start = call.find('{').unwrap();
        let end = call.rfind('}').unwrap();
        let parsed: Value = serde_json::from_str(&call[start..=end]).unwrap();
        assert_eq!(parsed["selector"], "[data-testid=\"track's \\ row\"]");
    }

    #[test]
    fn the_gate_message_does_not_leak_whether_a_window_exists() {
        // A locked-down instance must not answer "no window" — that is an
        // answer about the machine, given to a caller who was refused.
        assert!(!GATE_MESSAGE.contains("no window"));
        assert!(GATE_MESSAGE.contains("--allow-dev"));
    }

    #[test]
    fn a_probe_refusal_is_passed_through_as_the_failure_it_is() {
        let refused = probe_result(
            Ok(json!({ "ok": false, "error": "nothing matches .play-button" })),
            |_| "unreachable".to_string(),
        );
        assert!(!refused.success);
        assert_eq!(refused.message, "nothing matches .play-button");
    }
}
