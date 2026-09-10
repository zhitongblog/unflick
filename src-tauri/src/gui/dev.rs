//! The window half of the dev bridge: running a program in the webview and
//! getting the answer back out.
//!
//! `core::dev` decides what every verb means and how every refusal reads.
//! This file solves the one problem that is genuinely the window's: Tauri's
//! `eval` is fire-and-forget (`WebviewWindow::eval` returns `Result<()>`,
//! not the value), so the program has to carry its own way home.
//!
//! # The round trip
//!
//! Each call takes an id, parks a `Sender` under it, and evals a program
//! that ends by handing the answer to `window.__TAURI_INTERNALS__.invoke`.
//! That lands in [`dev_report`] — an ordinary Tauri command — which posts
//! the value into the channel the caller is blocked on.
//!
//! It goes through Tauri's own injected runtime rather than through an
//! event the frontend listens for, and that is the decision worth not
//! re-deriving. A `listen()` in React would be frontend code that has to be
//! kept in step, and — decisively — the bridge would die with the app's
//! JavaScript. The whole point of this thing is diagnosing a broken UI, so
//! a completely blown React tree must still answer `dev snapshot` and
//! `dev capture`. `__TAURI_INTERNALS__` is injected into every webview
//! before any of our code runs (`tauri::manager::webview` renders
//! `scripts/core.js` into the init script unconditionally, with no
//! `withGlobalTauri` guard), so it is there even when nothing else is.
//!
//! # What the answer is not allowed to be
//!
//! A value that JSON cannot carry is refused by name — a DOM element, a
//! function — rather than quietly coerced. `JSON.stringify(document.body)`
//! is `{}`, and an empty object arriving where someone expected an element
//! is a worse answer than "you cannot return an element; here is what to
//! return instead".

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use tauri::{AppHandle, Manager, State};

use crate::core::dev::{DevHost, Shot, PROBE_JS};

/// Refusal when the app is up but its window has gone.
///
/// Distinct from `core::dev::NO_WINDOW_MESSAGE`, which is what a host with
/// no webview at all says. This one means there was one and it closed —
/// different problem, different fix.
const WINDOW_CLOSED: &str =
    "the unflick window is closed — dev commands need the player window on screen";

pub struct TauriDevHost {
    app: AppHandle,
    /// Ids are never reused within a process, so a late answer from a call
    /// that already timed out finds no sender and is dropped instead of
    /// being handed to whoever is waiting next.
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, Sender<Value>>>,
}

impl TauriDevHost {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            next_id: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// Hand a webview's answer to whoever asked for it.
    fn deliver(&self, id: u64, payload: Value) {
        let sender = self.pending.lock().unwrap().remove(&id);
        if let Some(sender) = sender {
            let _ = sender.send(payload);
        }
        // No sender means the caller gave up first. Nothing to do and
        // nothing to report — the caller already has its timeout message.
    }
}

impl DevHost for TauriDevHost {
    fn eval(&self, js: &str, timeout: Duration) -> Result<Value, String> {
        let window = self
            .app
            .get_webview_window("main")
            .ok_or(WINDOW_CLOSED)?;

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id, tx);

        if let Err(e) = window.eval(build_program(id, js)) {
            self.pending.lock().unwrap().remove(&id);
            return Err(format!("the window would not take the script: {}", e));
        }

        let answer = rx.recv_timeout(timeout);
        // Removed on every path, including success: `deliver` has already
        // taken it, and leaving a stale entry behind would leak a sender
        // per call for the life of the process.
        self.pending.lock().unwrap().remove(&id);

        match answer {
            Ok(payload) => unwrap_payload(payload),
            Err(_) => Err(format!(
                "the window did not answer within {:.1}s. If unflick has only just \
                 started, its page may still be loading — an eval sent before the \
                 first document finishes is dropped by the navigation that replaces \
                 it. `unflick dev wait body` waits for the page, then retry.",
                timeout.as_secs_f64()
            )),
        }
    }

    fn capture(&self, timeout: Duration) -> Result<Shot, String> {
        let window = self
            .app
            .get_webview_window("main")
            .ok_or(WINDOW_CLOSED)?;
        super::dev_capture::capture(&window, timeout)
    }
}

/// The webview handing back what a program evaluated to.
///
/// Registered like any other command. It is reachable from page JavaScript
/// that did not come from us, which costs nothing: the worst a caller can
/// do is answer a question nobody asked (dropped, no sender) or answer one
/// early with rubbish — and anything running in that page could already
/// have done whatever it liked to the page itself.
#[tauri::command]
pub fn dev_report(host: State<'_, Arc<TauriDevHost>>, id: u64, payload: Value) {
    host.deliver(id, payload);
}

/// `{value: …}` becomes the value; `{error: …}` becomes the refusal.
fn unwrap_payload(payload: Value) -> Result<Value, String> {
    if let Some(error) = payload.get("error").and_then(|v| v.as_str()) {
        return Err(error.to_string());
    }
    match payload.get("value") {
        Some(value) => Ok(value.clone()),
        // Neither field: something other than our own program invoked
        // `dev_report`. Say what arrived rather than pretending it is a
        // result.
        None => Err(format!(
            "the window sent something that is not a dev answer: {}",
            payload
        )),
    }
}

/// The probe, plus a wrapper that runs `js` and reports what it produced.
///
/// The user's script is embedded as a JSON string literal and compiled with
/// `new Function`, expression form first and statement form on the
/// SyntaxError, so `document.title` and `let x = 1; return x` both work
/// without either being escaped by hand.
///
/// **`new Function` needs `"csp": null` in tauri.conf.json.** It is null
/// today and there is no CSP meta tag in index.html. Adding either would
/// break every dev command at once with an opaque `unsafe-eval` violation
/// from inside the webview — the failure would look like the bridge, and it
/// would be the policy. tauri.conf.json is strict JSON with nowhere to put
/// this warning, so it lives here, at the line that depends on it.
fn build_program(id: u64, js: &str) -> String {
    // serde_json, not manual quoting: a script containing a quote, a
    // backslash, a newline or a line separator is an ordinary script.
    let source = Value::String(js.to_string()).to_string();
    format!(
        r#"{probe}
;(function () {{
  var __id = {id};
  var __src = {source};

  function __send(payload) {{
    try {{
      window.__TAURI_INTERNALS__.invoke('dev_report', {{ id: __id, payload: payload }});
    }} catch (e) {{
      // Nothing left to report to. The caller's timeout is the message.
    }}
  }}

  function __wire(value) {{
    if (value === undefined) return {{ value: null }};
    var t = typeof value;
    if (t === 'function' || t === 'symbol' || t === 'bigint') {{
      return {{ error: 'the script returned a ' + t + ', which JSON cannot carry' }};
    }}
    if (value && t === 'object' && typeof value.nodeType === 'number') {{
      var tag = value.tagName
        ? value.tagName.toLowerCase()
        : String(value.nodeName || 'node').toLowerCase();
      return {{ error: 'the script returned a <' + tag + '>, which JSON cannot carry — '
        + 'return something out of it (textContent, className, '
        + 'getBoundingClientRect()) or use: unflick dev snapshot' }};
    }}
    try {{
      return {{ value: JSON.parse(JSON.stringify(value)) }};
    }} catch (e) {{
      return {{ error: 'the value the script produced cannot be turned into JSON: '
        + (e && e.message ? e.message : e) }};
    }}
  }}

  var __fn;
  try {{
    __fn = new Function('"use strict"; return (async function () {{ return (' + __src + '); }})();');
  }} catch (e1) {{
    try {{
      __fn = new Function('"use strict"; return (async function () {{' + __src + '\n}})();');
    }} catch (e2) {{
      __send({{ error: 'the script did not parse: ' + (e2 && e2.message ? e2.message : e2) }});
      return;
    }}
  }}

  try {{
    Promise.resolve(__fn()).then(
      function (v) {{ __send(__wire(v)); }},
      function (err) {{ __send({{ error: String(err && err.stack ? err.stack : err) }}); }}
    );
  }} catch (err) {{
    __send({{ error: String(err && err.stack ? err.stack : err) }});
  }}
}})();
"#,
        probe = PROBE_JS,
        id = id,
        source = source
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_script_with_quotes_and_newlines_survives_into_the_program() {
        // The reason the script is a JSON literal and not concatenation.
        let program = build_program(7, "return \"a\\nb\";\n// trailing comment");
        assert!(program.contains(r#"var __id = 7;"#), "{}", program);
        // The literal must parse back to exactly what was asked for —
        // including the trailing comment, which would otherwise swallow the
        // rest of the wrapper.
        let start = program.find("var __src = ").unwrap() + "var __src = ".len();
        let end = program[start..].find(";\n").unwrap() + start;
        let parsed: Value = serde_json::from_str(&program[start..end]).unwrap();
        assert_eq!(parsed, json!("return \"a\\nb\";\n// trailing comment"));
    }

    #[test]
    fn the_probe_rides_along_with_every_program() {
        // Every eval carries it, so a call made before any other has run
        // still finds `__unflickDev` defined.
        let program = build_program(1, "1");
        assert!(program.contains("globalThis.__unflickDev"), "probe missing");
        assert!(program.contains("dev_report"), "no way home");
    }

    #[test]
    fn an_error_payload_is_the_refusal_and_not_a_value() {
        assert_eq!(
            unwrap_payload(json!({ "error": "nothing matches .play" })).unwrap_err(),
            "nothing matches .play"
        );
        assert_eq!(unwrap_payload(json!({ "value": 42 })).unwrap(), json!(42));
        assert!(unwrap_payload(json!({ "surprise": 1 }))
            .unwrap_err()
            .contains("not a dev answer"));
    }
}

