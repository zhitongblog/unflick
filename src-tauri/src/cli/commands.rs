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
    /// Take a screenshot of the current frame
    Screenshot {
        /// Output file path (default: auto-generated)
        #[arg(long)]
        output: Option<String>,
    },
    /// Manage subtitles
    Subtitle {
        #[command(subcommand)]
        action: SubtitleAction,
    },
    /// Manage the playlist
    Playlist {
        #[command(subcommand)]
        action: PlaylistAction,
    },
    /// Manage the media library
    Library {
        #[command(subcommand)]
        action: LibraryAction,
    },
    /// Shut down the daemon
    Shutdown,
}

#[derive(Subcommand)]
pub enum SubtitleAction {
    /// Load an external subtitle file
    Load {
        /// Path to the subtitle file (.srt, .ass, .sub)
        file: String,
    },
    /// List subtitle tracks
    List,
    /// Select a subtitle track by ID (0 to disable)
    Select {
        /// Subtitle track ID
        id: i64,
    },
}

#[derive(Subcommand)]
pub enum PlaylistAction {
    /// Add a file to the playlist
    Add {
        /// Path to the media file
        file: String,
    },
    /// Remove entry at index
    Remove {
        /// Index of the entry to remove
        index: usize,
    },
    /// List all playlist entries
    List,
    /// Play the next track
    Next,
    /// Play the previous track
    Prev,
    /// Clear the playlist
    Clear,
    /// Play a specific entry by index
    Play {
        /// Index of the entry to play
        index: usize,
    },
}

#[derive(Subcommand)]
pub enum LibraryAction {
    /// Scan a directory for video files and add them to the library
    Scan {
        /// Directory path to scan
        dir: String,
    },
    /// Search the library by title or path
    Search {
        /// Search query
        query: String,
    },
    /// List all media in the library
    List,
    /// Remove an entry by ID
    Remove {
        /// Media entry ID
        id: i64,
    },
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
        Some(Commands::Screenshot { output }) => {
            let mut args = json!({});
            if let Some(o) = output { args["output"] = json!(o); }
            send("screenshot", args)
        }
        Some(Commands::Info { file }) => {
            ensure_daemon();
            send("info", json!({"file": file}))
        }
        Some(Commands::Subtitle { action }) => {
            ensure_daemon();
            match action {
                SubtitleAction::Load { file } => send("subtitle_load", json!({"file": file})),
                SubtitleAction::List => send("subtitle_list", json!({})),
                SubtitleAction::Select { id } => send("subtitle_select", json!({"id": id})),
            }
        }
        Some(Commands::Playlist { action }) => {
            ensure_daemon();
            match action {
                PlaylistAction::Add { file } => send("playlist_add", json!({"file": file})),
                PlaylistAction::Remove { index } => send("playlist_remove", json!({"index": index})),
                PlaylistAction::List => send("playlist_list", json!({})),
                PlaylistAction::Next => send("playlist_next", json!({})),
                PlaylistAction::Prev => send("playlist_prev", json!({})),
                PlaylistAction::Clear => send("playlist_clear", json!({})),
                PlaylistAction::Play { index } => send("playlist_play", json!({"index": index})),
            }
        }
        Some(Commands::Library { action }) => {
            ensure_daemon();
            match action {
                LibraryAction::Scan { dir } => send("library_scan", json!({"dir": dir})),
                LibraryAction::Search { query } => send("library_search", json!({"query": query})),
                LibraryAction::List => send("library_list", json!({})),
                LibraryAction::Remove { id } => send("library_remove", json!({"id": id})),
            }
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
