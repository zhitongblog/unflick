//! The tools that read what the player can see and hear: transcript search,
//! chapter synthesis, and frame capture.
//!
//! These exist because they can't be built outside the player 鈥?they work
//! off the subtitle track it already has open and the frame it is showing.
//! That also means they can only be tested this way, end to end.

mod common;

use common::{fixtures, mcp_roundtrip, Daemon};
use serde_json::json;

// 鈹€鈹€鈹€ Transcript 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€

#[test]
fn reads_the_transcript_of_the_playing_file() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);

    let transcript = d.send("transcript_get", json!({})).expect_ok().data();
    let cues = transcript["cues"].as_array().expect("cues array");
    assert_eq!(cues.len(), 12);
    assert_eq!(cues[0]["text"], "Welcome to the show");
    assert!((cues[0]["start"].as_f64().unwrap()).abs() < 1e-6);

    // mpv auto-loads a same-stem sidecar and selects it, so that's the
    // track we should be reading.
    assert!(
        ["selected", "external", "sidecar"]
            .contains(&transcript["origin"].as_str().unwrap()),
        "unexpected transcript origin: {}",
        transcript["origin"]
    );
}

#[test]
fn search_is_case_insensitive_and_respects_the_limit() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);

    let hits = d.send("transcript_search", json!({ "query": "refund" }));
    hits.expect_ok();
    assert_eq!(hits.data()["matches"].as_array().unwrap().len(), 3);

    let upper = d.send("transcript_search", json!({ "query": "REFUND" }));
    upper.expect_ok();
    assert_eq!(upper.data()["matches"].as_array().unwrap().len(), 3);

    let limited = d.send("transcript_search", json!({ "query": "refund", "limit": 1 }));
    limited.expect_ok();
    assert_eq!(limited.data()["matches"].as_array().unwrap().len(), 1);
}

#[test]
fn seek_to_text_lands_just_before_the_line() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);

    // "First up is the refund policy" starts at 18s; we should land a
    // little before it so the sentence is heard from its start.
    let first = d.send("transcript_seek", json!({ "query": "refund" }));
    first.expect_ok();
    let pos = first.data()["position"].as_f64().unwrap();
    assert!(
        (17.0..18.1).contains(&pos),
        "expected to land just before 18s, got {pos}"
    );

    // Occurrences are 1-based and distinct: 18s, 21s, 48s.
    let third = d.send("transcript_seek", json!({ "query": "refund", "occurrence": 3 }));
    third.expect_ok();
    let pos = third.data()["position"].as_f64().unwrap();
    assert!(
        (47.0..48.1).contains(&pos),
        "expected the third match near 48s, got {pos}"
    );
}

#[test]
fn seek_to_text_reports_how_many_matches_exist() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);

    // Asking for an occurrence that doesn't exist should say how many do,
    // rather than just failing.
    d.send("transcript_seek", json!({ "query": "refund", "occurrence": 9 }))
        .expect_err_containing("3 found");

    d.send("transcript_seek", json!({ "query": "banana daiquiri" }))
        .expect_err_containing("no match");
}

#[test]
fn a_file_without_subtitles_says_what_to_do_about_it() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.plain);

    // The error is the whole feature here: an agent that gets "no readable
    // subtitles" needs to know generating them is an option.
    d.send("transcript_get", json!({}))
        .expect_err_containing("subtitle");
}

// 鈹€鈹€鈹€ Chapter synthesis 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€

#[test]
fn generates_chapters_from_transcript_pauses() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);

    assert!(d.send("chapter_list", json!({})).expect_ok().data().as_array().unwrap().is_empty());

    let generated = d.send("chapters_generate", json!({ "count": 4 }));
    generated.expect_ok();
    let list = generated.data();
    let list = list.as_array().unwrap();
    assert!(
        (2..=4).contains(&list.len()),
        "expected 2-4 chapters, got {}",
        list.len()
    );
    assert_eq!(list[0]["time"].as_f64().unwrap(), 0.0, "first chapter starts at 0");
    for pair in list.windows(2) {
        assert!(
            pair[1]["time"].as_f64().unwrap() > pair[0]["time"].as_f64().unwrap(),
            "chapters must be strictly ordered"
        );
    }
}

#[test]
fn generated_chapters_become_real_navigation() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);
    d.send("chapters_generate", json!({ "count": 4 })).expect_ok();

    let list = d.send("chapter_list", json!({})).expect_ok().data();
    let target_time = list.as_array().unwrap()[1]["time"].as_f64().unwrap();

    // The point of synthesising them: seeking works, on a file that shipped
    // without any chapters at all.
    d.send("chapter_seek", json!({ "index": 1 })).expect_ok();
    d.wait_for(
        |d| (d.position() - target_time).abs() < 2.0,
        "seek to a generated chapter",
    );
}

#[test]
fn explicit_chapters_are_sorted_and_clamped() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);

    let reply = d.send(
        "chapters_set",
        json!({ "chapters": [
            { "time": 20, "title": "Third" },
            { "time": 0,  "title": "First" },
            { "time": 10, "title": "Second" },
            { "time": 99999, "title": "Past the end" }
        ]}),
    );
    reply.expect_ok();
    let list = reply.data();
    let list = list.as_array().unwrap();

    assert_eq!(list.len(), 3, "entries past the end of the file are dropped");
    let times: Vec<f64> = list.iter().map(|c| c["time"].as_f64().unwrap()).collect();
    assert_eq!(times, vec![0.0, 10.0, 20.0], "input order should not matter");
    assert_eq!(list[0]["title"], "First");
}

#[test]
fn chapters_set_refuses_to_override_a_files_own_chapters() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_chapters);

    d.send("chapters_set", json!({ "chapters": [{ "time": 0, "title": "Nope" }] }))
        .expect_err_containing("own chapters");

    // The container's chapters are untouched.
    let list = d.send("chapter_list", json!({})).expect_ok().data();
    assert_eq!(list.as_array().unwrap()[0]["title"], "Opening");
}

#[test]
fn clearing_removes_only_generated_chapters() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);

    d.send("chapters_generate", json!({ "count": 4 })).expect_ok();
    d.send("chapters_clear", json!({})).expect_ok();
    assert!(d
        .send("chapter_list", json!({}))
        .expect_ok()
        .data()
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn generated_chapters_do_not_survive_a_file_change() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);
    d.send("chapters_generate", json!({ "count": 4 })).expect_ok();

    // They describe one specific recording; carrying them over would put
    // the wrong marks on another file's timeline.
    d.play(&f.plain);
    assert!(d
        .send("chapter_list", json!({}))
        .expect_ok()
        .data()
        .as_array()
        .unwrap()
        .is_empty());
}

// 鈹€鈹€鈹€ Frame capture 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€

#[test]
fn captures_a_frame_as_a_jpeg_file() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);

    let out = d.data_dir().join("frame.jpg");
    let reply = d.send(
        "describe_frame",
        json!({ "output": out.to_string_lossy(), "position": 12.0 }),
    );
    reply.expect_ok();

    let bytes = std::fs::read(&out).expect("captured frame file");
    assert!(bytes.len() > 500, "frame is suspiciously small: {} bytes", bytes.len());
    assert_eq!(&bytes[..3], &[0xff, 0xd8, 0xff], "output is not a JPEG");
}

#[test]
fn captures_a_frame_as_base64_when_no_path_is_given() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);

    let reply = d.send("describe_frame", json!({ "max_edge": 128 }));
    reply.expect_ok();
    let data = reply.data();
    let encoded = data["base64"].as_str().expect("base64 payload");
    assert!(!encoded.is_empty());
    // JPEG's SOI marker is 0xFFD8FF, which base64-encodes to a leading "/9j/".
    assert!(
        encoded.starts_with("/9j/"),
        "payload does not look like a JPEG: {}",
        &encoded[..encoded.len().min(16)]
    );
}

#[test]
fn frame_capture_reports_a_clear_error_with_nothing_playing() {
    let d = Daemon::start();
    d.send("describe_frame", json!({}))
        .expect_err_containing("nothing is playing");
}

// 鈹€鈹€鈹€ MCP surface 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€

// --- Online subtitles (OpenSubtitles) -------------------------------------
//
// The network path can't be exercised here: it needs the user's own API key
// and every call burns a unit of their daily download quota. What is worth
// testing is everything that happens on *this* side of the request - the
// defaults, the refusals, and the fact that a missing key produces an
// instruction rather than a stack trace. Each test daemon gets its own
// config dir, so no key is ever configured and that path is the one we hit.

#[test]
fn subtitle_search_without_a_key_says_how_to_get_one() {
    let d = Daemon::start();
    let reply = d.send("subtitle_search", json!({ "query": "The Matrix" }));

    reply.expect_err_containing("opensubtitles.com/consumers");
    reply.expect_err_containing("settings set opensubtitles_api_key");
}

#[test]
fn subtitle_auto_without_a_key_fails_the_same_way() {
    // The one-step form must not swallow the setup instruction on its way
    // through the search it wraps.
    let d = Daemon::start();
    d.send("subtitle_auto", json!({ "query": "Arrival" }))
        .expect_err_containing("api key not set");
}

#[test]
fn subtitle_search_with_nothing_playing_and_no_query_refuses_early() {
    // No file, no query: there is nothing to search for, and saying so beats
    // sending an empty query the API would reject with a 400.
    let d = Daemon::start();
    d.send("subtitle_search", json!({}))
        .expect_err_containing("nothing to search for");
}

#[test]
fn subtitle_search_derives_a_query_from_the_playing_file() {
    // Reaches the key check, which means the query and language defaults
    // were resolved first - if they hadn't been, this would fail with
    // "nothing to search for" instead.
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.plain);

    d.send("subtitle_search", json!({}))
        .expect_err_containing("api key not set");
}

#[test]
fn subtitle_search_rejects_an_empty_language_list() {
    let d = Daemon::start();
    d.send(
        "subtitle_search",
        json!({ "query": "x", "languages": " , , " }),
    )
    .expect_err_containing("languages list is empty");
}

#[test]
fn subtitle_download_requires_a_file_id() {
    let d = Daemon::start();
    d.send("subtitle_download", json!({}))
        .expect_err_containing("file_id required");
}

#[test]
fn subtitle_download_with_a_file_id_but_no_key_still_explains_itself() {
    let d = Daemon::start();
    d.send("subtitle_download", json!({ "file_id": 12345 }))
        .expect_err_containing("api key not set");
}

#[test]
fn mcp_exposes_the_online_subtitle_tools_with_usable_schemas() {
    let d = Daemon::start();
    let replies = mcp_roundtrip(
        &[json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/list", "params": {} })],
        &d,
    );
    let tools = replies[&7]["result"]["tools"]
        .as_array()
        .expect("tool list")
        .clone();

    let find = |name: &str| {
        tools
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("MCP is missing `{name}`"))
            .clone()
    };

    // download_subtitle is useless without an id, so the schema has to say
    // so - an agent reading the listing is the only thing that knows.
    let download = find("download_subtitle");
    assert_eq!(download["inputSchema"]["required"][0], "file_id");

    // The two that can run with no arguments must not demand any, or an
    // agent will refuse to call them for the playing file.
    for name in ["get_subtitles", "find_subtitles"] {
        let tool = find(name);
        assert!(
            tool["inputSchema"].get("required").is_none(),
            "`{name}` should be callable with no arguments"
        );
    }

    // Quota is the thing an agent most needs warning about: these calls
    // spend a limited daily allowance that belongs to the user.
    for name in ["get_subtitles", "download_subtitle"] {
        let desc = find(name)["description"].as_str().unwrap_or("").to_lowercase();
        assert!(
            desc.contains("daily") || desc.contains("quota"),
            "`{name}` should warn that downloads are limited"
        );
    }
}

#[test]
fn mcp_get_subtitles_reports_the_missing_key_as_an_error() {
    let d = Daemon::start();
    let replies = mcp_roundtrip(
        &[json!({
            "jsonrpc": "2.0", "id": 8, "method": "tools/call",
            "params": { "name": "get_subtitles", "arguments": { "query": "Dune" } }
        })],
        &d,
    );

    let result = &replies[&8]["result"];
    assert_eq!(result["isError"], true, "expected an error result: {result}");
    let text = result["content"][0]["text"].as_str().unwrap_or("");
    assert!(
        text.contains("opensubtitles.com/consumers"),
        "the agent should be told how to fix this, got: {text}"
    );
}

#[test]
fn mcp_exposes_the_understanding_tools() {
    let d = Daemon::start();
    let replies = mcp_roundtrip(
        &[json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} })],
        &d,
    );

    let listing = &replies[&2];
    let tools = listing["result"]["tools"].as_array().expect("tool list");
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();

    for expected in [
        "play", "pause", "seek",
        "subtitle_delay", "audio_delay", "chapter_list", "chapter_seek",
        "ab_loop", "frame_step", "playlist_repeat", "playlist_shuffle",
        "search_transcript", "seek_to_text", "transcript_get",
        "transcript_summary", "library_transcript_search",
        "script_list", "script_add", "script_toggle", "script_remove",
        "generate_chapters", "set_chapters", "clear_chapters", "describe_frame",
        "get_subtitles", "find_subtitles", "download_subtitle",
    ] {
        assert!(names.contains(&expected), "MCP is missing the `{expected}` tool");
    }

    // Every tool needs a schema; a missing one makes the tool uncallable
    // even though it shows up in the listing.
    for tool in tools {
        assert!(
            tool["inputSchema"]["type"] == "object",
            "tool {} has no object inputSchema",
            tool["name"]
        );
        // Not a length check 鈥?"Pause playback" is a fine description for a
        // tool that pauses playback. What matters is that every tool has
        // one, so nothing ships as an unexplained name in the listing.
        assert!(
            tool["description"]
                .as_str()
                .map(|d| !d.trim().is_empty())
                .unwrap_or(false),
            "tool {} has no description",
            tool["name"]
        );
    }
}

#[test]
fn mcp_returns_a_captured_frame_as_an_image_block() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);

    let replies = mcp_roundtrip(
        &[json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {
                "name": "describe_frame",
                "arguments": { "position": 30, "max_edge": 320 }
            }
        })],
        &d,
    );

    let content = replies[&3]["result"]["content"]
        .as_array()
        .expect("tool result content");
    let kinds: Vec<&str> = content.iter().filter_map(|c| c["type"].as_str()).collect();
    assert!(
        kinds.contains(&"image"),
        "describe_frame must return an image block, got {kinds:?}"
    );

    let image = content.iter().find(|c| c["type"] == "image").unwrap();
    assert_eq!(image["mimeType"], "image/jpeg");
    assert!(image["data"].as_str().unwrap().starts_with("/9j/"));
}

#[test]
fn mcp_search_transcript_reaches_the_same_player() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);

    let replies = mcp_roundtrip(
        &[json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": { "name": "search_transcript", "arguments": { "query": "shipping" } }
        })],
        &d,
    );

    let text = replies[&4]["result"]["content"][0]["text"]
        .as_str()
        .expect("text result");
    assert!(
        text.contains("shipping"),
        "MCP search did not reach the playing file's subtitles: {text}"
    );
}

// ─── v1.0: Transcript Summary, Cross-Library Transcript Search, Scripts & HTTP Bridge ──

#[test]
fn transcript_summary_groups_cues_and_extracts_keywords() {
    let f = fixtures();
    let d = Daemon::start();
    d.play(&f.with_subtitles);

    let summary = d
        .send("transcript_summary", json!({ "segments": 3 }))
        .expect_ok()
        .data();
    assert_eq!(summary["cue_count"], 12);
    assert!(summary["word_count"].as_u64().unwrap_or(0) >= 30);
    assert_eq!(summary["segments"].as_array().unwrap().len(), 3);
    let keywords: Vec<&str> = summary["keywords"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        keywords.contains(&"refund"),
        "expected 'refund' in keywords, got {keywords:?}"
    );
}

#[test]
fn library_transcript_search_finds_lines_across_scanned_entries() {
    let f = fixtures();
    let d = Daemon::start();

    let scan_dir = f.with_subtitles.parent().unwrap().to_string_lossy().to_string();
    d.send("library_scan", json!({ "dir": scan_dir })).expect_ok();

    let res = d
        .send("library_transcript_search", json!({ "query": "refund", "limit": 10 }))
        .expect_ok()
        .data();
    let matches = res["matches"].as_array().expect("matches array");
    assert_eq!(matches.len(), 3);
    assert!(matches[0]["cue"]["text"]
        .as_str()
        .unwrap_or("")
        .to_lowercase()
        .contains("refund"));
}

#[test]
fn user_scripts_can_be_installed_toggled_and_removed() {
    let d = Daemon::start();

    let script_src = d.data_dir().join("autocrop.lua");
    std::fs::write(
        &script_src,
        "-- Automatically crop black bars\nmp.msg.info('autocrop loaded')\n",
    )
    .expect("write test lua script");

    let added = d
        .send("script_add", json!({ "path": script_src.to_string_lossy() }))
        .expect_ok()
        .data();
    assert_eq!(added["name"], "autocrop.lua");
    assert_eq!(added["enabled"], true);
    assert_eq!(added["description"], "Automatically crop black bars");

    let disabled = d
        .send("script_disable", json!({ "name": "autocrop.lua" }))
        .expect_ok()
        .data();
    assert_eq!(disabled["enabled"], false);

    let enabled = d
        .send("script_enable", json!({ "name": "autocrop.lua" }))
        .expect_ok()
        .data();
    assert_eq!(enabled["enabled"], true);

    let list = d.send("script_list", json!({})).expect_ok().data();
    assert_eq!(list["scripts"].as_array().unwrap().len(), 1);

    d.send("script_remove", json!({ "name": "autocrop.lua" }))
        .expect_ok();
    let after = d.send("script_list", json!({})).expect_ok().data();
    assert!(after["scripts"].as_array().unwrap().is_empty());
}

#[test]
fn control_socket_speaks_http_for_the_browser_extension() {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let f = fixtures();
    let d = Daemon::start();

    // 1. OPTIONS preflight returns CORS + Private Network Access headers.
    {
        let mut s = TcpStream::connect(d.addr()).expect("connect for OPTIONS");
        write!(
            s,
            "OPTIONS /open HTTP/1.1\r\nHost: {}\r\nOrigin: chrome-extension://abc\r\n\r\n",
            d.addr()
        )
        .unwrap();
        let mut raw = String::new();
        s.read_to_string(&mut raw).unwrap();
        assert!(raw.starts_with("HTTP/1.1 204"), "unexpected OPTIONS: {raw}");
        assert!(raw.contains("Access-Control-Allow-Private-Network: true"));
    }

    // 2. POST /open with action="queue" appends to playlist.
    {
        let body = serde_json::to_string(&json!({
            "url": f.plain.to_string_lossy(),
            "action": "queue"
        }))
        .unwrap();
        let mut s = TcpStream::connect(d.addr()).expect("connect for POST /open");
        write!(
            s,
            "POST /open HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            d.addr(),
            body.len(),
            body
        )
        .unwrap();
        let mut raw = String::new();
        s.read_to_string(&mut raw).unwrap();
        assert!(raw.starts_with("HTTP/1.1 200"), "unexpected POST /open: {raw}");
        let json_part = raw.split("\r\n\r\n").nth(1).unwrap_or("");
        let parsed: serde_json::Value = serde_json::from_str(json_part).expect("valid JSON");
        assert_eq!(parsed["success"], true);
        assert_eq!(parsed["data"].as_array().unwrap().len(), 1);
    }

    // 3. GET /status returns player status over HTTP.
    {
        let mut s = TcpStream::connect(d.addr()).expect("connect for GET /status");
        write!(s, "GET /status HTTP/1.1\r\nHost: {}\r\n\r\n", d.addr()).unwrap();
        let mut raw = String::new();
        s.read_to_string(&mut raw).unwrap();
        assert!(raw.starts_with("HTTP/1.1 200"), "unexpected GET /status: {raw}");
        let json_part = raw.split("\r\n\r\n").nth(1).unwrap_or("");
        let parsed: serde_json::Value = serde_json::from_str(json_part).expect("valid JSON");
        assert_eq!(parsed["success"], true);
    }
}

