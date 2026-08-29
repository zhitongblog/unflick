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

/// `unflick cast` with no action reports what is being cast.
#[derive(Subcommand, Debug, Clone)]
pub enum CastAction {
    /// Find DLNA renderers on the network
    List {
        /// How long to listen for answers. Replies are spread out on
        /// purpose, so a short wait finds only the quickest television.
        #[arg(long, default_value = "3")]
        seconds: f64,
    },
    /// Send what is playing (or a named file) to a renderer
    To {
        /// Which renderer — its name, or part of it. Omit when there is
        /// only one.
        renderer: Option<String>,
        /// A file to cast instead of what is playing
        #[arg(long)]
        file: Option<String>,
        #[arg(long, default_value = "3")]
        seconds: f64,
    },
    /// Stop casting
    Stop,
    /// Where the cast has got to
    Status,
    Pause,
    Resume,
    /// Seek the cast, in seconds
    Seek { seconds: f64 },
}

/// `unflick session` with no action reports; the others act.
#[derive(Subcommand, Debug, Clone)]
pub enum SessionAction {
    /// Report what would be resumed (the default)
    Show,
    /// Reopen it, at the point it got to
    Restore,
    /// Forget it
    Clear,
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
    #[command(allow_negative_numbers = true)]
    Seek {
        /// Position in seconds
        seconds: f64,
    },
    /// Set volume (0-100)
    Volume {
        /// Volume level
        level: i64,
    },
    /// Get or set the playback speed
    #[command(allow_negative_numbers = true)]
    Speed {
        /// Speed multiplier (e.g. 1.5). Omit to read the current rate.
        rate: Option<f64>,
        /// Treat the value as an offset from the current rate
        #[arg(long)]
        relative: bool,
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
    /// Manage SponsorBlock auto-skip (segment fetch + on/off + categories)
    Sponsor {
        #[command(subcommand)]
        action: SponsorAction,
    },
    /// Navigate chapters of the current file
    Chapter {
        #[command(subcommand)]
        action: ChapterAction,
    },
    /// A-B loop: repeat a section of the current file
    Loop {
        #[command(subcommand)]
        action: LoopAction,
    },
    /// Step the current file one frame at a time, or capture one as an image
    Frame {
        #[command(subcommand)]
        action: FrameAction,
    },
    /// Read and search the current file's subtitles as a transcript
    Transcript {
        #[command(subcommand)]
        action: TranscriptAction,
    },
    /// View and change keyboard shortcuts
    Keybind {
        #[command(subcommand)]
        action: KeybindAction,
    },
    /// View and change mouse bindings (wheel, clicks, drag gestures)
    Mouse {
        #[command(subcommand)]
        action: MouseAction,
    },
    /// Send what is playing to a TV on the network (DLNA)
    Cast {
        #[command(subcommand)]
        action: Option<CastAction>,
    },
    /// Optical drives, and whether there is a video disc in them
    ///
    /// Play one by path: `unflick play D:\` or `unflick play film.iso`.
    /// A specific title is mpv's own syntax — `unflick play dvd://3`.
    Disc {
        /// A path to ask about instead of listing drives — an image, a
        /// folder, or a drive. Reports what it is without opening it.
        path: Option<String>,
    },
    /// What was being watched, and getting back to it
    Session {
        #[command(subcommand)]
        action: Option<SessionAction>,
    },
    /// The last launch's startup timeline, phase by phase
    Startup,
    /// Find (and optionally remove) files an older unflick left behind
    Cleanup {
        /// Actually delete them. Without this the command only reports.
        #[arg(long)]
        apply: bool,
    },
    /// The player window: normal, picture-in-picture, or music
    Window {
        #[command(subcommand)]
        action: WindowAction,
    },
    /// What is playing, as tags rather than a path
    Nowplaying {
        /// Also extract the embedded cover art and return its path
        #[arg(long)]
        cover: bool,
    },
    /// Picture geometry: aspect ratio, rotation, zoom, deinterlace
    Video {
        #[command(subcommand)]
        action: VideoAction,
    },
    /// Turn incognito on or off (nothing is written to the play history)
    Incognito {
        /// on | off. Omit to read the current setting.
        enabled: Option<String>,
    },
    /// List or clear recently played files
    Recent {
        #[command(subcommand)]
        action: RecentAction,
    },
    /// Save, list, and jump back to positions in a file
    Bookmark {
        #[command(subcommand)]
        action: BookmarkAction,
    },
    /// Shut down the daemon
    Shutdown,
}

#[derive(Subcommand)]
pub enum ChapterAction {
    /// List all chapters of the current file
    List,
    /// Jump to a chapter by 0-based index
    Seek {
        /// Chapter index
        index: i64,
    },
    /// Jump to the next chapter
    Next,
    /// Jump to the previous chapter
    Prev,
    /// Derive chapters from the transcript, for a file that has none
    Generate {
        /// Roughly how many chapters to aim for
        #[arg(long, default_value_t = 8)]
        count: u64,
    },
    /// Set chapters explicitly from JSON: [{"time": 0, "title": "Intro"}, …]
    Set {
        /// JSON array of {time, title} objects
        json: String,
    },
    /// Remove generated chapters (container chapters are untouched)
    Clear,
}

#[derive(Subcommand)]
pub enum KeybindAction {
    /// List every bindable action with its current key
    List,
    /// Bind a key to an action, e.g. `keybind set play_pause k`
    Set {
        /// Action id from `keybind list`
        action: String,
        /// Key, e.g. `k`, `Shift+z`, `Mod+o`, `PageUp`
        key: String,
    },
    /// Restore defaults — one action, or all of them
    Reset {
        /// Action id. Omit to reset every binding.
        action: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum VideoAction {
    /// Show the current aspect, rotation, zoom, pan-scan and deinterlace
    Get,
    /// Set one: aspect | rotate | zoom | panscan | deinterlace
    Set {
        /// Property name
        name: String,
        /// aspect: auto | 16:9 | 1.78 · rotate: 0|90|180|270 · zoom: 1.0
        /// · panscan: 0-1 · deinterlace: on|off
        value: String,
    },
    /// Back to the file's own geometry
    Reset,
}

#[derive(Subcommand)]
pub enum BookmarkAction {
    /// Save a position — where playback is now, unless told otherwise
    Add {
        /// Label for it. Omit and it shows as its timestamp.
        #[arg(long)]
        name: Option<String>,
        /// Position in seconds. Defaults to the current position.
        #[arg(long)]
        position: Option<f64>,
        /// File to bookmark. Defaults to what's playing.
        #[arg(long)]
        file: Option<String>,
    },
    /// Show bookmarks for the current file
    List {
        /// A different file
        #[arg(long)]
        file: Option<String>,
        /// Every file instead of just one
        #[arg(long, conflicts_with = "file")]
        all: bool,
    },
    /// Jump to a bookmark, opening its file if it isn't the one playing
    Goto {
        /// Bookmark id from `bookmark list`
        id: i64,
    },
    /// Rename a bookmark, or drop its name with `--clear`
    Rename {
        /// Bookmark id from `bookmark list`
        id: i64,
        /// The new name
        #[arg(required_unless_present = "clear")]
        name: Option<String>,
        /// Remove the name instead, leaving it shown as its timestamp
        #[arg(long, conflicts_with = "name")]
        clear: bool,
    },
    /// Delete one bookmark
    Remove {
        /// Bookmark id from `bookmark list`
        id: i64,
    },
    /// Delete every bookmark for the current file
    Clear {
        /// A different file
        #[arg(long)]
        file: Option<String>,
        /// Every bookmark, for every file
        #[arg(long, conflicts_with = "file")]
        all: bool,
    },
}

#[derive(Subcommand)]
pub enum RecentAction {
    /// Show recently played files, newest first
    List {
        /// How many to return
        #[arg(long, default_value_t = 20)]
        limit: u64,
    },
    /// Forget the play history (scanned metadata is kept)
    Clear,
}

#[derive(Subcommand)]
pub enum MouseAction {
    /// List every mouse trigger with the action it runs
    List,
    /// Point a trigger at an action, e.g. `mouse set wheel_up volume_up`
    Set {
        /// Trigger id from `mouse list`
        trigger: String,
        /// Action id from `keybind list`, or `none` to disable
        action: String,
    },
    /// Restore defaults — one trigger, or all of them
    Reset {
        /// Trigger id. Omit to reset every mouse binding.
        trigger: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum TranscriptAction {
    /// Print every cue of the current file's transcript
    Get,
    /// Find lines containing a phrase
    Search {
        /// Text to look for (case-insensitive)
        query: String,
        /// Maximum matches to return
        #[arg(long, default_value_t = 20)]
        limit: u64,
    },
    /// Jump to where a phrase is spoken
    Seek {
        /// Text to look for (case-insensitive)
        query: String,
        /// Which occurrence to jump to, 1-based
        #[arg(long, default_value_t = 1)]
        occurrence: u64,
    },
}

#[derive(Subcommand)]
pub enum LoopAction {
    /// Set the loop start point (defaults to the current position)
    A {
        /// Position in seconds
        position: Option<f64>,
    },
    /// Set the loop end point (defaults to the current position)
    B {
        /// Position in seconds
        position: Option<f64>,
    },
    /// Clear both loop points
    Clear,
    /// Show the current loop points
    Status,
}

#[derive(Subcommand)]
pub enum FrameAction {
    /// Step forward one frame (pauses playback)
    Next,
    /// Step back one frame (pauses playback)
    Prev,
    /// Save the timeline preview frame for a position (cached, keyframe-accurate)
    Thumbnail {
        /// Position in seconds
        position: f64,
        /// Output file path
        #[arg(long, default_value = "unflick-thumb.jpg")]
        output: String,
        /// Preview width in pixels
        #[arg(long, default_value_t = 160)]
        width: u64,
    },
    /// Save the current frame as a downscaled JPEG
    Capture {
        /// Output file path
        #[arg(long, default_value = "unflick-frame.jpg")]
        output: String,
        /// Seek here first, in seconds
        #[arg(long)]
        position: Option<f64>,
        /// Longest edge of the saved image, in pixels
        #[arg(long, default_value_t = 768)]
        max_edge: u64,
    },
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
    /// Find subtitles for the playing file on OpenSubtitles
    ///
    /// Requires an API key: get one free at
    /// https://www.opensubtitles.com/consumers, then run
    /// `unflick settings set opensubtitles_api_key <key>`.
    Search {
        /// Search text. Defaults to a title derived from the playing file.
        query: Option<String>,
        /// Video to match against. Defaults to the playing file.
        #[arg(long)]
        file: Option<String>,
        /// Comma-separated language codes, e.g. "zh-CN,en".
        /// Defaults to the `opensubtitles_languages` setting, else "en".
        #[arg(long = "lang")]
        languages: Option<String>,
        /// Skip the file hash and search by title only
        #[arg(long = "no-hash")]
        no_hash: bool,
    },
    /// Download a subtitle found by `subtitle search` and load it
    Download {
        /// The `file_id` from a search result
        file_id: i64,
        /// Video to save the subtitle beside. Defaults to the playing file.
        #[arg(long)]
        file: Option<String>,
        /// Language code, used in the saved filename
        #[arg(long = "lang")]
        language: Option<String>,
        /// Save under this exact filename instead of the derived one
        #[arg(long)]
        name: Option<String>,
        /// Download without loading it into the player
        #[arg(long = "no-load")]
        no_load: bool,
    },
    /// Search and download the best match in one step
    Auto {
        /// Search text. Defaults to a title derived from the playing file.
        query: Option<String>,
        /// Video to match against. Defaults to the playing file.
        #[arg(long)]
        file: Option<String>,
        /// Comma-separated language codes, e.g. "zh-CN,en"
        #[arg(long = "lang")]
        languages: Option<String>,
        /// Skip the file hash and search by title only
        #[arg(long = "no-hash")]
        no_hash: bool,
        /// Download without loading it into the player
        #[arg(long = "no-load")]
        no_load: bool,
    },
    /// Get or set the subtitle delay. Positive values show subtitles later.
    #[command(allow_negative_numbers = true)]
    Delay {
        /// Delay in seconds. Omit to read the current value.
        seconds: Option<f64>,
        /// Treat the value as an offset from the current delay
        #[arg(long)]
        relative: bool,
    },
    /// Get or set subtitle appearance
    Style {
        #[command(subcommand)]
        action: StyleAction,
    },
}

#[derive(Subcommand)]
pub enum StyleAction {
    /// Print all subtitle style values
    Get,
    /// Set one style property
    Set {
        /// One of: scale | pos | color | border_size | bold
        name: String,
        /// New value (number, #RRGGBBAA colour, or true/false for bold)
        value: String,
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
    /// Get or set the audio delay. Positive values play audio later.
    #[command(allow_negative_numbers = true)]
    Delay {
        /// Delay in seconds. Omit to read the current value.
        seconds: Option<f64>,
        /// Treat the value as an offset from the current delay
        #[arg(long)]
        relative: bool,
    },
    /// 10-band equaliser and loudness normalisation
    Eq {
        #[command(subcommand)]
        action: EqAction,
    },
    /// Keep the original pitch when playback speed changes
    Pitch {
        /// on | off. Omit to read the current setting.
        state: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum WindowAction {
    /// Read or set the window mode
    Mode {
        /// normal | pip | music. Omit to read the current mode.
        mode: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum EqAction {
    /// Show the current curve, preamp and normalisation state
    Get,
    /// Turn the equaliser on or off without discarding the curve
    On,
    Off,
    /// Set one band's gain in dB
    #[command(allow_negative_numbers = true)]
    Band {
        /// Band index, 0-9 (31 Hz to 16 kHz)
        index: i64,
        /// Gain in dB, -12 to +12
        gain: f64,
    },
    /// Set the whole curve at once
    #[command(allow_negative_numbers = true)]
    Curve {
        /// Ten gains in dB, low to high, e.g. `-4 -3 -1 2 4 4 3 1 -1 -2`
        #[arg(num_args = 10)]
        gains: Vec<f64>,
    },
    /// Set the preamp in dB (negative makes headroom for boosted bands)
    #[command(allow_negative_numbers = true)]
    Preamp {
        db: f64,
    },
    /// Even out quiet dialogue against loud action
    Normalize {
        /// on | off
        state: String,
    },
    /// Apply a named preset
    Preset {
        /// Preset name; omit to list what's available
        name: Option<String>,
    },
    /// Clear every audio filter
    Reset,
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
    /// Get or set the repeat mode
    Repeat {
        /// off | one | all. Omit to read the current mode.
        mode: Option<String>,
    },
    /// Get or set shuffle
    Shuffle {
        /// on | off. Omit to read the current setting.
        enabled: Option<String>,
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
pub enum SponsorAction {
    /// Fetch SponsorBlock segments for a YouTube URL and print them as JSON.
    /// 404 / no-segments returns an empty list (success).
    List {
        /// YouTube URL (watch / shorts / youtu.be / embed all accepted)
        url: String,
    },
    /// Turn SponsorBlock auto-skip on (writes settings).
    Enable,
    /// Turn SponsorBlock auto-skip off (writes settings).
    Disable,
    /// Replace the SponsorBlock category list (comma-separated, e.g.
    /// `sponsor,selfpromo,intro,outro,interaction`).
    Categories {
        /// Comma-separated list of categories
        list: String,
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

/// Coerce a CLI style value into the JSON type the daemon expects.
///
/// `subtitle style set` takes everything as a string on the command line,
/// but `scale` wants a number and `bold` a boolean — pushing raw strings
/// through would make every property fall back to its default.
/// Colours (`#RRGGBBAA`) stay strings.

/// Build the argument object shared by `subtitle search` and `subtitle auto`.
///
/// Omitted options are left out entirely rather than sent as null, so the
/// daemon's "fall back to the playing file / the configured languages"
/// defaults stay in one place instead of being duplicated here.
fn search_args(
    query: Option<String>,
    file: Option<String>,
    languages: Option<String>,
    no_hash: bool,
) -> serde_json::Value {
    let mut args = json!({});
    if let Some(q) = query {
        args["query"] = json!(q);
    }
    if let Some(f) = file {
        args["file"] = json!(f);
    }
    if let Some(l) = languages {
        args["languages"] = json!(l);
    }
    if no_hash {
        args["hash"] = json!(false);
    }
    args
}

fn parse_style_value(raw: &str) -> serde_json::Value {
    let trimmed = raw.trim();
    match trimmed.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" => return json!(true),
        "false" | "no" | "off" => return json!(false),
        _ => {}
    }
    // Integers must stay integers. Properties like `rotate` and `pos` are
    // read with `as_i64`, which rejects a JSON float outright — so
    // coercing "90" to 90.0 here silently fed them their default instead
    // of the value the user typed.
    if let Ok(n) = trimmed.parse::<i64>() {
        return json!(n);
    }
    if let Ok(n) = trimmed.parse::<f64>() {
        return json!(n);
    }
    json!(trimmed)
}

#[cfg(test)]
mod tests {
    use super::{parse_on_off, parse_style_value};

    #[test]
    fn whole_numbers_stay_integers() {
        // The regression: properties read with `as_i64` see nothing at all
        // in a JSON float, so "90" arriving as 90.0 became the default.
        assert!(parse_style_value("90").is_i64());
        assert_eq!(parse_style_value("90").as_i64(), Some(90));
        assert_eq!(parse_style_value("-5").as_i64(), Some(-5));
        assert_eq!(parse_style_value("0").as_i64(), Some(0));
    }

    #[test]
    fn fractional_numbers_stay_floats() {
        assert_eq!(parse_style_value("1.4").as_f64(), Some(1.4));
        assert_eq!(parse_style_value("0.5").as_f64(), Some(0.5));
    }

    #[test]
    fn booleans_are_recognised_by_their_usual_spellings() {
        for yes in ["true", "yes", "on", "TRUE", "On"] {
            assert_eq!(parse_style_value(yes).as_bool(), Some(true), "{yes}");
        }
        for no in ["false", "no", "off", "OFF"] {
            assert_eq!(parse_style_value(no).as_bool(), Some(false), "{no}");
        }
    }

    #[test]
    fn anything_else_stays_a_string() {
        assert_eq!(parse_style_value("#FF00FFAA").as_str(), Some("#FF00FFAA"));
        assert_eq!(parse_style_value(" 16:9 ").as_str(), Some("16:9"));
    }

    #[test]
    fn on_off_parsing_covers_the_same_spellings() {
        assert_eq!(parse_on_off("on"), Some(true));
        assert_eq!(parse_on_off("OFF"), Some(false));
        assert_eq!(parse_on_off("1"), Some(true));
        assert_eq!(parse_on_off("0"), Some(false));
        assert_eq!(parse_on_off("maybe"), None);
    }
}

/// Accept the usual spellings for a boolean flag argument.
/// Scope arguments shared by `bookmark list` and `bookmark clear`. Neither
/// key is sent when the user named neither, which is what tells the daemon
/// to fall back to the file that's playing.
fn bookmark_scope_args(file: Option<String>, all: bool) -> serde_json::Value {
    let mut args = json!({});
    if all {
        args["all"] = json!(true);
    }
    if let Some(f) = file {
        args["file"] = json!(f);
    }
    args
}

fn parse_on_off(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Some(true),
        "off" | "false" | "no" | "0" => Some(false),
        _ => None,
    }
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

            // Surface URL extraction progress on stderr so the user sees
            // *something* during the up-to-60s yt-dlp call. stdout is the
            // JSON command result the caller pipes elsewhere — keep it
            // clean.
            if crate::core::yt_dlp::is_http_url(&file) {
                eprintln!("[unflick] resolving {}...", file);
            }

            let mut args = json!({"file": file});
            if let Some(s) = seek { args["seek"] = json!(s); }
            if let Some(v) = volume { args["volume"] = json!(v); }
            if let Some(sp) = speed { args["speed"] = json!(sp); }
            // Forward the saved proxy setting (if any) to the daemon so
            // yt-dlp can use it. The daemon will only use it for URL
            // extraction; local-file plays ignore it.
            if let Ok(all) = crate::core::settings::read_all() {
                if let Some(p) = all.get("proxy").and_then(|v| v.as_str()) {
                    if !p.is_empty() {
                        args["proxy"] = json!(p);
                    }
                }
            }
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
        Some(Commands::Speed { rate, relative }) => {
            let mut args = json!({});
            if let Some(r) = rate {
                args["rate"] = json!(r);
                args["relative"] = json!(relative);
            }
            send("speed", args)
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
                SubtitleAction::Search { query, file, languages, no_hash } => {
                    send("subtitle_search", search_args(query, file, languages, no_hash))
                }
                SubtitleAction::Download { file_id, file, language, name, no_load } => {
                    let mut args = json!({"file_id": file_id, "load": !no_load});
                    if let Some(f) = file { args["file"] = json!(f); }
                    if let Some(l) = language { args["language"] = json!(l); }
                    if let Some(n) = name { args["name"] = json!(n); }
                    send("subtitle_download", args)
                }
                SubtitleAction::Auto { query, file, languages, no_hash, no_load } => {
                    let mut args = search_args(query, file, languages, no_hash);
                    args["load"] = json!(!no_load);
                    send("subtitle_auto", args)
                }
                SubtitleAction::Delay { seconds, relative } => {
                    let mut args = json!({"relative": relative});
                    if let Some(s) = seconds { args["seconds"] = json!(s); }
                    send("subtitle_delay", args)
                }
                SubtitleAction::Style { action } => match action {
                    StyleAction::Get => send("subtitle_style_get", json!({})),
                    StyleAction::Set { name, value } => {
                        send("subtitle_style_set", json!({"name": name, "value": parse_style_value(&value)}))
                    }
                },
            }
        }
        Some(Commands::Audio { action }) => {
            ensure_daemon();
            match action {
                AudioAction::List => send("audio_list", json!({})),
                AudioAction::Select { id } => send("audio_select", json!({"id": id})),
                AudioAction::Delay { seconds, relative } => {
                    let mut args = json!({"relative": relative});
                    if let Some(s) = seconds { args["seconds"] = json!(s); }
                    send("audio_delay", args)
                }
                AudioAction::Eq { action } => match action {
                    EqAction::Get => send("audio_eq_get", json!({})),
                    EqAction::On => send("audio_eq_set", json!({"enabled": true})),
                    EqAction::Off => send("audio_eq_set", json!({"enabled": false})),
                    EqAction::Band { index, gain } => send(
                        "audio_eq_set",
                        // Reaching for a band means wanting to hear it, the
                        // same way `curve` and `preset` do.
                        json!({"band": index, "gain": gain, "enabled": true}),
                    ),
                    EqAction::Curve { gains } => {
                        // Turning it on is implied: nobody types ten numbers
                        // to leave the equaliser bypassed.
                        send("audio_eq_set", json!({"bands": gains, "enabled": true}))
                    }
                    EqAction::Preamp { db } => send("audio_eq_set", json!({"preamp": db})),
                    EqAction::Normalize { state } => match parse_on_off(&state) {
                        Some(on) => send("audio_eq_set", json!({"normalize": on})),
                        None => CommandResult::err(format!("expected on or off, got: {}", state)),
                    },
                    EqAction::Preset { name } => match name {
                        Some(n) => send("audio_eq_preset", json!({"name": n})),
                        None => send("audio_eq_presets", json!({})),
                    },
                    EqAction::Reset => send("audio_eq_reset", json!({})),
                },
                AudioAction::Pitch { state } => match state {
                    Some(v) => match parse_on_off(&v) {
                        Some(on) => send("audio_pitch", json!({"enabled": on})),
                        None => CommandResult::err(format!("expected on or off, got: {}", v)),
                    },
                    None => send("audio_pitch", json!({})),
                },
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
                PlaylistAction::Repeat { mode } => {
                    let mut args = json!({});
                    if let Some(m) = mode { args["mode"] = json!(m); }
                    send("playlist_repeat", args)
                }
                PlaylistAction::Shuffle { enabled } => match enabled {
                    None => send("playlist_shuffle", json!({})),
                    Some(e) => match parse_on_off(&e) {
                        Some(b) => send("playlist_shuffle", json!({"enabled": b})),
                        None => CommandResult::err(format!(
                            "invalid shuffle value: {} (expected on | off)",
                            e
                        )),
                    },
                },
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
        Some(Commands::Sponsor { action }) => {
            handle_sponsor(action)
        }
        Some(Commands::Chapter { action }) => {
            ensure_daemon();
            match action {
                ChapterAction::List => send("chapter_list", json!({})),
                ChapterAction::Seek { index } => send("chapter_seek", json!({"index": index})),
                ChapterAction::Next => send("chapter_next", json!({})),
                ChapterAction::Prev => send("chapter_prev", json!({})),
                ChapterAction::Generate { count } => {
                    send("chapters_generate", json!({"count": count}))
                }
                ChapterAction::Set { json: raw } => match serde_json::from_str::<serde_json::Value>(&raw) {
                    Ok(parsed) => send("chapters_set", json!({"chapters": parsed})),
                    Err(e) => CommandResult::err(format!("invalid JSON: {}", e)),
                },
                ChapterAction::Clear => send("chapters_clear", json!({})),
            }
        }
        Some(Commands::Keybind { action }) => {
            // Bindings live in settings.json, so no player is needed —
            // same as the `settings` subcommand.
            ensure_daemon();
            match action {
                KeybindAction::List => send("keybind_list", json!({})),
                KeybindAction::Set { action, key } => {
                    send("keybind_set", json!({"action": action, "key": key}))
                }
                KeybindAction::Reset { action } => {
                    let mut args = json!({});
                    if let Some(a) = action { args["action"] = json!(a); }
                    send("keybind_reset", args)
                }
            }
        }
        Some(Commands::Cast { action }) => {
            ensure_daemon();
            let args = match action {
                None | Some(CastAction::Status) => json!({"action": "status"}),
                Some(CastAction::List { seconds }) => {
                    json!({"action": "list", "seconds": seconds})
                }
                Some(CastAction::To { renderer, file, seconds }) => {
                    let mut a = json!({"action": "to", "seconds": seconds});
                    if let Some(r) = renderer { a["renderer"] = json!(r); }
                    if let Some(f) = file { a["file"] = json!(f); }
                    a
                }
                Some(CastAction::Stop) => json!({"action": "stop"}),
                Some(CastAction::Pause) => json!({"action": "pause"}),
                Some(CastAction::Resume) => json!({"action": "resume"}),
                Some(CastAction::Seek { seconds }) => {
                    json!({"action": "seek", "seconds": seconds})
                }
            };
            send("cast", args)
        }
        Some(Commands::Disc { path }) => {
            ensure_daemon();
            let mut args = json!({});
            if let Some(p) = path {
                args["path"] = json!(p);
            }
            send("disc_list", args)
        }
        Some(Commands::Session { action }) => {
            let verb = match action {
                None | Some(SessionAction::Show) => "show",
                Some(SessionAction::Restore) => "restore",
                Some(SessionAction::Clear) => "clear",
            };
            // `restore` starts playing, so it needs a player. `show` and
            // `clear` only touch the database — but they go through the
            // daemon anyway, because the running player is what is keeping
            // that row current, and reading around it would race with it.
            ensure_daemon();
            send("session", json!({"action": verb}))
        }
        Some(Commands::Startup) => {
            // No `ensure_daemon()`, same reasoning as `cleanup`: this reads
            // a log file. Starting a player to ask how long the last player
            // took to start would measure the wrong launch.
            let path = crate::core::boot::log_path();
            match std::fs::read_to_string(&path) {
                Err(e) => CommandResult::err(format!("{}: {}", path.display(), e)),
                Ok(body) => {
                    let phases = crate::core::boot::parse_last_launch(&body);
                    let total = phases.last().map(|p| p.at_ms).unwrap_or(0);
                    let message = match phases.last() {
                        None => format!("no startup marks in {}", path.display()),
                        Some(last) => {
                            format!("last launch reached \"{}\" at {} ms", last.label, total)
                        }
                    };
                    CommandResult::ok_with_data(
                        message,
                        json!({
                            "total_ms": total,
                            "phases": phases,
                            "log": path.to_string_lossy(),
                        }),
                    )
                }
            }
        }
        Some(Commands::Cleanup { apply }) => {
            // No `ensure_daemon()`: this reads the filesystem, and starting
            // a player to ask about disk space would be absurd. It also has
            // to work when the app is too broken to start.
            if apply {
                match crate::core::cleanup::remove_leftovers() {
                    Ok(report) => CommandResult::ok_with_data(
                        format!(
                            "removed {}",
                            crate::core::cleanup::human_size(report.total_bytes)
                        ),
                        serde_json::to_value(&report).unwrap(),
                    ),
                    Err(e) => CommandResult::err(e.to_string()),
                }
            } else {
                let report = crate::core::cleanup::scan();
                let message = match (&report.directory, report.items.len()) {
                    (None, _) => "nothing left behind".to_string(),
                    (Some(_), 0) => "nothing left to remove".to_string(),
                    (Some(dir), n) => format!(
                        "{} in {} item(s) at {} — re-run with --apply to remove",
                        crate::core::cleanup::human_size(report.total_bytes),
                        n,
                        dir
                    ),
                };
                CommandResult::ok_with_data(message, serde_json::to_value(&report).unwrap())
            }
        }
        Some(Commands::Window { action }) => {
            ensure_daemon();
            match action {
                WindowAction::Mode { mode } => match mode {
                    None => send("window_mode", json!({})),
                    Some(m) => send("window_mode", json!({"mode": m})),
                },
            }
        }
        Some(Commands::Nowplaying { cover }) => {
            ensure_daemon();
            send("nowplaying", json!({"cover": cover}))
        }
        Some(Commands::Video { action }) => {
            ensure_daemon();
            match action {
                VideoAction::Get => send("video_get", json!({})),
                VideoAction::Set { name, value } => {
                    // `aspect` is the one that must stay a string —
                    // "16:9" would otherwise be coerced to nothing useful.
                    let v = if name == "aspect" { json!(value) } else { parse_style_value(&value) };
                    send("video_set", json!({"name": name, "value": v}))
                }
                VideoAction::Reset => send("video_reset", json!({})),
            }
        }
        Some(Commands::Incognito { enabled }) => {
            ensure_daemon();
            match enabled {
                None => send("incognito", json!({})),
                Some(e) => match parse_on_off(&e) {
                    Some(b) => send("incognito", json!({"enabled": b})),
                    None => CommandResult::err(format!(
                        "invalid value: {} (expected on | off)",
                        e
                    )),
                },
            }
        }
        Some(Commands::Recent { action }) => {
            ensure_daemon();
            match action {
                RecentAction::List { limit } => send("recent_list", json!({"limit": limit})),
                RecentAction::Clear => send("recent_clear", json!({})),
            }
        }
        Some(Commands::Bookmark { action }) => {
            ensure_daemon();
            match action {
                BookmarkAction::Add {
                    name,
                    position,
                    file,
                } => {
                    // Absent keys, not nulls: the daemon fills each in from
                    // what's playing, and a null would read as "no name" /
                    // "position zero".
                    let mut args = json!({});
                    if let Some(n) = name {
                        args["name"] = json!(n);
                    }
                    if let Some(p) = position {
                        args["position"] = json!(p);
                    }
                    if let Some(f) = file {
                        args["file"] = json!(f);
                    }
                    send("bookmark_add", args)
                }
                BookmarkAction::List { file, all } => {
                    send("bookmark_list", bookmark_scope_args(file, all))
                }
                BookmarkAction::Goto { id } => send("bookmark_goto", json!({"id": id})),
                BookmarkAction::Rename { id, name, clear } => {
                    let mut args = json!({ "id": id });
                    if !clear {
                        if let Some(n) = name {
                            args["name"] = json!(n);
                        }
                    }
                    send("bookmark_rename", args)
                }
                BookmarkAction::Remove { id } => send("bookmark_remove", json!({"id": id})),
                BookmarkAction::Clear { file, all } => {
                    send("bookmark_clear", bookmark_scope_args(file, all))
                }
            }
        }
        Some(Commands::Mouse { action }) => {
            ensure_daemon();
            match action {
                MouseAction::List => send("mouse_list", json!({})),
                MouseAction::Set { trigger, action } => {
                    send("mouse_set", json!({"trigger": trigger, "action": action}))
                }
                MouseAction::Reset { trigger } => {
                    let mut args = json!({});
                    if let Some(t) = trigger { args["trigger"] = json!(t); }
                    send("mouse_reset", args)
                }
            }
        }
        Some(Commands::Transcript { action }) => {
            ensure_daemon();
            match action {
                TranscriptAction::Get => send("transcript_get", json!({})),
                TranscriptAction::Search { query, limit } => {
                    send("transcript_search", json!({"query": query, "limit": limit}))
                }
                TranscriptAction::Seek { query, occurrence } => send(
                    "transcript_seek",
                    json!({"query": query, "occurrence": occurrence}),
                ),
            }
        }
        Some(Commands::Loop { action }) => {
            ensure_daemon();
            match action {
                LoopAction::A { position } => {
                    let mut args = json!({"action": "a"});
                    if let Some(p) = position { args["position"] = json!(p); }
                    send("ab_loop", args)
                }
                LoopAction::B { position } => {
                    let mut args = json!({"action": "b"});
                    if let Some(p) = position { args["position"] = json!(p); }
                    send("ab_loop", args)
                }
                LoopAction::Clear => send("ab_loop", json!({"action": "clear"})),
                LoopAction::Status => send("ab_loop", json!({"action": "status"})),
            }
        }
        Some(Commands::Frame { action }) => {
            ensure_daemon();
            match action {
                FrameAction::Next => send("frame_step", json!({})),
                FrameAction::Prev => send("frame_back_step", json!({})),
                FrameAction::Thumbnail { position, output, width } => send(
                    "thumbnail",
                    json!({"position": position, "output": output, "width": width}),
                ),
                FrameAction::Capture { output, position, max_edge } => {
                    let mut args = json!({"output": output, "max_edge": max_edge});
                    if let Some(p) = position { args["position"] = json!(p); }
                    send("describe_frame", args)
                }
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
        // For categorized URL extraction failures, lead with a friendly
        // line on stderr before dumping the full JSON. That way scripts
        // that only `tail -1` stderr see something useful, and humans
        // running interactively see "this video requires login..."
        // instead of a wall of yt-dlp Python traceback.
        if let Some(kind) = result
            .data
            .as_ref()
            .and_then(|d| d.get("error_kind"))
            .and_then(|k| k.as_str())
        {
            let human = match kind {
                "login_required" => "this video requires login (try setting cookies-from-browser in settings)",
                "geo_blocked" => "this video is not available in your region",
                "private" => "this video is private",
                "unsupported_site" => "this site is not supported by yt-dlp",
                "network" => "network error reaching the site (check your connection or proxy)",
                "timeout" => "extraction timed out after 60s",
                "cancelled" => "extraction was cancelled",
                _ => "",
            };
            if !human.is_empty() {
                eprintln!("[unflick] {}", human);
            }
        }
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

/// SponsorBlock CLI handler. The `list` subcommand needs the tokio runtime
/// for the async `fetch_segments`; the others are pure settings reads/writes.
fn handle_sponsor(action: SponsorAction) -> CommandResult {
    use crate::core::settings;
    use crate::core::url_post_play;

    match action {
        SponsorAction::List { url } => {
            // Build a one-shot tokio runtime for the async fetch. We don't
            // need full multi-threaded scheduling here — just block on a
            // single network call.
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => return CommandResult::err(format!("tokio runtime: {}", e)),
            };
            let snapshot = url_post_play::read_settings_snapshot();
            let cats = snapshot.sponsorblock_categories.clone();
            let cats_for_async = cats.clone();
            let url_owned = url.clone();
            let res = runtime.block_on(async move {
                url_post_play::fetch_segments_for_url(&url_owned, &cats_for_async).await
            });
            match res {
                Ok(segments) => CommandResult::ok_with_data(
                    format!("{} segment(s)", segments.len()),
                    json!({
                        "url": url,
                        "categories": cats,
                        "segments": segments,
                    }),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        SponsorAction::Enable => match settings::set("sponsorblock_enabled", json!(true)) {
            Ok(()) => CommandResult::ok_with_data(
                "sponsorblock_enabled = true",
                json!({"sponsorblock_enabled": true}),
            ),
            Err(e) => CommandResult::err(e.to_string()),
        },
        SponsorAction::Disable => match settings::set("sponsorblock_enabled", json!(false)) {
            Ok(()) => CommandResult::ok_with_data(
                "sponsorblock_enabled = false",
                json!({"sponsorblock_enabled": false}),
            ),
            Err(e) => CommandResult::err(e.to_string()),
        },
        SponsorAction::Categories { list } => {
            let parsed: Vec<String> = list
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if parsed.is_empty() {
                return CommandResult::err("at least one category required");
            }
            match settings::set("sponsorblock_categories", json!(parsed)) {
                Ok(()) => CommandResult::ok_with_data(
                    format!("set {} categories", parsed.len()),
                    json!({"sponsorblock_categories": parsed}),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
    }
}

/// Start daemon in background if not already running.
/// How long to give a freshly spawned daemon to open its port.
///
/// It has to create an mpv instance before it listens, so "ready" is not
/// instant. The old budget was two seconds, which was under the real cost
/// on a cold start — and every probe inside the wait was itself blocking
/// for two seconds on a refused connection, so the loop gave up after one
/// or two actual attempts and reported "daemon not running. Start it with:
/// unflick daemon" for a daemon that was still starting.
const DAEMON_READY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

fn ensure_daemon() {
    if daemon::is_daemon_running() {
        return;
    }

    // Spawn the daemon as a detached child process.
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("unflick: cannot locate own executable to start a daemon: {e}");
            return;
        }
    };
    let spawned = std::process::Command::new(exe)
        .arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            // Discarding this was how a daemon that never started became a
            // message telling the user to start it by hand.
            eprintln!("unflick: could not start the daemon: {e}");
            return;
        }
    };

    // Wait for it to be ready. Poll on a deadline rather than a fixed
    // number of attempts: how long a probe takes varies by two orders of
    // magnitude between "refused immediately" and "refused after the
    // OS finishes retrying", and a count of attempts silently becomes a
    // different timeout on each machine.
    let deadline = std::time::Instant::now() + DAEMON_READY_TIMEOUT;
    while std::time::Instant::now() < deadline {
        if daemon::is_daemon_running() {
            return;
        }
        // A daemon that died is never going to answer, and saying so beats
        // spending the rest of the budget on a process that no longer exists.
        if let Ok(Some(status)) = child.try_wait() {
            eprintln!("unflick: the daemon exited during startup ({status})");
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    eprintln!(
        "unflick: the daemon did not open {} within {:?}",
        daemon::control_addr(),
        DAEMON_READY_TIMEOUT
    );
}
