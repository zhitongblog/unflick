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

/// The probe itself.
///
/// Prepended to every program a [`DevHost`] evaluates, so `click`, `text`,
/// `snapshot` and `wait` are one file that runs unchanged on WKWebView,
/// WebView2 and WebKitGTK. It lives here rather than in `gui/` because what
/// those four verbs *mean* is core's business; only running it is the
/// window's.
///
/// It defines `globalThis.__unflickDev` idempotently, so re-sending it on
/// every call costs a parse and nothing else.
pub const PROBE_JS: &str = include_str!("dev_probe.js");

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

/// How long to wait between looks, for the two verbs that wait.
///
/// The polling is here and not in the page, and that is measured rather
/// than stylistic. The obvious design is one eval carrying a `setTimeout`
/// loop and one answer at the end of it; it does not work. WebKit stops a
/// hidden page's timers a few seconds after it is hidden — on macOS 26,
/// ticks at 50 ms for about three seconds and then nothing, ever — and a
/// window that is covered, minimised, on another Space or behind a locked
/// screen is hidden. That is exactly the unattended case this surface
/// exists for, so `click` and `wait` would hang precisely when they were
/// needed. Chromium and WebKitGTK throttle background pages too; polling
/// from out here is immune to all of it by construction, the same way one
/// probe file makes a platform-specific snapshot bug impossible.
///
/// Every look is also a fresh eval, which is not incidental: it is the
/// only thing that runs script in a page nothing else is waking.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

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
        } => poll(
            host,
            "click",
            json!({ "selector": selector, "index": index }),
            timeout,
            |data, _| {
                data.get("clicked")
                    .and_then(|v| v.as_str())
                    .map(|s| format!("clicked {}", s))
                    .unwrap_or_else(|| "clicked".to_string())
            },
            |why, waited| format!("gave up after {} ms — {}", waited, why),
        ),
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
            let name = selector.clone();
            poll(
                host,
                "waitFor",
                json!({ "selector": selector, "gone": gone }),
                timeout,
                move |_, waited| {
                    if gone {
                        format!("{} left the page after {} ms", name, waited)
                    } else {
                        format!("{} appeared after {} ms", name, waited)
                    }
                },
                |why, waited| format!("gave up after {} ms — {}", waited, why),
            )
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

fn read_timeout() -> Duration {
    Duration::from_secs_f64(READ_TIMEOUT)
}

/// Run a probe entry point over and over until it says it is done.
///
/// Three kinds of answer, and the difference between them is the whole
/// point of polling from out here:
///
///   * `{ok: false, error}` — the caller's mistake. A string that is not a
///     selector, a selector that matches several things. Refused at once,
///     because looking again cannot change it.
///   * `{ok: true, done: false, why}` — not yet. A panel still opening, an
///     element still covered by the dialog that is fading out. Looked at
///     again, and `why` becomes the message if the deadline arrives first.
///   * `{ok: true, done: true, …}` — it happened.
///
/// A failed *eval* is also "not yet", and that is deliberate: an eval
/// issued while the page is still loading has its callback dropped by the
/// navigation that replaces it, so the first look at a window that has
/// only just opened routinely times out. Retrying is what makes `unflick
/// dev wait body` mean what its name says instead of being the thing you
/// have to have run already.
fn poll(
    host: &dyn DevHost,
    entry: &str,
    args: Value,
    timeout: Duration,
    summarise: impl Fn(&Value, u64) -> String,
    give_up: impl Fn(&str, u64) -> String,
) -> CommandResult {
    let call = probe_call(entry, args);
    let started = std::time::Instant::now();
    let deadline = started + timeout;
    let mut why = "the window never answered".to_string();

    loop {
        match host.eval(&call, read_timeout()) {
            Ok(value) => {
                if value.get("ok").and_then(|v| v.as_bool()) == Some(false) {
                    return CommandResult::err(
                        value
                            .get("error")
                            .and_then(|v| v.as_str())
                            .unwrap_or("the page refused the command without saying why")
                            .to_string(),
                    );
                }
                let waited = started.elapsed().as_millis() as u64;
                if value.get("done").and_then(|v| v.as_bool()) == Some(true) {
                    let message = summarise(&value, waited);
                    return CommandResult::ok_with_data(message, value);
                }
                if let Some(reason) = value.get("why").and_then(|v| v.as_str()) {
                    why = reason.to_string();
                }
            }
            Err(e) => why = e,
        }

        if std::time::Instant::now() >= deadline {
            return CommandResult::err(give_up(&why, started.elapsed().as_millis() as u64));
        }
        std::thread::sleep(POLL_INTERVAL);
    }
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
    fn the_probe_defines_every_entry_point_dispatch_calls() {
        // A rename on one side only would otherwise surface as an opaque
        // "undefined is not a function" from inside a webview, on whichever
        // platform someone happened to run next.
        for entry in PROBE_ENTRY_POINTS {
            assert!(
                PROBE_JS.contains(&format!("{}: {}", entry, entry)),
                "the probe does not export {}",
                entry
            );
            assert!(
                PROBE_JS.contains(&format!("function {}(", entry)),
                "the probe does not define {}",
                entry
            );
        }
    }

    #[test]
    fn the_gate_message_does_not_leak_whether_a_window_exists() {
        // A locked-down instance must not answer "no window" — that is an
        // answer about the machine, given to a caller who was refused.
        assert!(!GATE_MESSAGE.contains("no window"));
        assert!(GATE_MESSAGE.contains("--allow-dev"));
    }

    /// A host that answers from a script, one line per look, repeating the
    /// last line forever — which is what a page that is simply never going
    /// to become ready actually does.
    struct Scripted(std::sync::Mutex<std::collections::VecDeque<Result<Value, String>>>);

    impl Scripted {
        fn new(answers: Vec<Result<Value, String>>) -> Self {
            Self(std::sync::Mutex::new(answers.into_iter().collect()))
        }
    }

    impl DevHost for Scripted {
        fn eval(&self, _js: &str, _timeout: Duration) -> Result<Value, String> {
            let mut answers = self.0.lock().unwrap();
            if answers.len() > 1 {
                answers.pop_front().unwrap()
            } else {
                answers.front().cloned().unwrap()
            }
        }
        fn capture(&self, _timeout: Duration) -> Result<Shot, String> {
            Err("no pixels in a test".into())
        }
    }

    #[test]
    fn waiting_keeps_looking_until_the_page_is_ready() {
        // The reason `wait` exists at all: the first look is routinely too
        // early, and the second or third is not.
        let host = Scripted::new(vec![
            Ok(json!({ "ok": true, "done": false, "why": "nothing matches .panel" })),
            Ok(json!({ "ok": true, "done": false, "why": "nothing matches .panel" })),
            Ok(json!({ "ok": true, "done": true, "count": 1 })),
        ]);
        let result = run(
            &host,
            Request::Wait {
                selector: ".panel".into(),
                gone: false,
                timeout: Duration::from_secs(5),
            },
        );
        assert!(result.success, "{}", result.message);
        assert!(result.message.starts_with(".panel appeared after"), "{}", result.message);
    }

    #[test]
    fn an_eval_that_fails_is_looked_at_again_rather_than_reported() {
        // An eval issued while the page is still loading has its callback
        // dropped by the navigation. That is what `dev wait body` is for,
        // so it must survive one — otherwise the command that exists to
        // wait out a load is the one thing a load can break.
        let host = Scripted::new(vec![
            Err("the window did not answer within 5.0s".into()),
            Ok(json!({ "ok": true, "done": true, "count": 1 })),
        ]);
        let result = run(
            &host,
            Request::Wait {
                selector: "body".into(),
                gone: false,
                timeout: Duration::from_secs(5),
            },
        );
        assert!(result.success, "{}", result.message);
    }

    #[test]
    fn giving_up_says_what_the_last_look_saw() {
        // "timed out" on its own sends someone to look at the wrong thing.
        // "still visible" and "in the page but hidden" send them to the
        // right one.
        let host = Scripted::new(vec![Ok(json!({
            "ok": true,
            "done": false,
            "why": "2 match(es) for .row are in the page but none is visible"
        }))]);
        let result = run(
            &host,
            Request::Wait {
                selector: ".row".into(),
                gone: false,
                // Shortest the clamp allows, so the test is not a sleep.
                timeout: Duration::from_secs_f64(MIN_TIMEOUT),
            },
        );
        assert!(!result.success);
        assert!(result.message.contains("none is visible"), "{}", result.message);
    }

    #[test]
    fn an_ambiguous_selector_is_refused_on_the_first_look() {
        // Not retried: looking again cannot make two buttons into one, and
        // clicking the first of them is the false pass this exists to stop.
        let host = Scripted::new(vec![Ok(json!({
            "ok": false,
            "error": "button matches 3 visible elements — pass an index (0–2) to say which"
        }))]);
        let result = run(
            &host,
            Request::Click {
                selector: "button".into(),
                index: None,
                timeout: Duration::from_secs(30),
            },
        );
        assert!(!result.success);
        assert!(result.message.contains("pass an index"), "{}", result.message);
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
