//! The one test that opens a window.
//!
//! Every other suite here proves something about `core/`. This one proves
//! the thing `core/` cannot: that `unflick dev` reaches an actual webview,
//! runs a program in it, and gets an answer back. Without it the track
//! ships a bridge nobody ever drove — which is the exact failure it was
//! built to stop, one level up.
//!
//! # Why it is its own binary
//!
//! Deliberately. This is the highest-flake-risk thing in the tree: it
//! launches a real GUI, on a real window server, on three platforms. In its
//! own binary a bad day here cannot mask a playback or understanding
//! regression, and `cargo test --test playback` stays a thing you can trust
//! on its own.
//!
//! # Why it is one test with phases
//!
//! `tauri_plugin_single_instance` is keyed on the app identifier, so a
//! second unflick process does not start a second window — it hands its
//! arguments to the first and exits. Two `#[test]`s launching two windows
//! would therefore not be two windows, and the second would fail for a
//! reason that has nothing to do with what it was testing. So: one window,
//! one test, and the phases collect their failures rather than panicking at
//! the first, because a broken `click` should not hide a broken `snapshot`.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// Well clear of the range `common::Daemon` hands out, and of the 19542 a
/// developer's own player uses.
const PORT: u16 = 29831;

#[test]
fn the_bridge_drives_the_real_window() {
    require_an_embedded_frontend();

    let fixtures = common::fixtures();
    let gui = Gui::start(&fixtures.plain);
    let mut broken: Vec<String> = Vec::new();

    // ── the page is there ───────────────────────────────────────────────
    // First, and not just for ordering: an eval issued before the first
    // document finishes loading has its callback dropped by the navigation
    // that replaces it. `wait` retries its eval, so this is the phase that
    // absorbs the load, and everything after it can assume a page.
    let ready = gui.send("dev_wait", json!({ "selector": "body", "timeout_seconds": 30 }));
    check(&mut broken, "dev_wait body", ready.ok(), &ready);
    if !ready.ok() {
        // Nothing below can mean anything without a page. Say so once,
        // rather than producing eight more failures that all say the same.
        panic!(
            "the window never produced a page, so nothing else could be tested: {}",
            ready.message()
        );
    }

    // ── eval really evaluates ───────────────────────────────────────────
    let title = gui.send("dev_eval", json!({ "script": "document.title" }));
    check(
        &mut broken,
        "dev_eval document.title",
        title.ok() && title.data()["value"] == json!("unflick"),
        &title,
    );

    // A value JSON cannot carry is refused by name rather than arriving as
    // an empty object, which is what `JSON.stringify(document.body)` is.
    let node = gui.send("dev_eval", json!({ "script": "document.body" }));
    check(
        &mut broken,
        "dev_eval refuses a DOM node by name",
        !node.ok() && node.message().contains("<body>"),
        &node,
    );

    // A thrown error arrives as a sentence. WebKit's bare `stack` is a
    // frame list with the message missing, which tells nobody anything.
    let boom = gui.send("dev_eval", json!({ "script": "definitelyNotDefined.x" }));
    check(
        &mut broken,
        "dev_eval reports a throw with its message",
        !boom.ok() && boom.message().contains("Error"),
        &boom,
    );

    // ── the interface, read as an interface ─────────────────────────────
    // Waited for, not slept for. `body` exists before React has rendered a
    // thing, so a snapshot taken on the strength of the phase above finds
    // one node and proves nothing.
    let mounted = gui.send("dev_wait", json!({ "selector": "button", "timeout_seconds": 30 }));
    check(&mut broken, "dev_wait button (the interface rendered)", mounted.ok(), &mounted);

    let snapshot = gui.send("dev_snapshot", json!({ "depth": 6 }));
    let nodes = snapshot.data()["nodes"].as_array().cloned().unwrap_or_default();
    check(
        &mut broken,
        "dev_snapshot returns a tree",
        snapshot.ok() && nodes.len() > 5,
        &snapshot,
    );
    // The bug class this exists for: a label rendering the literal string
    // "undefined" compiles, passes every headless test, and is visible
    // here and only here.
    let undefined_labels: Vec<&Value> = nodes
        .iter()
        .filter(|n| n["name"] == json!("undefined") || n["name"] == json!("null"))
        .collect();
    check_msg(
        &mut broken,
        "no node is named \"undefined\"",
        undefined_labels.is_empty(),
        || format!("{:?}", undefined_labels),
    );
    // Every selector a snapshot hands out has to find its own node, or it
    // is worse than no selector at all — an instruction that silently does
    // the wrong thing when pasted into `dev click`.
    //
    // Checked inside one eval, and that is not a shortcut. Asked a second
    // later over a second round trip, some of them legitimately match
    // nothing: this app has no `id`s and no `data-testid`s anywhere, so
    // every selector is positional, and the player-bar overlay hides itself
    // after a few seconds of no mouse. That is the interface changing, not
    // the selector being wrong, and a test that conflated the two would
    // fail on a timer.
    let round_trip = gui.send("dev_eval", json!({ "script": SELECTORS_FIND_THEIR_OWN_NODE }));
    let offenders = round_trip.data()["value"]["bad"].to_string();
    let counted = round_trip.data()["value"]["checked"].as_u64().unwrap_or(0);
    check_msg(
        &mut broken,
        "every snapshot selector matches exactly its own node",
        round_trip.ok() && counted > 5 && offenders == "[]",
        || format!("checked {}, offenders {}: {}", counted, offenders, round_trip.message()),
    );

    // ── text ────────────────────────────────────────────────────────────
    let text = gui.send("dev_text", json!({ "selector": "button" }));
    check(
        &mut broken,
        "dev_text returns every match",
        text.ok() && text.data()["matches"].as_array().map(|a| a.len()).unwrap_or(0) > 1,
        &text,
    );
    let missing = gui.send("dev_text", json!({ "selector": ".nothing-is-here" }));
    check(
        &mut broken,
        "dev_text names a selector that matches nothing",
        !missing.ok() && missing.message().contains(".nothing-is-here"),
        &missing,
    );

    // ── click refuses rather than guessing ──────────────────────────────
    let nowhere = gui.send(
        "dev_click",
        json!({ "selector": ".nothing-is-here", "timeout_seconds": 0.5 }),
    );
    check(
        &mut broken,
        "dev_click names a selector that matches nothing",
        !nowhere.ok() && nowhere.message().contains(".nothing-is-here"),
        &nowhere,
    );
    // The whole reason for the hit test and the ambiguity check: clicking
    // the first of several is how a suite comes back green having pressed
    // the wrong button.
    let ambiguous = gui.send(
        "dev_click",
        json!({ "selector": "button", "timeout_seconds": 0.5 }),
    );
    check(
        &mut broken,
        "dev_click refuses an ambiguous selector and counts the matches",
        !ambiguous.ok() && ambiguous.message().contains("pass an index"),
        &ambiguous,
    );

    // ── click actually clicks ───────────────────────────────────────────
    // Against a control of our own rather than a real button: this phase is
    // about the event sequence arriving, and a real button would also be
    // testing whatever the app does next.
    let plant = gui.send("dev_eval", json!({ "script": PLANT_A_BUTTON }));
    check(&mut broken, "planted a button to click", plant.ok(), &plant);
    let clicked = gui.send("dev_click", json!({ "selector": "#unflick-probe-button" }));
    check(&mut broken, "dev_click clicks it", clicked.ok(), &clicked);
    let heard = gui.send(
        "dev_eval",
        json!({ "script": "window.__unflickProbeHeard || []" }),
    );
    // `pointerdown` because React and Framer Motion listen there far more
    // often than on `click`; `click` because only it performs a native
    // activation. Both, or a press is reported that something missed.
    let events = heard.data()["value"].to_string();
    check_msg(
        &mut broken,
        "the click was heard as a real press",
        events.contains("pointerdown") && events.contains("mousedown") && events.contains("click"),
        || format!("the element heard {}", events),
    );

    // ── wait, in both directions ────────────────────────────────────────
    let vanish = gui.send(
        "dev_eval",
        json!({ "script": "setTimeout(function () { var e = document.getElementById('unflick-probe-button'); if (e) e.remove(); }, 300); return 'scheduled'" }),
    );
    check(&mut broken, "scheduled the element to leave", vanish.ok(), &vanish);
    let gone = gui.send(
        "dev_wait",
        json!({ "selector": "#unflick-probe-button", "gone": true, "timeout_seconds": 10 }),
    );
    // Verifying that a panel *closed* is impossible without this half, and
    // an assertion run immediately after the click that closes it passes
    // for the wrong reason.
    check(&mut broken, "dev_wait --gone waits for it to leave", gone.ok(), &gone);

    // ── capture ─────────────────────────────────────────────────────────
    // Either a real picture or a refusal that names why there is none.
    // Never a black rectangle reported as a success — a window that is
    // minimised, covered, or behind a locked screen is the normal state of
    // an unattended machine, and this is the assertion that keeps the verb
    // honest there.
    let shot = gui.send("dev_capture", json!({ "max_edge": 320 }));
    if shot.ok() {
        let data = shot.data();
        let width = data["width"].as_u64().unwrap_or(0);
        let bytes = data["bytes"].as_u64().unwrap_or(0);
        check_msg(
            &mut broken,
            "dev_capture returned a picture with pixels in it",
            width > 0 && bytes > 0 && data["base64"].is_string(),
            || format!("{}", data),
        );
    } else {
        let named = ["locked", "minimis", "covered", "flat colour", "not on screen"]
            .iter()
            .any(|cause| shot.message().contains(cause));
        check_msg(
            &mut broken,
            "dev_capture refused, naming the cause",
            named,
            || format!("refused without naming a cause: {}", shot.message()),
        );
    }

    // ── full resolution to a file ───────────────────────────────────────
    // Skipped honestly when the capture above could not happen at all;
    // there is nothing to learn from asserting on a second refusal.
    if shot.ok() {
        let path = std::env::temp_dir().join("unflick-gui-dev-capture.png");
        let _ = std::fs::remove_file(&path);
        let written = gui.send(
            "dev_capture",
            json!({ "output": path.to_string_lossy() }),
        );
        let magic = std::fs::read(&path).unwrap_or_default();
        check_msg(
            &mut broken,
            "dev_capture --output writes a real PNG",
            written.ok() && magic.starts_with(&[0x89, b'P', b'N', b'G']),
            || format!("{} wrote {} bytes", written.message(), magic.len()),
        );
        let _ = std::fs::remove_file(&path);
    }

    drop(gui);
    assert!(
        broken.is_empty(),
        "{} of the window bridge's promises did not hold:\n  {}",
        broken.len(),
        broken.join("\n  ")
    );
}

/// Take a snapshot and immediately ask each selector to find its node.
const SELECTORS_FIND_THEIR_OWN_NODE: &str = "\
var snap = __unflickDev.snapshot({ depth: 12 });
var withSelector = snap.nodes.filter(function (n) { return n.selector; });
var bad = withSelector
  .map(function (n) { return [n.selector, document.querySelectorAll(n.selector).length]; })
  .filter(function (pair) { return pair[1] !== 1; });
return { checked: withSelector.length, nodes: snap.nodes.length, bad: bad };";

/// A button that records what it was sent.
const PLANT_A_BUTTON: &str = "\
window.__unflickProbeHeard = [];
var old = document.getElementById('unflick-probe-button');
if (old) old.remove();
var b = document.createElement('button');
b.id = 'unflick-probe-button';
b.textContent = 'probe';
b.style.cssText = 'position:fixed;left:8px;top:8px;width:120px;height:32px;z-index:2147483647';
['pointerdown','mousedown','mouseup','click'].forEach(function (name) {
  b.addEventListener(name, function () { window.__unflickProbeHeard.push(name); });
});
document.body.appendChild(b);
return 'planted';";

fn check(broken: &mut Vec<String>, what: &str, held: bool, reply: &Reply) {
    check_msg(broken, what, held, || reply.message().to_string());
}

fn check_msg(broken: &mut Vec<String>, what: &str, held: bool, detail: impl FnOnce() -> String) {
    if !held {
        broken.push(format!("{}: {}", what, detail()));
    }
}

/// Refuse to run against a binary whose window cannot show the app.
///
/// Tauri picks between the built `dist/` and `devUrl` at compile time on
/// the `custom-protocol` feature. Without it the webview navigates to
/// http://localhost:1420 and, with no vite server there, sits on
/// about:blank — where `dev_eval` answers, `document.title` is empty, and
/// every assertion below fails for a reason that has nothing to do with the
/// bridge. Panicking with the command to run beats eleven confusing
/// failures, and beats skipping: a verification that quietly does not run
/// is the thing this whole track exists to stop.
fn require_an_embedded_frontend() {
    if !cfg!(feature = "custom-protocol") {
        panic!(
            "this test needs a GUI binary that serves its own frontend. Build the \
             frontend once (`pnpm build`, from the repository root) and run:\n\n    \
             cargo test --test gui_dev --features custom-protocol\n\n\
             Without the feature Tauri points the webview at devUrl \
             (http://localhost:1420) and the window comes up blank."
        );
    }
}

/// The real GUI, on a private port with a private database. Dropping it
/// kills the process, so a failed assertion cannot leave a window on
/// someone's screen.
struct Gui {
    child: Child,
    addr: String,
    data_dir: PathBuf,
}

impl Gui {
    fn start(file: &std::path::Path) -> Self {
        let addr = format!("127.0.0.1:{}", PORT);
        assert!(
            TcpListener::bind(&addr).is_ok(),
            "port {} is already in use — another unflick, or a previous run that \
             did not clean up",
            addr
        );

        let data_dir = std::env::temp_dir().join(format!("unflick-gui-dev-{}", PORT));
        let _ = std::fs::remove_dir_all(&data_dir);
        std::fs::create_dir_all(&data_dir).expect("create the test data dir");

        let log = data_dir.join("gui.log");
        let child = Command::new(env!("CARGO_BIN_EXE_unflick"))
            // The flag and the file together: `gui_launch_file` in main.rs
            // takes both, and this is the launch it takes them for.
            .arg("--allow-dev")
            .arg(file)
            .env(unflick_lib::core::daemon::CONTROL_ADDR_ENV, &addr)
            .env(unflick_lib::db::DATA_DIR_ENV, &data_dir)
            .env(unflick_lib::core::settings::CONFIG_DIR_ENV, &data_dir)
            .env(unflick_lib::core::boot::LOG_PATH_ENV, &log)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn the unflick GUI");

        let gui = Self { child, addr, data_dir };
        gui.wait_until_listening();
        gui
    }

    fn wait_until_listening(&self) {
        // Generous, and for the same reason the headless harness is: this
        // runs while rustc is saturating the machine, and a window has more
        // to do before it answers than a daemon does.
        let deadline = Instant::now() + Duration::from_secs(90);
        while Instant::now() < deadline {
            if TcpStream::connect(&self.addr).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        panic!(
            "the GUI never listened on {}. Two causes worth checking before the \
             code: another unflick is already running, in which case \
             tauri-plugin-single-instance handed our arguments to it and our \
             process exited; or there is no window server for this session at all \
             (on Linux, run under xvfb-run).",
            self.addr
        );
    }

    fn send(&self, command: &str, args: Value) -> Reply {
        let stream = TcpStream::connect(&self.addr).expect("connect to the GUI");
        stream
            .set_read_timeout(Some(Duration::from_secs(180)))
            .expect("set a read timeout");
        let mut writer = stream.try_clone().expect("clone the stream");
        let mut reader = BufReader::new(stream);

        writeln!(writer, "{}", json!({ "command": command, "args": args }))
            .expect("write the command");
        let mut line = String::new();
        reader.read_line(&mut line).expect("read the reply");
        Reply(
            serde_json::from_str(&line)
                .unwrap_or_else(|e| panic!("the GUI returned invalid JSON: {e}: {line}")),
        )
    }
}

impl Drop for Gui {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.data_dir);
    }
}

struct Reply(Value);

impl Reply {
    fn ok(&self) -> bool {
        self.0["success"].as_bool().unwrap_or(false)
    }
    fn message(&self) -> &str {
        self.0["message"].as_str().unwrap_or("")
    }
    fn data(&self) -> Value {
        self.0.get("data").cloned().unwrap_or(Value::Null)
    }
}
