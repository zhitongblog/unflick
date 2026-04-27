//! Daemon mode: a background process that holds the Player instance.
//! CLI commands connect via a local TCP socket to send commands and receive responses.
//! The MCP server also runs inside the daemon.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

use serde_json::{json, Value};

use super::player::Player;
use super::types::CommandResult;

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
                thread::spawn(move || handle_client(stream, &player));
            }
            Err(e) => {
                eprintln!("connection error: {}", e);
            }
        }
    }

    0
}

fn handle_client(stream: TcpStream, player: &Player) {
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

        let result = dispatch_command(player, cmd, args);
        let json = serde_json::to_string(&result).unwrap();
        let _ = writeln!(writer, "{}", json);
    }
}

fn dispatch_command(player: &Player, cmd: &str, args: &Value) -> CommandResult {
    match cmd {
        "play" => {
            let file = args["file"].as_str().unwrap_or("");
            let seek = args.get("seek").and_then(|v| v.as_f64());
            let volume = args.get("volume").and_then(|v| v.as_i64());
            let speed = args.get("speed").and_then(|v| v.as_f64());
            match player.play(file, seek, volume, speed) {
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
        "stop" => match player.stop() {
            Ok(()) => CommandResult::ok("stopped"),
            Err(e) => CommandResult::err(e.to_string()),
        },
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
