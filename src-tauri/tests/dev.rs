//! The dev surface, held to account without a window.
//!
//! Everything here runs against the headless daemon, which is the only host
//! CI can create and also the only one that can prove the refusals are
//! honest. The bridge exists because six bugs in the 2026-08-27 batch
//! compiled cleanly and passed the whole headless suite; a bridge that
//! quietly answered "ok" from a process with no window would be the same
//! failure wearing the uniform of the fix.
//!
//! What is *not* here: that `eval` evaluates anything. That needs a real
//! window and lives in `tests/gui_dev.rs`.

mod common;

use std::net::TcpListener;
use std::process::Command;

use serde_json::{json, Value};

use common::{mcp_roundtrip, Daemon};

/// Every verb, with arguments valid enough to get past parsing. The list is
/// a constant so a seventh verb added without a gate check fails this suite
/// rather than shipping open.
const VERBS: [(&str, fn() -> Value); 6] = [
    ("dev_eval", || json!({ "script": "1 + 1" })),
    ("dev_click", || json!({ "selector": "body" })),
    ("dev_text", || json!({ "selector": "body" })),
    ("dev_snapshot", || json!({})),
    ("dev_capture", || json!({})),
    ("dev_wait", || json!({ "selector": "body" })),
];

// ─── The gate ─────────────────────────────────────────────────────────────

#[test]
fn every_dev_command_is_refused_when_the_surface_was_never_armed() {
    let daemon = Daemon::start();
    for (cmd, args) in VERBS {
        daemon
            .send(cmd, args())
            .expect_err_containing("--allow-dev");
    }
}

#[test]
fn the_gate_is_answered_before_the_window() {
    // A locked-down instance must not reveal whether there is a window to
    // drive. "no window" is an answer about the machine, and a caller who
    // was refused has not earned one.
    let daemon = Daemon::start();
    let reply = daemon.send("dev_eval", json!({ "script": "1" }));
    assert!(!reply.success(), "an unarmed daemon answered dev_eval");
    assert!(
        !reply.message().to_lowercase().contains("no window"),
        "the gate leaked whether a window exists: {}",
        reply.message()
    );
}

#[test]
fn the_gate_is_answered_before_the_arguments() {
    // Same reasoning one step further: an unarmed daemon must not become an
    // argument validator for a surface it will not run.
    let daemon = Daemon::start();
    let reply = daemon.send("dev_click", json!({}));
    reply.expect_err_containing("--allow-dev");
    assert!(
        !reply.message().contains("selector is required"),
        "the gate leaked the argument shape: {}",
        reply.message()
    );
}

// ─── Armed, but with nothing on screen ────────────────────────────────────

#[test]
fn an_armed_daemon_with_no_window_says_so_rather_than_pretending() {
    let daemon = Daemon::start_with_dev();
    for (cmd, args) in VERBS {
        let reply = daemon.send(cmd, args());
        assert!(!reply.success(), "`{}` succeeded without a window", cmd);
        assert_eq!(
            reply.message(),
            "no window — dev commands need the unflick GUI running",
            "`{}` gave the wrong refusal",
            cmd
        );
    }
}

#[test]
fn an_unknown_dev_command_names_the_ones_that_exist() {
    // The same courtesy `WindowMode::from_str` extends: a typo is reported
    // as a typo, with the alternatives, and never as a missing window.
    let daemon = Daemon::start_with_dev();
    let reply = daemon.send("dev_frobnicate", json!({}));
    assert!(!reply.success());
    for verb in ["eval", "click", "text", "snapshot", "capture", "wait"] {
        assert!(
            reply.message().contains(verb),
            "{:?} does not mention {}",
            reply.message(),
            verb
        );
    }
    assert!(
        !reply.message().contains("no window"),
        "a typo was reported as a missing window: {}",
        reply.message()
    );
}

#[test]
fn a_missing_required_argument_is_named_before_the_window_is_looked_for() {
    let daemon = Daemon::start_with_dev();
    for (cmd, key) in [
        ("dev_eval", "script"),
        ("dev_click", "selector"),
        ("dev_text", "selector"),
        ("dev_wait", "selector"),
    ] {
        let reply = daemon.send(cmd, json!({}));
        assert_eq!(
            reply.message(),
            format!("{} is required", key),
            "`{}` reported the wrong problem",
            cmd
        );
    }
}

#[test]
fn an_argument_that_is_only_whitespace_counts_as_missing() {
    // `--selector " "` is a shell accident, not a selector, and matching
    // nothing would be reported as "nothing matches" — a true statement
    // that sends someone looking at their UI instead of their command.
    let daemon = Daemon::start_with_dev();
    let reply = daemon.send("dev_click", json!({ "selector": "   " }));
    assert_eq!(reply.message(), "selector is required");
}

#[test]
fn out_of_range_arguments_are_clamped_rather_than_refused() {
    // A mistyped `--timeout 6000` should not be a hard error — the command
    // still means something. It is clamped, and the proof that it was
    // clamped rather than honoured is that the call reaches the window
    // check instead of hanging for an hour and a half.
    let daemon = Daemon::start_with_dev();
    for args in [
        json!({ "script": "1", "timeout_seconds": 6000 }),
        json!({ "script": "1", "timeout_seconds": 0 }),
        json!({ "script": "1", "timeout_seconds": -4 }),
    ] {
        let reply = daemon.send("dev_eval", args.clone());
        assert_eq!(
            reply.message(),
            "no window — dev commands need the unflick GUI running",
            "{} was not clamped through to the window check",
            args
        );
    }
    for args in [json!({ "depth": 0 }), json!({ "depth": 100000 })] {
        let reply = daemon.send("dev_snapshot", args.clone());
        assert!(!reply.success());
        assert!(reply.message().contains("no window"), "{}", args);
    }
    for args in [json!({ "max_edge": 1 }), json!({ "max_edge": 99999 })] {
        let reply = daemon.send("dev_capture", args.clone());
        assert!(!reply.success());
        assert!(reply.message().contains("no window"), "{}", args);
    }
}

// ─── The CLI ──────────────────────────────────────────────────────────────

/// A port nobody is listening on. Bound and released, so the number is real
/// and free rather than assumed.
fn vacant_addr() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
    let addr = listener.local_addr().expect("read the bound port").to_string();
    drop(listener);
    addr
}

fn run_cli(addr: &str, args: &[&str]) -> (bool, Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_unflick"))
        .args(args)
        .env(unflick_lib::core::daemon::CONTROL_ADDR_ENV, addr)
        .output()
        .expect("run unflick");
    let body = if out.status.success() { &out.stdout } else { &out.stderr };
    let value: Value = serde_json::from_slice(body).unwrap_or_else(|e| {
        panic!(
            "`unflick {}` did not print JSON: {}\nstdout: {}\nstderr: {}",
            args.join(" "),
            e,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    });
    (out.status.success(), value)
}

#[test]
fn the_cli_points_at_the_gui_when_nothing_is_running() {
    // `unflick dev` deliberately does not call `ensure_daemon()`. A daemon
    // it spawned would have no window and no flag, and would answer "start
    // unflick with --allow-dev" — sending someone after a flag when their
    // real problem is that no GUI is running.
    let addr = vacant_addr();
    let (ok, value) = run_cli(&addr, &["dev", "snapshot"]);
    assert!(!ok, "dev snapshot succeeded with nothing listening: {}", value);
    let message = value["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("--allow-dev"),
        "the message does not name the flag: {}",
        message
    );
    assert!(
        !message.contains("unflick daemon"),
        "the message points at the headless daemon, which has no window: {}",
        message
    );
}

#[test]
fn the_cli_reaches_the_same_gate_the_control_port_does() {
    let daemon = Daemon::start();
    let (ok, value) = run_cli(daemon.addr(), &["dev", "eval", "1 + 1"]);
    assert!(!ok, "dev eval succeeded against an unarmed daemon: {}", value);
    assert!(
        value["message"]
            .as_str()
            .unwrap_or_default()
            .contains("--allow-dev"),
        "{}",
        value
    );
}

#[test]
fn every_cli_verb_reaches_the_window_check_with_its_defaults() {
    // Drives the real argument parser: a flag renamed here and not in the
    // daemon would show up as an argument error rather than "no window".
    let daemon = Daemon::start_with_dev();
    let invocations: [&[&str]; 8] = [
        &["dev", "eval", "document.title"],
        &["dev", "click", "#play"],
        &["dev", "click", "#play", "--index", "2", "--timeout", "1.5"],
        &["dev", "text", ".track-row"],
        &["dev", "snapshot"],
        &["dev", "snapshot", "--selector", "#panel", "--depth", "3"],
        &["dev", "capture", "--max-edge", "512"],
        &["dev", "wait", "#panel", "--gone", "--timeout", "2"],
    ];
    for args in invocations {
        let (ok, value) = run_cli(daemon.addr(), args);
        assert!(!ok, "`unflick {}` succeeded without a window", args.join(" "));
        assert_eq!(
            value["message"].as_str().unwrap_or_default(),
            "no window — dev commands need the unflick GUI running",
            "`unflick {}` gave the wrong refusal",
            args.join(" ")
        );
    }
}

#[test]
fn allow_dev_is_accepted_wherever_it_is_typed() {
    // `global = true` on the flag is what makes all three orderings work.
    // A user who types it after the subcommand has not made a mistake.
    for args in [
        ["--allow-dev", "--help"],
        ["daemon", "--help"],
        ["dev", "--help"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_unflick"))
            .args(args)
            .output()
            .expect("run unflick --help");
        assert!(out.status.success(), "`unflick {}` failed", args.join(" "));
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.contains("--allow-dev"),
            "`unflick {}` does not offer --allow-dev:\n{}",
            args.join(" "),
            text
        );
    }
}

// ─── MCP ──────────────────────────────────────────────────────────────────

fn tool_named<'a>(tools: &'a Value, name: &str) -> &'a Value {
    tools
        .as_array()
        .expect("tools/list returned no array")
        .iter()
        .find(|t| t["name"] == name)
        .unwrap_or_else(|| panic!("tools/list does not offer {}", name))
}

#[test]
fn mcp_exposes_all_six_dev_tools_with_the_schemas_they_promise() {
    let daemon = Daemon::start();
    let replies = mcp_roundtrip(
        &[json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})],
        &daemon,
    );
    let tools = &replies[&1]["result"]["tools"];

    for name in [
        "dev_eval",
        "dev_click",
        "dev_text",
        "dev_snapshot",
        "dev_capture",
        "dev_wait",
    ] {
        let tool = tool_named(tools, name);
        assert!(
            tool["description"].as_str().unwrap_or_default().len() > 40,
            "{} has no description worth reading",
            name
        );
    }

    assert_eq!(tool_named(tools, "dev_eval")["inputSchema"]["required"], json!(["script"]));
    assert_eq!(tool_named(tools, "dev_click")["inputSchema"]["required"], json!(["selector"]));
    assert_eq!(tool_named(tools, "dev_text")["inputSchema"]["required"], json!(["selector"]));
    assert_eq!(tool_named(tools, "dev_wait")["inputSchema"]["required"], json!(["selector"]));

    // The entry point an agent should reach for first asks for nothing.
    assert!(
        tool_named(tools, "dev_snapshot")["inputSchema"]
            .get("required")
            .is_none(),
        "dev_snapshot should require nothing"
    );

    // Deliberately no `output`: writing files is the CLI's job, the same
    // rule `describe_frame` follows. An agent asking to see the window
    // wants pixels, not a path on someone else's disk.
    let capture = tool_named(tools, "dev_capture");
    assert!(
        capture["inputSchema"]["properties"].get("output").is_none(),
        "dev_capture must not offer `output`: {}",
        capture["inputSchema"]
    );
    assert!(capture["inputSchema"]["properties"]["max_edge"].is_object());
}

#[test]
fn mcp_cannot_walk_around_the_gate_the_cli_goes_through() {
    let daemon = Daemon::start();
    let replies = mcp_roundtrip(
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                   "params": {"name": "dev_eval", "arguments": {"script": "1"}}}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                   "params": {"name": "dev_capture", "arguments": {}}}),
        ],
        &daemon,
    );
    for id in [1, 2] {
        let result = &replies[&id]["result"];
        assert_eq!(result["isError"], json!(true), "reply {}: {}", id, result);
        let text = result["content"][0]["text"].as_str().unwrap_or_default();
        assert!(
            text.contains("--allow-dev"),
            "reply {} does not name the flag: {}",
            id,
            text
        );
    }
}

#[test]
fn mcp_dev_tools_give_the_same_no_window_answer_the_control_port_does() {
    let daemon = Daemon::start_with_dev();
    let replies = mcp_roundtrip(
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                   "params": {"name": "dev_snapshot", "arguments": {}}}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                   "params": {"name": "dev_capture", "arguments": {"max_edge": 400}}}),
        ],
        &daemon,
    );
    for id in [1, 2] {
        let result = &replies[&id]["result"];
        assert_eq!(result["isError"], json!(true), "reply {}: {}", id, result);
        let text = result["content"][0]["text"].as_str().unwrap_or_default();
        assert!(
            text.contains("no window"),
            "reply {} did not say there is no window: {}",
            id,
            text
        );
    }
}
