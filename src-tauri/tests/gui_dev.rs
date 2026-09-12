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
    let mut gui = Gui::start(&fixtures.plain);
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

    // ── the launch wrote its own timeline down ──────────────────────────
    // `unflick startup` reads the marks `boot::mark` prints, and it prints
    // them to stderr — so the log is only a log if init_file_log actually
    // points stderr at it. That redirect was written for Windows and for
    // nothing else, which meant `startup` answered "no startup marks" on
    // macOS and Linux for every release that shipped it: the file was
    // created, the marks went to the inherited stderr, and the two never
    // met. Nothing headless could see that, because the marks are emitted
    // by a launch and this is the only suite that performs one.
    //
    // Deliberately asserted on the file rather than through `startup`: the
    // parser was never the broken part, and a test that goes through it
    // would pass on a build where the file is empty and the parser is
    // simply being asked about the wrong path.
    let launch_log = std::fs::read_to_string(gui.data_dir.join("gui.log")).unwrap_or_default();
    check_msg(
        &mut broken,
        "the launch left its startup marks in UNFLICK_LOG",
        launch_log.contains("[unflick] +") && launch_log.contains("ms "),
        || {
            format!(
                "{} wrote {} bytes and none of it is a boot mark — stderr is not                  reaching the log, so `unflick startup` has nothing to parse",
                gui.data_dir.join("gui.log").display(),
                launch_log.len()
            )
        },
    );
    // The banner has to be in there too, and before the marks: it is what
    // parse_last_launch cuts runs on, so a redirect installed after it is
    // written leaves a log whose launches cannot be told apart.
    check_msg(
        &mut broken,
        "the run banner landed in the log ahead of the marks",
        launch_log
            .find("=== unflick ")
            .zip(launch_log.find("[unflick] +"))
            .map(|(banner, mark)| banner < mark)
            .unwrap_or(false),
        || format!("banner/mark order wrong in {} bytes", launch_log.len()),
    );

    // ── one wheel event moves the volume by every step it is worth ──────
    // The accumulator in lib/gesture.ts turns a deltaY into N steps and the
    // wheel handler runs the bound trigger N times. Each of those calls used
    // to read the `volume` captured when App last rendered, so all N computed
    // the same target and N notches moved the volume by one — a mouse notch
    // is deltaY 120, which is three steps, so two thirds of every scroll was
    // thrown away. gesture.test.ts is green either way: it asserts the
    // accumulator returns 5, which it always did. Only a real window shows
    // what the handler does with the 5.
    //
    // Restarted first for the same reason the storm below is: the phases
    // above take long enough that a 20-second fixture may have run out, and
    // the wheel handler ignores everything while the state is "stopped".
    let replay = gui.send("play", json!({ "file": gui.file.to_string_lossy() }));
    check(&mut broken, "restarted the fixture to scroll on", replay.ok(), &replay);
    std::thread::sleep(Duration::from_millis(1200));
    let set = gui.send("volume", json!({ "level": 100 }));
    check(&mut broken, "volume set to 100 to scroll down from", set.ok(), &set);
    // The store learns the new level from its own 250 ms poll, and the fix
    // reads the level from the store — so give it one.
    std::thread::sleep(Duration::from_millis(800));
    let wheeled = gui.send("dev_eval", json!({ "script": FIVE_WHEEL_STEPS_DOWN }));
    check_msg(
        &mut broken,
        "the wheel event reached the video area",
        wheeled.ok() && wheeled.data()["value"] == json!("dispatched"),
        || format!("{}: {}", wheeled.message(), wheeled.data()),
    );
    if wheeled.data()["value"] == json!("dispatched") {
        std::thread::sleep(Duration::from_millis(800));
        let after = gui.send("status", json!({}));
        let level = after.data()["volume"].as_f64().unwrap_or(-1.0);
        check_msg(
            &mut broken,
            "five wheel steps moved the volume by five steps",
            (level - 75.0).abs() < 0.5,
            || format!(
                "volume is {} after one deltaY 200 from 100; 75 is five steps,                  95 is the stale-closure bug applying exactly one",
                level
            ),
        );
    }

    // ── the cast panel agrees with `cast status` ────────────────────────
    //
    // Casting shipped in v0.13 with no entry point in the window on any
    // platform — the CLI and MCP had it, the GUI did not, and nobody
    // noticed for two releases because nothing headless can notice a
    // button that does not exist. This phase is the thing that would
    // have.
    //
    // Written to hold on a machine with a television and on one without,
    // because the interesting assertion is the same either way: whatever
    // the panel shows has to be what `cast list` and `cast status` say.
    let opened = gui.send("dev_click", json!({ "selector": "[data-cast-button]" }));
    check(&mut broken, "the player bar has a casting button", opened.ok(), &opened);

    if opened.ok() {
        // The search has to be visible from the first frame. Discovery
        // takes seconds by design — SSDP replies are spread across the MX
        // window — and a panel that looks empty while it waits is one
        // people click twice.
        let searching = gui.send(
            "dev_eval",
            json!({ "script": "document.querySelectorAll('[data-cast-searching]').length" }),
        );
        check_msg(
            &mut broken,
            "opening the panel starts a search and says so",
            searching.data()["value"].as_f64().unwrap_or(0.0) >= 1.0,
            || format!("{}: {}", searching.message(), searching.data()),
        );

        // Let the panel's own discovery finish, then ask the same question
        // through the same dispatcher the CLI uses.
        std::thread::sleep(Duration::from_secs(8));
        let listed = gui.send("cast", json!({ "action": "list", "seconds": 2 }));
        let found = listed.data().as_array().map(|a| a.len()).unwrap_or(0);
        let view = gui.send(
            "dev_eval",
            json!({
                "script": "document.querySelector('[data-cast-active]') ? 'casting' : \
                           document.querySelector('[data-cast-list]') ? 'renderers' : \
                           document.querySelector('[data-cast-empty]') ? 'empty' : \
                           document.querySelector('[data-cast-searching]') ? 'searching' : 'none'"
            }),
        );
        let shown = view.data()["value"].as_str().unwrap_or("none").to_string();
        let casting = gui.send("cast", json!({ "action": "status" }));
        let is_casting = casting.data().get("renderer").is_some();
        let expected = if is_casting {
            "casting"
        } else if found == 0 {
            // The case this machine can actually reach, and the one an
            // empty box explains worst: nothing answered, so the panel
            // must say what to check rather than showing a blank list.
            "empty"
        } else {
            "renderers"
        };
        check_msg(
            &mut broken,
            "the panel shows what `cast list` and `cast status` report",
            shown == expected,
            || format!(
                "panel is showing {:?} while cast list found {} renderer(s) and cast status says {:?}",
                shown, found, casting.message()
            ),
        );

        // Closing and reopening must re-ask. The trap is `AnimatePresence`:
        // reopen a popover inside its own 120 ms close and the exit is
        // reversed rather than the component remounted, so a panel that
        // reads state in a mount effect goes on showing the televisions
        // that answered the first time — including ones since switched off.
        let closed = gui.send("dev_click", json!({ "selector": "[data-cast-button]" }));
        check(&mut broken, "the casting button closes the panel again", closed.ok(), &closed);
        let reopened = gui.send("dev_click", json!({ "selector": "[data-cast-button]" }));
        check(&mut broken, "the casting button reopens the panel", reopened.ok(), &reopened);
        let fresh = gui.send(
            "dev_eval",
            json!({
                "script": "document.querySelectorAll('[data-cast-searching]').length + ':' + \
                           document.querySelectorAll('[data-cast-empty]').length"
            }),
        );
        let fresh_value = fresh.data()["value"].as_str().unwrap_or("").to_string();
        check_msg(
            &mut broken,
            "reopening the panel searches again instead of showing the last answer",
            fresh_value.starts_with("1:") && fresh_value.ends_with(":0"),
            || format!(
                "searching:empty is {:?} right after the reopen — anything but \"1:0\" means the \
                 panel kept the previous search, which is the AnimatePresence reversal",
                fresh_value
            ),
        );
        // Leave it closed so the phases after this one see the bar.
        let _ = gui.send("dev_click", json!({ "selector": "[data-cast-button]" }));
    }

    // ── the render thread outlives a geometry storm ─────────────────────
    // Last, because it is the phase that can take the window down with it,
    // and everything above should get its answer first.
    //
    // What it is for: `video_surface_set_geometry` runs on the AppKit main
    // thread, and on macOS it resizes the NSView and calls
    // `[NSOpenGLContext update]` — both of which reallocate the drawable
    // that the render thread is at that moment painting into from inside
    // `mpv_render_context_render`. Unsynchronised, that is a SIGSEGV in
    // AppleMetalOpenGLRenderer: 10 out of 10 runs died within 3 seconds of
    // this storm starting, and 1 in 10 idle launches died on the single
    // geometry push the frontend makes when it mounts.
    //
    // Two things are asserted, because the obvious fix for the first
    // causes the second: that the process is still alive (no crash), and
    // that playback has moved on (no deadlock — a render thread wedged
    // waiting on a lock stalls mpv behind a video queue that never
    // drains, and the position stops).
    // Start the file over first: the phases above take long enough that a
    // 20-second fixture may already have run out, and "the position did
    // not move" would then be true for a reason that is not a bug.
    let restart = gui.send("play", json!({ "file": gui.file.to_string_lossy() }));
    check(&mut broken, "restarted the fixture for the storm", restart.ok(), &restart);
    let before = gui.send("status", json!({}));
    let started_at = before.data()["position"].as_f64().unwrap_or(-1.0);
    check_msg(
        &mut broken,
        "playback had a position to compare against",
        before.ok() && started_at >= 0.0,
        || format!("{}", before.data()),
    );
    let storm = gui.send("dev_eval", json!({ "script": A_GEOMETRY_STORM }));
    check(&mut broken, "the geometry storm started", storm.ok(), &storm);
    if storm.ok() {
        std::thread::sleep(STORM);
        match gui.exited() {
            Some(status) => broken.push(format!(
                "the window died under a geometry storm after {:?}: {}. A SIGSEGV \
                 here is the render thread and the main thread in the same GL \
                 context at the same time — see VideoSurface::lock_gl",
                STORM, status
            )),
            None => {
                let stop = gui.send(
                    "dev_eval",
                    json!({ "script": "clearInterval(window.__unflickStorm); window.__unflickStorm = 0; return __unflickStormPushes" }),
                );
                check_msg(
                    &mut broken,
                    "the window still answered after the storm",
                    stop.ok() && stop.data()["value"].as_f64().unwrap_or(0.0) > 0.0,
                    || format!("{}: {}", stop.message(), stop.data()),
                );
                let after = gui.send("status", json!({}));
                let ended_at = after.data()["position"].as_f64().unwrap_or(-1.0);
                check_msg(
                    &mut broken,
                    "playback moved on through the storm",
                    after.ok() && ended_at > started_at,
                    || format!("position went {} → {}", started_at, ended_at),
                );
            }
        }
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

/// One wheel event carrying five steps of travel (the accumulator's
/// threshold is 40), dispatched onto the element that listens for it.
///
/// Dispatched on the video area itself rather than on `body`: the handler is
/// React's `onWheel` on that div, and an event fired at an ancestor never
/// reaches it.
const FIVE_WHEEL_STEPS_DOWN: &str = "\
var el = document.querySelector('div.relative.flex.flex-1.items-center.justify-center.overflow-hidden');
if (!el) return 'the video area was not found — its class list changed';
var r = el.getBoundingClientRect();
el.dispatchEvent(new WheelEvent('wheel', {
  bubbles: true, cancelable: true, deltaY: 200,
  clientX: r.left + r.width / 2, clientY: r.top + r.height / 2, view: window
}));
return 'dispatched';";

/// How long to hold the geometry storm. Unsynchronised, the median run
/// died 0.7 s in and the slowest of ten took 2.8 s, so this is a wide
/// margin over the worst measured — long enough to mean something, short
/// enough to fit inside the 20-second fixture that has to keep playing
/// through it.
const STORM: Duration = Duration::from_secs(10);

/// Push a new video-surface rect twice a frame, the way a window being
/// dragged by its corner does, only without pause. Two things matter and
/// both were measured: the sizes have to actually change (the frontend's
/// own sync skips a rect equal to the last one, and so would the
/// interesting half of the backend), and bigger rects catch it more
/// often, a bigger reallocation being a wider window to land in.
///
/// How reliably this catches an unfixed build depends on something the
/// test cannot control. With the screen unlocked it is near-certain: a
/// storm of 560x380-ish rects killed the unfixed build 10 times out of
/// 10, median 0.65 s. With the screen locked the race narrows sharply and
/// these larger sizes caught it 4 times in 25. So a green run on a locked
/// machine is weak evidence; a green run on an unlocked one is strong.
const A_GEOMETRY_STORM: &str = "\
if (window.__unflickStorm) return 'already';
window.__unflickStormPushes = 0;
var n = 0;
window.__unflickStorm = setInterval(function () {
  n++;
  var w = 900 + (n % 11) * 90;
  var h = 600 + (n % 9) * 70;
  window.__unflickStormPushes++;
  window.__TAURI_INTERNALS__
    .invoke('video_surface_set_geometry', { x: 0, y: 0, w: w, h: h })
    .catch(function () {});
}, 8);
return 'storming';";

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
    /// The file it was launched with, kept so a phase can start it over.
    file: PathBuf,
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

        let gui = Self { child, addr, data_dir, file: file.to_path_buf() };
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

    /// The exit status if the window is already gone, `None` while it is
    /// still running. Asked rather than inferred from a refused
    /// connection: a process that took a signal and one that is merely
    /// too busy to accept look the same from the socket.
    fn exited(&mut self) -> Option<std::process::ExitStatus> {
        self.child.try_wait().ok().flatten()
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
