use serde_json::json;
use tauri::command;

use crate::core::daemon;

/// Public entry point for setup hook — starts daemon in background.
pub fn ensure_daemon_startup() {
    ensure_daemon();
}

/// Start daemon in background if not already running.
fn ensure_daemon() {
    if daemon::is_daemon_running() {
        return;
    }

    // Spawn daemon as a detached child process
    let exe = std::env::current_exe().unwrap();
    let _ = std::process::Command::new(exe)
        .arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    // Wait for daemon to be ready (up to 2 seconds)
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if daemon::is_daemon_running() {
            return;
        }
    }
}

/// Send a command to the daemon and return the result as a JSON value.
/// On success (result.success == true), returns Ok with the data or message.
/// On failure, returns Err with the error message.
fn send(cmd: &str, args: serde_json::Value) -> Result<serde_json::Value, String> {
    ensure_daemon();
    let result = daemon::send_to_daemon(cmd, args).map_err(|e| e.to_string())?;
    if result.success {
        // Return data if present, otherwise wrap the message
        Ok(result.data.unwrap_or_else(|| json!({ "message": result.message })))
    } else {
        Err(result.message)
    }
}

#[command]
pub fn player_play(
    file: String,
    seek: Option<f64>,
    volume: Option<i64>,
    speed: Option<f64>,
) -> Result<serde_json::Value, String> {
    let mut args = json!({"file": file});
    if let Some(s) = seek {
        args["seek"] = json!(s);
    }
    if let Some(v) = volume {
        args["volume"] = json!(v);
    }
    if let Some(sp) = speed {
        args["speed"] = json!(sp);
    }
    send("play", args)
}

#[command]
pub fn player_pause() -> Result<serde_json::Value, String> {
    send("pause", json!({}))
}

#[command]
pub fn player_resume() -> Result<serde_json::Value, String> {
    send("resume", json!({}))
}

#[command]
pub fn player_stop() -> Result<serde_json::Value, String> {
    send("stop", json!({}))
}

#[command]
pub fn player_seek(seconds: f64) -> Result<serde_json::Value, String> {
    send("seek", json!({"seconds": seconds}))
}

#[command]
pub fn player_set_volume(level: i64) -> Result<serde_json::Value, String> {
    send("volume", json!({"level": level}))
}

#[command]
pub fn player_set_speed(rate: f64) -> Result<serde_json::Value, String> {
    send("speed", json!({"rate": rate}))
}

#[command]
pub fn player_status() -> Result<serde_json::Value, String> {
    send("status", json!({}))
}
