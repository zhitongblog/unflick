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

            // Save position of current file before switching
            let current_status = player.status();
            if let Some(ref current_file) = current_status.file {
                if current_status.position > 1.0 {
                    let _ = db.save_position(current_file, current_status.position);
                }
            }

            // Check for saved position if no explicit seek
            let effective_seek = seek.or_else(|| {
                db.get_position(file).ok().flatten()
            });

            match player.play(file, effective_seek, volume, speed) {
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
            // Use a separate mpv instance to probe file info without disrupting playback
            match Player::new() {
                Ok(probe) => {
                    match probe.play(file, None, None, None) {
                        Ok(()) => {
                            std::thread::sleep(std::time::Duration::from_millis(800));
                            let status = probe.status();
                            let width = probe.get_property_i64("width").ok();
                            let height = probe.get_property_i64("height").ok();
                            let video_codec = probe.get_property_string("video-codec").ok();
                            let audio_codec = probe.get_property_string("audio-codec").ok();
                            CommandResult::ok_with_data(
                                "ok",
                                json!({
                                    "path": file,
                                    "duration": status.duration,
                                    "width": width,
                                    "height": height,
                                    "video_codec": video_codec,
                                    "audio_codec": audio_codec,
                                }),
                            )
                        }
                        Err(e) => CommandResult::err(e.to_string()),
                    }
                }
                Err(e) => CommandResult::err(format!("failed to create probe: {}", e)),
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
