use clap::{Parser, Subcommand};
use serde_json::json;

use crate::core::daemon;
use crate::core::types::CommandResult;

#[derive(Parser)]
#[command(name = "unflick", version, about = "A video player for humans and AI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Start MCP server (stdio JSON-RPC)
    #[arg(long)]
    pub mcp: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the background daemon (holds the player instance)
    Daemon,
    /// Play a video file
    Play {
        /// Path to the video file
        file: String,
        /// Seek to position in seconds
        #[arg(long)]
        seek: Option<f64>,
        /// Set volume (0-100)
        #[arg(long)]
        volume: Option<i64>,
        /// Set playback speed
        #[arg(long)]
        speed: Option<f64>,
    },
    /// Pause playback
    Pause,
    /// Resume playback
    Resume,
    /// Stop playback
    Stop,
    /// Seek to position in seconds
    Seek {
        /// Position in seconds
        seconds: f64,
    },
    /// Set volume (0-100)
    Volume {
        /// Volume level
        level: i64,
    },
    /// Set playback speed
    Speed {
        /// Speed multiplier (e.g. 1.5)
        rate: f64,
    },
    /// Get current playback status
    Status,
    /// Get media file info
    Info {
        /// Path to the video file
        file: String,
    },
    /// Shut down the daemon
    Shutdown,
}

pub fn run_cli(cli: Cli) -> i32 {
    let result = match cli.command {
        Some(Commands::Daemon) => {
            if daemon::is_daemon_running() {
                CommandResult::err("daemon is already running")
            } else {
                // This blocks forever
                std::process::exit(daemon::start_daemon());
            }
        }
        Some(Commands::Play { file, seek, volume, speed }) => {
            // Auto-start daemon if not running
            ensure_daemon();

            let mut args = json!({"file": file});
            if let Some(s) = seek { args["seek"] = json!(s); }
            if let Some(v) = volume { args["volume"] = json!(v); }
            if let Some(sp) = speed { args["speed"] = json!(sp); }
            send("play", args)
        }
        Some(Commands::Pause) => {
            send("pause", json!({}))
        }
        Some(Commands::Resume) => {
            send("resume", json!({}))
        }
        Some(Commands::Stop) => {
            send("stop", json!({}))
        }
        Some(Commands::Seek { seconds }) => {
            send("seek", json!({"seconds": seconds}))
        }
        Some(Commands::Volume { level }) => {
            send("volume", json!({"level": level}))
        }
        Some(Commands::Speed { rate }) => {
            send("speed", json!({"rate": rate}))
        }
        Some(Commands::Status) => {
            send("status", json!({}))
        }
        Some(Commands::Info { file }) => {
            ensure_daemon();
            send("info", json!({"file": file}))
        }
        Some(Commands::Shutdown) => {
            send("shutdown", json!({}))
        }
        None => {
            CommandResult::err("no command specified. Use --help for usage.")
        }
    };

    let json = serde_json::to_string_pretty(&result).unwrap();
    if result.success {
        println!("{}", json);
        0
    } else {
        eprintln!("{}", json);
        1
    }
}

fn send(cmd: &str, args: serde_json::Value) -> CommandResult {
    match daemon::send_to_daemon(cmd, args) {
        Ok(r) => r,
        Err(e) => CommandResult::err(e),
    }
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

    // Wait for daemon to be ready
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if daemon::is_daemon_running() {
            return;
        }
    }
}
