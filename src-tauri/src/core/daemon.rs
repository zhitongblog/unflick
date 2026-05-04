//! Daemon mode: a background process that holds the Player instance.
//! CLI commands connect via a local TCP socket to send commands and receive responses.
//! The MCP server also runs inside the daemon.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

use serde_json::{json, Value};

use super::player::Player;
use super::playlist::Playlist;
use super::types::CommandResult;
use crate::db::Database;

const DAEMON_ADDR: &str = "127.0.0.1:19542";

/// Start the daemon: create a Player and listen for commands over TCP.
pub fn start_daemon() -> i32 {
    let player = match Player::new() {
        Ok(p) => Arc::new(p),
        Err(e) => {
            eprintln!("failed to initialize player: {}", e);
            return 1;
        }
    };

    let playlist = Arc::new(Playlist::new());

    let db = match Database::open() {
        Ok(d) => Arc::new(d),
        Err(e) => {
            eprintln!("failed to open database: {}", e);
            return 1;
        }
    };

    let listener = match TcpListener::bind(DAEMON_ADDR) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("failed to bind {}: {}", DAEMON_ADDR, e);
            return 1;
        }
    };

    eprintln!("unflick daemon listening on {}", DAEMON_ADDR);

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let player = Arc::clone(&player);
                let playlist = Arc::clone(&playlist);
                let db = Arc::clone(&db);
                thread::spawn(move || handle_client(stream, &player, &playlist, &db));
            }
            Err(e) => {
                eprintln!("connection error: {}", e);
            }
        }
    }

    0
}

fn handle_client(stream: TcpStream, player: &Player, playlist: &Playlist, db: &Database) {
    let reader = BufReader::new(stream.try_clone().unwrap());
    let mut writer = stream;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }

        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let resp = json!({"success": false, "message": format!("parse error: {}", e)});
                let _ = writeln!(writer, "{}", resp);
                continue;
            }
        };

        let cmd = request["command"].as_str().unwrap_or("");
        let args = &request["args"];

        let result = dispatch_command(player, playlist, db, cmd, args);
        let json = serde_json::to_string(&result).unwrap();
        let _ = writeln!(writer, "{}", json);
    }
}

fn dispatch_command(player: &Player, playlist: &Playlist, db: &Database, cmd: &str, args: &Value) -> CommandResult {
    match cmd {
        "play" => {
            let file = args["file"].as_str().unwrap_or("");
            let seek = args.get("seek").and_then(|v| v.as_f64());
            let volume = args.get("volume").and_then(|v| v.as_i64());
            let speed = args.get("speed").and_then(|v| v.as_f64());
            let proxy = args.get("proxy").and_then(|v| v.as_str()).map(String::from);

            // Save position of current file before switching
            let current_status = player.status();
            if let Some(ref current_file) = current_status.file {
                if current_status.position > 1.0 {
                    let _ = db.save_position(current_file, current_status.position);
                }
            }

            // If the input is an http(s) URL, resolve it through yt-dlp
            // before handing it to mpv. mpv can play HLS / direct-MP4 URLs
            // natively, but YouTube/Bilibili/etc. need an extraction step.
            // We only extract for URLs — local paths (and previously
            // resolved direct stream URLs from the GUI) hit mpv directly.
            let resolved = if crate::core::yt_dlp::is_http_url(file) {
                let yt_dlp = match crate::core::yt_dlp::find_yt_dlp() {
                    Some(p) => p,
                    None => return CommandResult::err(
                        "yt-dlp not found. Place yt-dlp(.exe) next to unflick or on PATH.",
                    ),
                };
                let url_owned = file.to_string();
                let proxy_owned = proxy.clone();
                // Build an ad-hoc tokio runtime: the daemon is otherwise
                // pure-sync and we don't want to convert the whole listener
                // for one command. The runtime is dropped at the end of
                // the call so it has zero ongoing cost.
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(r) => r,
                    Err(e) => return CommandResult::err(format!("tokio runtime: {}", e)),
                };
                let result = rt.block_on(crate::core::yt_dlp::extract_stream_url(
                    &yt_dlp,
                    &url_owned,
                    proxy_owned.as_deref(),
                ));
                if !result.is_ok() {
                    let kind = result
                        .error_kind
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string());
                    let msg = result
                        .error_message
                        .clone()
                        .unwrap_or_else(|| "yt-dlp failed".to_string());
                    let mut err = CommandResult::err(msg);
                    err.data = Some(json!({"error_kind": kind}));
                    return err;
                }
                result.stream_url
            } else {
                file.to_string()
            };

            // Check for saved position if no explicit seek (use the
            // *original* input as the key — saved positions are keyed by
            // the user-facing path/URL, not the resolved CDN URL which
            // changes between sessions).
            let effective_seek = seek.or_else(|| {
                db.get_position(file).ok().flatten()
            });

            match player.play(&resolved, effective_seek, volume, speed) {
                Ok(()) => CommandResult::ok(format!("playing {}", file)),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "pause" => match player.pause() {
            Ok(()) => CommandResult::ok("paused"),
            Err(e) => CommandResult::err(e.to_string()),
        },
        "resume" => match player.resume() {
            Ok(()) => CommandResult::ok("resumed"),
            Err(e) => CommandResult::err(e.to_string()),
        },
        "stop" => {
            let status = player.status();
            if let Some(ref file) = status.file {
                if status.position > 1.0 {
                    let _ = db.save_position(file, status.position);
                }
            }
            match player.stop() {
                Ok(()) => CommandResult::ok("stopped"),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "seek" => {
            let seconds = args["seconds"].as_f64().unwrap_or(0.0);
            match player.seek(seconds) {
                Ok(()) => CommandResult::ok(format!("seeked to {}s", seconds)),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "volume" => {
            let level = args["level"].as_i64().unwrap_or(100);
            match player.set_volume(level) {
                Ok(()) => CommandResult::ok(format!("volume set to {}", level)),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "speed" => {
            let rate = args["rate"].as_f64().unwrap_or(1.0);
            match player.set_speed(rate) {
                Ok(()) => CommandResult::ok(format!("speed set to {}x", rate)),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "status" => {
            let status = player.status();
            CommandResult::ok_with_data("ok", serde_json::to_value(&status).unwrap())
        }
        "info" => {
            let file = args["file"].as_str().unwrap_or("");
            if file.is_empty() {
                return CommandResult::err("file path required");
            }
            match crate::core::player::probe_file(file) {
                Ok(info) => CommandResult::ok_with_data("ok", serde_json::to_value(&info).unwrap()),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "playlist_add" => {
            let file = args["file"].as_str().unwrap_or("");
            if file.is_empty() {
                return CommandResult::err("file path required");
            }
            playlist.add(file);
            let entries = playlist.list();
            CommandResult::ok_with_data(
                format!("added {}", file),
                serde_json::to_value(&entries).unwrap(),
            )
        }
        "playlist_remove" => {
            let index = args["index"].as_u64().unwrap_or(0) as usize;
            match playlist.remove(index) {
                Ok(()) => {
                    let entries = playlist.list();
                    CommandResult::ok_with_data("removed", serde_json::to_value(&entries).unwrap())
                }
                Err(e) => CommandResult::err(e),
            }
        }
        "playlist_list" => {
            let entries = playlist.list();
            CommandResult::ok_with_data("ok", serde_json::to_value(&entries).unwrap())
        }
        "playlist_next" => {
            match playlist.next() {
                Some(path) => {
                    match player.play(&path, None, None, None) {
                        Ok(()) => CommandResult::ok(format!("playing next: {}", path)),
                        Err(e) => CommandResult::err(e.to_string()),
                    }
                }
                None => CommandResult::err("no next track"),
            }
        }
        "playlist_prev" => {
            match playlist.prev() {
                Some(path) => {
                    match player.play(&path, None, None, None) {
                        Ok(()) => CommandResult::ok(format!("playing previous: {}", path)),
                        Err(e) => CommandResult::err(e.to_string()),
                    }
                }
                None => CommandResult::err("no previous track"),
            }
        }
        "playlist_clear" => {
            playlist.clear();
            CommandResult::ok("playlist cleared")
        }
        "playlist_play" => {
            let index = args["index"].as_u64().unwrap_or(0) as usize;
            match playlist.set_current(index) {
                Ok(path) => {
                    match player.play(&path, None, None, None) {
                        Ok(()) => CommandResult::ok(format!("playing index {}: {}", index, path)),
                        Err(e) => CommandResult::err(e.to_string()),
                    }
                }
                Err(e) => CommandResult::err(e),
            }
        }
        "subtitle_load" => {
            let file = args["file"].as_str().unwrap_or("");
            match player.subtitle_load(file) {
                Ok(()) => CommandResult::ok(format!("loaded subtitle: {}", file)),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "subtitle_list" => {
            let tracks = player.subtitle_list();
            CommandResult::ok_with_data("ok", serde_json::to_value(&tracks).unwrap())
        }
        "subtitle_select" => {
            let id = args["id"].as_i64().unwrap_or(0);
            match player.subtitle_select(id) {
                Ok(()) => CommandResult::ok(format!("selected subtitle track {}", id)),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "audio_list" => {
            let tracks = player.audio_list();
            CommandResult::ok_with_data("ok", serde_json::to_value(&tracks).unwrap())
        }
        "audio_select" => {
            let id = args["id"].as_i64().unwrap_or(0);
            match player.audio_select(id) {
                Ok(()) => CommandResult::ok(format!("selected audio track {}", id)),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "subtitle_generate" => {
            let video = args["video"].as_str().unwrap_or("");
            if video.is_empty() {
                return CommandResult::err("video path required");
            }
            let api_key = args.get("api_key").and_then(|v| v.as_str()).map(String::from);
            let mode = args.get("mode").and_then(|v| v.as_str()).map(String::from)
                .unwrap_or_else(|| if api_key.is_some() { "api".into() } else { "local".into() });

            let output_dir = args.get("output_dir").and_then(|v| v.as_str()).map(String::from)
                .unwrap_or_else(default_subtitle_output_dir);
            if let Err(e) = std::fs::create_dir_all(&output_dir) {
                return CommandResult::err(format!("failed to create output dir: {}", e));
            }

            let ffmpeg = match crate::core::player::find_ffmpeg() {
                Some(p) => p.to_string_lossy().to_string(),
                None => return CommandResult::err("ffmpeg not found"),
            };

            match mode.as_str() {
                "local" => {
                    let (whisper_bin, model_path) = match (
                        args.get("whisper").and_then(|v| v.as_str()),
                        args.get("model").and_then(|v| v.as_str()),
                    ) {
                        (Some(w), Some(m)) => (w.to_string(), m.to_string()),
                        _ => match crate::core::whisper::find_bundled_whisper() {
                            Some((w, m)) => (w.to_string_lossy().into_owned(), m.to_string_lossy().into_owned()),
                            None => return CommandResult::err(
                                "local mode requires --whisper and --model, or a bundled whisper installation"
                            ),
                        },
                    };
                    match crate::core::whisper::transcribe_local(video, &whisper_bin, &model_path, &output_dir, &ffmpeg) {
                        Ok(srt) => CommandResult::ok_with_data("subtitles generated", json!({"srt_path": srt})),
                        Err(e) => CommandResult::err(e.to_string()),
                    }
                }
                "api" => {
                    let key = match api_key {
                        Some(k) => k,
                        None => return CommandResult::err("api mode requires --api-key"),
                    };
                    match crate::core::whisper::transcribe_api(video, &key, &output_dir, &ffmpeg) {
                        Ok(srt) => CommandResult::ok_with_data("subtitles generated", json!({"srt_path": srt})),
                        Err(e) => CommandResult::err(e.to_string()),
                    }
                }
                other => CommandResult::err(format!("unknown mode: {} (expected 'local' or 'api')", other)),
            }
        }
        "subtitle_translate" => {
            let srt = args["srt"].as_str().unwrap_or("");
            let target_lang = args["target_lang"].as_str().unwrap_or("");
            let api_key = args["api_key"].as_str().unwrap_or("");
            if srt.is_empty() || target_lang.is_empty() || api_key.is_empty() {
                return CommandResult::err("srt, target_lang, and api_key are required");
            }
            let output_dir = args.get("output_dir").and_then(|v| v.as_str()).map(String::from)
                .unwrap_or_else(default_subtitle_output_dir);
            if let Err(e) = std::fs::create_dir_all(&output_dir) {
                return CommandResult::err(format!("failed to create output dir: {}", e));
            }
            match crate::core::whisper::translate_srt(srt, target_lang, api_key, &output_dir) {
                Ok(path) => CommandResult::ok_with_data("translated", json!({"srt_path": path})),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "library_scan" => {
            let dir = args["dir"].as_str().unwrap_or("");
            match crate::core::library::scan_directory(db, dir) {
                Ok(entries) => CommandResult::ok_with_data(
                    format!("scanned {} files", entries.len()),
                    serde_json::to_value(&entries).unwrap(),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "library_search" => {
            let query = args["query"].as_str().unwrap_or("");
            match db.search(query) {
                Ok(entries) => CommandResult::ok_with_data(
                    "ok",
                    serde_json::to_value(&entries).unwrap(),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "library_list" => {
            match db.list_all() {
                Ok(entries) => CommandResult::ok_with_data(
                    "ok",
                    serde_json::to_value(&entries).unwrap(),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "library_remove" => {
            let id = args["id"].as_i64().unwrap_or(0);
            match db.remove(id) {
                Ok(()) => CommandResult::ok("removed"),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "clip" => {
            let file = args["file"].as_str().unwrap_or("");
            let input = if file.is_empty() {
                // Use currently playing file
                match player.status().file {
                    Some(f) => f,
                    None => return CommandResult::err("no file specified and nothing playing"),
                }
            } else {
                file.to_string()
            };
            let start = args["start"].as_f64().unwrap_or(0.0);
            let end = args["end"].as_f64().unwrap_or(0.0);
            let output = args.get("output").and_then(|v| v.as_str()).unwrap_or("");
            let gif = args.get("gif").and_then(|v| v.as_bool()).unwrap_or(false);

            let ffmpeg = match crate::core::player::find_ffmpeg() {
                Some(p) => p.to_string_lossy().to_string(),
                None => return CommandResult::err("ffmpeg not found".to_string()),
            };
            match crate::core::player::extract_clip(&input, start, end, output, gif, &ffmpeg) {
                Ok(path) => CommandResult::ok_with_data("clip saved", json!({"path": path})),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "screenshot" => {
            let output = args.get("output").and_then(|v| v.as_str()).map(String::from).unwrap_or_else(|| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                format!("unflick-screenshot-{}.png", ts)
            });
            match player.screenshot(&output) {
                Ok(()) => CommandResult::ok_with_data("screenshot saved", json!({"path": output})),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "save_position" => {
            let path = args["path"].as_str().unwrap_or("");
            let position = args["position"].as_f64().unwrap_or(0.0);
            match db.save_position(path, position) {
                Ok(()) => CommandResult::ok("position saved"),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "get_position" => {
            let path = args["path"].as_str().unwrap_or("");
            match db.get_position(path) {
                Ok(pos) => CommandResult::ok_with_data("ok", json!({"position": pos})),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "settings_path" => {
            CommandResult::ok_with_data(
                "ok",
                json!({"path": crate::core::settings::settings_path().to_string_lossy()}),
            )
        }
        "settings_get" => match crate::core::settings::read_all() {
            Ok(all) => match args.get("key").and_then(|v| v.as_str()) {
                Some(k) => match all.get(k) {
                    Some(v) => CommandResult::ok_with_data("ok", json!({"key": k, "value": v})),
                    None => CommandResult::err(format!("key not found: {}", k)),
                },
                None => CommandResult::ok_with_data("ok", all),
            },
            Err(e) => CommandResult::err(e.to_string()),
        },
        "settings_set" => {
            let key = args["key"].as_str().unwrap_or("");
            if key.is_empty() {
                return CommandResult::err("key is required");
            }
            let value = args.get("value").cloned().unwrap_or(Value::Null);
            match crate::core::settings::set(key, value.clone()) {
                Ok(()) => CommandResult::ok_with_data(
                    format!("set {}", key),
                    json!({"key": key, "value": value}),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "settings_unset" => {
            let key = args["key"].as_str().unwrap_or("");
            if key.is_empty() {
                return CommandResult::err("key is required");
            }
            match crate::core::settings::unset(key) {
                Ok(true) => CommandResult::ok(format!("removed {}", key)),
                Ok(false) => CommandResult::err(format!("key not found: {}", key)),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "filter_list" => {
            let mut out = serde_json::Map::new();
            for prop in ["brightness", "contrast", "saturation", "gamma", "hue"] {
                let value = player.get_property_i64(prop).unwrap_or(0);
                out.insert(prop.to_string(), json!(value));
            }
            CommandResult::ok_with_data("ok", Value::Object(out))
        }
        "filter_set" => {
            let name = args["name"].as_str().unwrap_or("");
            if !matches!(name, "brightness" | "contrast" | "saturation" | "gamma" | "hue") {
                return CommandResult::err(format!(
                    "unknown filter: {} (expected brightness | contrast | saturation | gamma | hue)",
                    name
                ));
            }
            let value = args["value"].as_i64().unwrap_or(0).clamp(-100, 100);
            match player.set_property_i64(name, value) {
                Ok(()) => CommandResult::ok_with_data(
                    format!("{} = {}", name, value),
                    json!({"name": name, "value": value}),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "filter_reset" => {
            for prop in ["brightness", "contrast", "saturation", "gamma", "hue"] {
                let _ = player.set_property_i64(prop, 0);
            }
            CommandResult::ok("filters reset")
        }
        "shutdown" => {
            std::process::exit(0);
        }
        _ => CommandResult::err(format!("unknown command: {}", cmd)),
    }
}

/// Send a command to the running daemon. Returns the response.
pub fn send_to_daemon(cmd: &str, args: Value) -> Result<CommandResult, String> {
    let mut stream = TcpStream::connect(DAEMON_ADDR)
        .map_err(|_| "daemon not running. Start it with: unflick daemon".to_string())?;

    let request = json!({ "command": cmd, "args": args });
    writeln!(stream, "{}", request).map_err(|e| e.to_string())?;

    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response).map_err(|e| e.to_string())?;

    serde_json::from_str(&response).map_err(|e| format!("invalid response: {}", e))
}

/// Check if daemon is already running.
pub fn is_daemon_running() -> bool {
    TcpStream::connect(DAEMON_ADDR).is_ok()
}

/// Default output directory for AI-generated subtitle files (matches GUI behavior).
fn default_subtitle_output_dir() -> String {
    dirs_next::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("unflick")
        .to_string_lossy()
        .into_owned()
}
