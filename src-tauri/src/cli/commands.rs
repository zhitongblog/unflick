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
    /// Extract a video clip (requires ffmpeg)
    Clip {
        /// Start time in seconds
        start: f64,
        /// End time in seconds
        end: f64,
        /// Input file (uses currently playing file if omitted)
        #[arg(long)]
        file: Option<String>,
        /// Output file path (auto-generated if omitted)
        #[arg(long)]
        output: Option<String>,
        /// Export as GIF instead of MP4
        #[arg(long)]
        gif: bool,
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
    /// Manage audio tracks
    Audio {
        #[command(subcommand)]
        action: AudioAction,
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
    /// Manage user settings (persisted to a JSON file)
    Settings {
        #[command(subcommand)]
        action: SettingsAction,
    },
    /// Adjust video filters (brightness, contrast, saturation, gamma, hue)
    Filter {
        #[command(subcommand)]
        action: FilterAction,
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
    /// Generate subtitles for a video using whisper (local or OpenAI API)
    Generate {
        /// Path to the video file
        video: String,
        /// Transcription mode: "local" (whisper.cpp) or "api" (OpenAI). Auto-detected if omitted.
        #[arg(long)]
        mode: Option<String>,
        /// Path to whisper-cli executable (local mode; auto-detects bundled if omitted)
        #[arg(long)]
        whisper: Option<String>,
        /// Path to whisper model file (local mode; auto-detects bundled if omitted)
        #[arg(long)]
        model: Option<String>,
        /// OpenAI API key (api mode)
        #[arg(long)]
        api_key: Option<String>,
        /// Output directory for the .srt file (default: OS cache dir/unflick)
        #[arg(long)]
        output_dir: Option<String>,
    },
    /// Translate an SRT file to another language via OpenAI API
    Translate {
        /// Path to the source .srt file
        srt: String,
        /// Target language (e.g. "Chinese", "Spanish", "Japanese")
        #[arg(long = "to")]
        target_lang: String,
        /// OpenAI API key
        #[arg(long)]
        api_key: String,
        /// Output directory (default: OS cache dir/unflick)
        #[arg(long)]
        output_dir: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum AudioAction {
    /// List all audio tracks
    List,
    /// Select an audio track by ID (0 to disable)
    Select {
        /// Audio track ID
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
pub enum SettingsAction {
    /// Print the absolute path to the settings file
    Path,
    /// Print all settings (or a single key with --key)
    Get {
        /// Specific key to read; if omitted, prints the entire settings JSON
        #[arg(long)]
        key: Option<String>,
    },
    /// Set a single key to the given JSON value
    Set {
        /// Key name
        key: String,
        /// JSON-encoded value (e.g. '"foo"', '42', 'true', '{"a":1}'). Falls back to a string if not valid JSON.
        value: String,
    },
    /// Remove a single key
    Unset {
        /// Key name
        key: String,
    },
}

#[derive(Subcommand)]
pub enum FilterAction {
    /// Show all current filter values (brightness, contrast, saturation, gamma, hue)
    List,
    /// Set a filter to the given value (-100 to 100)
    Set {
        /// Filter name (brightness | contrast | saturation | gamma | hue)
        name: String,
        /// Value in the range -100 to 100 (0 = neutral)
        #[arg(allow_hyphen_values = true)]
        value: i64,
    },
    /// Reset all filters to 0 (neutral)
    Reset,
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
        Some(Commands::Clip { start, end, file, output, gif }) => {
            ensure_daemon();
            let mut args = json!({"start": start, "end": end, "gif": gif});
            if let Some(f) = file { args["file"] = json!(f); }
            if let Some(o) = output { args["output"] = json!(o); }
            send("clip", args)
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
                SubtitleAction::Generate { video, mode, whisper, model, api_key, output_dir } => {
                    let mut args = json!({"video": video});
                    if let Some(m) = mode { args["mode"] = json!(m); }
                    if let Some(w) = whisper { args["whisper"] = json!(w); }
                    if let Some(m) = model { args["model"] = json!(m); }
                    if let Some(k) = api_key { args["api_key"] = json!(k); }
                    if let Some(d) = output_dir { args["output_dir"] = json!(d); }
                    send("subtitle_generate", args)
                }
                SubtitleAction::Translate { srt, target_lang, api_key, output_dir } => {
                    let mut args = json!({"srt": srt, "target_lang": target_lang, "api_key": api_key});
                    if let Some(d) = output_dir { args["output_dir"] = json!(d); }
                    send("subtitle_translate", args)
                }
            }
        }
        Some(Commands::Audio { action }) => {
            ensure_daemon();
            match action {
                AudioAction::List => send("audio_list", json!({})),
                AudioAction::Select { id } => send("audio_select", json!({"id": id})),
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
        Some(Commands::Settings { action }) => {
            // Settings ops touch a JSON file directly — no daemon needed.
            handle_settings(action)
        }
        Some(Commands::Filter { action }) => {
            ensure_daemon();
            match action {
                FilterAction::List => send("filter_list", json!({})),
                FilterAction::Set { name, value } => send("filter_set", json!({"name": name, "value": value})),
                FilterAction::Reset => send("filter_reset", json!({})),
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

fn handle_settings(action: SettingsAction) -> CommandResult {
    use crate::core::settings;

    match action {
        SettingsAction::Path => CommandResult::ok_with_data(
            "ok",
            json!({"path": settings::settings_path().to_string_lossy()}),
        ),
        SettingsAction::Get { key } => match settings::read_all() {
            Ok(all) => match key {
                Some(k) => match all.get(&k) {
                    Some(v) => CommandResult::ok_with_data("ok", json!({"key": k, "value": v})),
                    None => CommandResult::err(format!("key not found: {}", k)),
                },
                None => CommandResult::ok_with_data("ok", all),
            },
            Err(e) => CommandResult::err(e.to_string()),
        },
        SettingsAction::Set { key, value } => {
            // Try to parse as JSON; fall back to a plain string if it's not valid JSON.
            let parsed: serde_json::Value =
                serde_json::from_str(&value).unwrap_or_else(|_| json!(value));
            match settings::set(&key, parsed.clone()) {
                Ok(()) => CommandResult::ok_with_data(
                    format!("set {}", key),
                    json!({"key": key, "value": parsed}),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        SettingsAction::Unset { key } => match settings::unset(&key) {
            Ok(true) => CommandResult::ok(format!("removed {}", key)),
            Ok(false) => CommandResult::err(format!("key not found: {}", key)),
            Err(e) => CommandResult::err(e.to_string()),
        },
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
