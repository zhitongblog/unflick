//! Control server: a TCP command surface that owns (or borrows) a Player.
//! CLI commands connect via a local TCP socket to send commands and receive responses.
//! The MCP server also routes through this socket.
//!
//! Two processes can host it:
//!
//!   * `unflick daemon` — headless, creates its own `vo=null` Player.
//!   * the GUI — hosts the same server against the *visible* render Player,
//!     so `unflick pause` and MCP `pause` act on the window the user is
//!     actually watching. See `ControlContext::embedded`.
//!
//! The GUI takes priority: on startup it asks any headless daemon to step
//! aside (`shutdown`) before binding. Only one host holds the port at a time.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

use super::audio::AudioSettings;
use super::nowplaying;
use super::opensubtitles;
use super::player::{self as player, Player};
use super::playlist::Playlist;
use super::source;
use super::window::{WindowHost, WindowMode};
use super::types::{CommandResult, RepeatMode};
use crate::db::Database;

const DEFAULT_CONTROL_ADDR: &str = "127.0.0.1:19542";

/// Environment override for the control port.
///
/// Two reasons this isn't just a constant: the integration tests must not
/// hijack a player the user is actually watching (and must not be hijacked
/// by it), and anyone wanting a second isolated instance shouldn't have to
/// rebuild. Read per call rather than cached — a test sets it in-process
/// before spawning the daemon.
pub const CONTROL_ADDR_ENV: &str = "UNFLICK_CONTROL_ADDR";

fn control_addr() -> String {
    std::env::var(CONTROL_ADDR_ENV)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_CONTROL_ADDR.to_string())
}

/// Everything a control-server connection needs. Shared across connection
/// threads; the GUI builds one around its own render Player so external
/// commands drive the on-screen window instead of a second, invisible mpv.
pub struct ControlContext {
    pub player: Arc<Player>,
    pub playlist: Arc<Playlist>,
    pub db: Arc<Database>,
    /// While set, nothing is written to the play history.
    ///
    /// Incognito used to be a frontend-only toggle, which was safe while
    /// the CLI drove a separate process. Since the GUI hosts the control
    /// server, a `unflick play` reaches the very player the user has
    /// incognito switched on for — so the flag has to live where both can
    /// see it.
    pub incognito: Arc<std::sync::atomic::AtomicBool>,
    /// True when hosted inside the GUI process. `shutdown` must not call
    /// `process::exit` there — it would take the whole app down, and the
    /// GUI answers `shutdown` by declining instead.
    pub embedded: bool,
    /// The window, when this server is hosted by the GUI.
    ///
    /// `None` in the headless daemon, and the window commands say so rather
    /// than reporting a mode change nothing performed. PiP shipped as a
    /// button with no headless equivalent; this is the seam that closes it.
    pub window: Option<Arc<dyn WindowHost>>,
}

/// Bind the control port and serve connections forever. Blocks the calling
/// thread. Returns the bind error if the port is already held.
pub fn serve_control(ctx: Arc<ControlContext>) -> std::io::Result<()> {
    let addr = control_addr();
    let listener = TcpListener::bind(&addr)?;
    eprintln!(
        "unflick control server listening on {} ({})",
        addr,
        if ctx.embedded { "gui" } else { "headless" }
    );

    spawn_autoplay_task(Arc::clone(&ctx));

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let ctx = Arc::clone(&ctx);
                thread::spawn(move || handle_client(stream, ctx));
            }
            Err(e) => {
                eprintln!("connection error: {}", e);
            }
        }
    }

    Ok(())
}

/// Start the headless daemon: create a `vo=null` Player and serve on the
/// control port. Blocks forever; the return value is a process exit code.
pub fn start_daemon() -> i32 {
    let player = match Player::new() {
        Ok(p) => Arc::new(p),
        Err(e) => {
            eprintln!("failed to initialize player: {}", e);
            return 1;
        }
    };

    let db = match Database::open() {
        Ok(d) => Arc::new(d),
        Err(e) => {
            eprintln!("failed to open database: {}", e);
            return 1;
        }
    };

    let ctx = Arc::new(ControlContext {
        player,
        playlist: Arc::new(Playlist::new()),
        db,
        embedded: false,
        incognito: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        // No GUI here, so no window to reshape.
        window: None,
    });

    if let Err(e) = serve_control(ctx) {
        eprintln!("failed to bind {}: {}", control_addr(), e);
        return 1;
    }

    0
}

/// Watch for end-of-file and advance the playlist.
///
/// mpv runs with `keep-open=yes`, so hitting the end parks the file at its
/// duration with `eof-reached` set rather than unloading it. Nothing ever
/// acted on that signal: a playlist played its first entry and stopped
/// there. This task is the single auto-advance driver for all three
/// interfaces, which works precisely because GUI, CLI and MCP now share one
/// control context.
///
/// It routes through `dispatch_command` rather than calling `Player::play`
/// directly so an advancing playlist gets the same treatment as a manual
/// play: position saved for the outgoing file, yt-dlp resolution for URL
/// entries, SponsorBlock re-armed.
fn spawn_autoplay_task(ctx: Arc<ControlContext>) {
    thread::spawn(move || {
        // Latches on the rising edge of `eof-reached` so one EOF advances
        // exactly one track, no matter how many polls observe it.
        let mut handled = false;

        loop {
            thread::sleep(Duration::from_millis(400));

            let eof = ctx.player.get_property_bool("eof-reached").unwrap_or(false);
            if !eof {
                handled = false;
                continue;
            }
            if handled {
                continue;
            }
            handled = true;

            let next = match ctx.playlist.next_on_eof() {
                Some(path) => Some(path),
                // Repeat-one should also loop a file the user opened
                // directly, which never entered the playlist.
                None if ctx.playlist.repeat_mode() == RepeatMode::One => ctx.player.status().file,
                None => None,
            };

            if let Some(path) = next {
                let result = dispatch_command(&ctx, "play", &json!({ "file": path }));
                if !result.success {
                    eprintln!("[unflick] autoplay failed: {}", result.message);
                }
            }
        }
    });
}

fn handle_client(stream: TcpStream, ctx: Arc<ControlContext>) {
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

        let result = dispatch_command(&ctx, cmd, args);
        let json = serde_json::to_string(&result).unwrap();
        let _ = writeln!(writer, "{}", json);
    }
}

fn dispatch_command(ctx: &ControlContext, cmd: &str, args: &Value) -> CommandResult {
    let player = &ctx.player;
    let playlist: &Playlist = &ctx.playlist;
    let db: &Database = &ctx.db;

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
                let _ = db.remember_position(
                    current_file,
                    current_status.position,
                    current_status.duration,
                );
            }

            // A scheme mpv has no protocol for fails with nothing useful, and
            // `smb://` — what people type for a share, and what VLC takes — is
            // the common case. Catch it here so the answer can say what to do
            // instead of what went wrong.
            if let Some(scheme) = source::scheme_of(file) {
                let supported = player.supported_protocols();
                if !supported.iter().any(|p| p == &scheme) {
                    return CommandResult::err(source::unsupported_message(
                        file, &scheme, &supported,
                    ));
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
                // CLI / MCP have no per-call dialog override, so the
                // daemon resolves quality + cookies_browser straight from
                // saved settings. The GUI path still handles per-call
                // overrides in gui::commands::extract_stream_url before
                // it reaches us.
                let q = crate::core::settings::preferred_quality();
                let cb = crate::core::settings::cookies_browser();
                let result = rt.block_on(crate::core::yt_dlp::extract_stream_url(
                    &yt_dlp,
                    &url_owned,
                    proxy_owned.as_deref(),
                    q.as_deref(),
                    cb.as_deref(),
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
                Ok(outcome) => {
                    // URL play path: kick off SponsorBlock fetch + auto-
                    // subtitle download via the post-play hook. The hook
                    // returns immediately (everything spawned on tokio
                    // background tasks); auto-skip itself runs in the
                    // polling loop set up at app startup in `lib.rs`.
                    if crate::core::yt_dlp::is_http_url(file) {
                        let yt_dlp_path = crate::core::yt_dlp::find_yt_dlp();
                        let settings = crate::core::url_post_play::read_settings_snapshot();
                        crate::core::url_post_play::after_play_url_hooks(
                            Arc::clone(player),
                            file.to_string(),
                            yt_dlp_path,
                            settings,
                        );
                    }
                    // History is written here rather than being left to
                    // each caller: a play is a play whether it came from
                    // the window, a script, or an agent.
                    if !ctx.incognito.load(std::sync::atomic::Ordering::Relaxed) {
                        let _ = db.record_play(file);
                    }
                    // A source still opening after the load deadline is not a
                    // failure, but calling it "playing" would be a guess. Say
                    // which one it is; a script watching a network share needs
                    // to be able to tell.
                    let loaded = outcome == player::LoadOutcome::Loaded;
                    CommandResult::ok_with_data(
                        if loaded {
                            format!("playing {}", file)
                        } else {
                            format!("opening {} (still loading)", file)
                        },
                        json!({ "file": file, "loaded": loaded }),
                    )
                }
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
                let _ = db.remember_position(file, status.position, status.duration);
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
        // Absolute by default; `relative: true` nudges the current rate, which
        // is what the fine-tune buttons and the hotkeys send. A missing rate is
        // a read, matching `subtitle_delay` / `audio_delay`.
        "speed" => {
            let current = player.speed();

            let Some(rate) = args.get("rate").and_then(|v| v.as_f64()) else {
                return CommandResult::ok_with_data(
                    format!("{:.2}x", current),
                    json!({ "rate": current }),
                );
            };

            let relative = args.get("relative").and_then(|v| v.as_bool()).unwrap_or(false);
            // Clamping rather than erroring: holding the "slower" key down
            // should bottom out at 0.01x, not start reporting failures.
            let target = if relative {
                (current + rate).clamp(player::SPEED_MIN, player::SPEED_MAX)
            } else {
                rate
            };

            match player.set_speed(target) {
                Ok(()) => CommandResult::ok_with_data(
                    format!("{:.2}x", target),
                    json!({ "rate": target }),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "status" => {
            let status = player.status();
            CommandResult::ok_with_data("ok", serde_json::to_value(&status).unwrap())
        }
        // What a person would call the thing that's playing, as opposed to
        // what `status` reports. Cover extraction costs an ffmpeg run, so
        // it's opt-in — the GUI asks for it once per file, a poll doesn't.
        "nowplaying" => {
            let with_cover = args
                .get("cover")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let np = nowplaying::now_playing(player, with_cover);
            let label = match (&np.artist, &np.title) {
                (Some(a), Some(t)) => format!("{} — {}", a, t),
                (None, Some(t)) => t.clone(),
                _ => "nothing playing".to_string(),
            };
            CommandResult::ok_with_data(label, serde_json::to_value(&np).unwrap())
        }
        // Reading works without a window (there is one answer: normal);
        // changing it does not, and saying "ok" to a resize nothing
        // performed would be the same lie `play` used to tell.
        "window_mode" => {
            // Bad input is reported before a missing window, so a typo is
            // always named as a typo — "no window" would send someone
            // looking at the wrong problem.
            let requested = match args.get("mode").and_then(|v| v.as_str()) {
                Some(m) => match m.parse::<WindowMode>() {
                    Ok(mode) => Some(mode),
                    Err(e) => return CommandResult::err(e),
                },
                None => None,
            };

            let Some(host) = ctx.window.as_ref() else {
                return CommandResult::err(
                    "no window — window modes need the unflick GUI running",
                );
            };

            let Some(mode) = requested else {
                let mode = host.mode();
                return CommandResult::ok_with_data(
                    mode.as_str(),
                    json!({ "mode": mode.as_str() }),
                );
            };

            match host.set_mode(mode) {
                Ok(()) => CommandResult::ok_with_data(
                    mode.as_str(),
                    json!({ "mode": mode.as_str() }),
                ),
                Err(e) => CommandResult::err(e),
            }
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
                        Ok(_) => CommandResult::ok(format!("playing next: {}", path)),
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
                        Ok(_) => CommandResult::ok(format!("playing previous: {}", path)),
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
                        Ok(_) => CommandResult::ok(format!("playing index {}: {}", index, path)),
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
        // --- audio processing (v0.12) ---------------------------------
        "audio_eq_get" => {
            let settings = player.audio_settings();
            CommandResult::ok_with_data("ok", audio_state_json(&settings, player))
        }
        "audio_eq_set" => {
            // Two shapes on purpose: `band`+`gain` tweaks one slider (the
            // hot path, and the one that avoids a filter-graph rebuild),
            // while the named toggles change the chain's shape.
            let mut next = player.audio_settings();
            let mut single_band: Option<(usize, f64)> = None;

            if let Some(idx) = args.get("band").and_then(|v| v.as_i64()) {
                let index = match crate::core::audio::parse_band(idx) {
                    Ok(i) => i,
                    Err(e) => return CommandResult::err(e.to_string()),
                };
                let gain = match args.get("gain").and_then(|v| v.as_f64()) {
                    Some(g) => g,
                    None => return CommandResult::err("gain required when setting a band"),
                };
                single_band = Some((index, gain));
            }

            let mut shape_changed = false;
            if let Some(on) = args.get("enabled").and_then(|v| v.as_bool()) {
                next.equalizer = on;
                shape_changed = true;
            }
            if let Some(on) = args.get("normalize").and_then(|v| v.as_bool()) {
                next.normalize = on;
                shape_changed = true;
            }
            if let Some(p) = args.get("preamp").and_then(|v| v.as_f64()) {
                next.preamp = p;
                shape_changed = true;
            }
            if let Some(list) = args.get("bands").and_then(|v| v.as_array()) {
                next.bands = list.iter().filter_map(|v| v.as_f64()).collect();
                shape_changed = true;
            }

            if single_band.is_none() && !shape_changed {
                return CommandResult::err(
                    "nothing to set: pass band+gain, bands, enabled, normalize, or preamp",
                );
            }

            // Shape first, then the band: a band set on a chain that is about
            // to be rebuilt would be overwritten by the rebuild.
            if shape_changed {
                if let Err(e) = player.set_audio_settings(next) {
                    return CommandResult::err(e.to_string());
                }
            }
            if let Some((index, gain)) = single_band {
                if let Err(e) = player.set_band(index, gain) {
                    return CommandResult::err(e.to_string());
                }
            }

            let settings = player.audio_settings();
            CommandResult::ok_with_data(
                describe_audio(&settings),
                audio_saved_json(&settings, player),
            )
        }
        "audio_eq_preset" => {
            let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if name.is_empty() {
                return CommandResult::err("preset name required");
            }
            match player.set_audio_preset(name) {
                Ok(settings) => CommandResult::ok_with_data(
                    format!("preset: {}", name.trim().to_ascii_lowercase()),
                    audio_saved_json(&settings, player),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "audio_eq_presets" => CommandResult::ok_with_data(
            "ok",
            json!(crate::core::audio::PRESETS
                .iter()
                .map(|p| json!({
                    "name": p.name,
                    "description": p.description,
                    "bands": p.bands,
                }))
                .collect::<Vec<_>>()),
        ),
        "audio_eq_reset" => match player.reset_audio() {
            Ok(settings) => CommandResult::ok_with_data(
                "audio filters cleared",
                audio_saved_json(&settings, player),
            ),
            Err(e) => CommandResult::err(e.to_string()),
        },
        "audio_pitch" => {
            if let Some(on) = args.get("enabled").and_then(|v| v.as_bool()) {
                if let Err(e) = player.set_pitch_correction(on) {
                    return CommandResult::err(e.to_string());
                }
            }
            let on = player.pitch_correction();
            CommandResult::ok_with_data(
                if on {
                    "pitch correction on - speed changes keep the original pitch"
                } else {
                    "pitch correction off - speed changes shift the pitch"
                },
                json!({ "enabled": on }),
            )
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
        // --- online subtitles (OpenSubtitles) -------------------------
        //
        // Three entry points over two operations, because "search" and
        // "download" are what a UI needs (show a list, act on a choice)
        // while "auto" is what a person at a terminal or an agent needs
        // (just get me subtitles for this). `auto` is a composition, not a
        // third code path.
        "subtitle_search" => subtitle_search(player, args),
        "subtitle_download" => subtitle_download(player, args),
        "subtitle_auto" => subtitle_auto(player, args),
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
        "sponsor_segments" => {
            let url = args["url"].as_str().unwrap_or("");
            if url.is_empty() {
                return CommandResult::err("url is required");
            }
            // Build a single-thread tokio runtime per call. The daemon TCP
            // server is sync and we don't keep a long-lived runtime around;
            // a one-shot here is fine since fetch_segments is a single
            // network round-trip with a 5s timeout.
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(r) => r,
                Err(e) => return CommandResult::err(format!("tokio runtime: {}", e)),
            };
            let snapshot = crate::core::url_post_play::read_settings_snapshot();
            let cats = snapshot.sponsorblock_categories.clone();
            let cats_for_async = cats.clone();
            let url_owned = url.to_string();
            match rt.block_on(async move {
                crate::core::url_post_play::fetch_segments_for_url(&url_owned, &cats_for_async).await
            }) {
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
        // ─── Subtitle / audio timing ──────────────────────────────────────
        // Both accept an absolute value, or `relative: true` to nudge the
        // current one — the hotkeys send relative, the settings UI absolute.
        "subtitle_delay" | "audio_delay" => {
            let is_sub = cmd == "subtitle_delay";
            let current = if is_sub { player.sub_delay() } else { player.audio_delay() };

            let Some(seconds) = args.get("seconds").and_then(|v| v.as_f64()) else {
                // No value supplied: this is a read.
                return CommandResult::ok_with_data(
                    format!("{:+.3}s", current),
                    json!({ "seconds": current }),
                );
            };

            let relative = args.get("relative").and_then(|v| v.as_bool()).unwrap_or(false);
            let target = if relative { current + seconds } else { seconds };

            let applied = if is_sub {
                player.set_sub_delay(target)
            } else {
                player.set_audio_delay(target)
            };
            match applied {
                Ok(()) => CommandResult::ok_with_data(
                    format!("{:+.3}s", target),
                    json!({ "seconds": target }),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }

        // ─── Chapters ─────────────────────────────────────────────────────
        "chapter_list" => {
            let chapters = player.chapter_list();
            CommandResult::ok_with_data(
                format!("{} chapter(s)", chapters.len()),
                json!(chapters),
            )
        }
        "chapter_seek" => {
            let Some(index) = args.get("index").and_then(|v| v.as_i64()) else {
                return CommandResult::err("index is required");
            };
            match player.chapter_seek(index) {
                Ok(()) => CommandResult::ok_with_data(
                    format!("chapter {}", index),
                    json!({ "index": index }),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "chapter_next" | "chapter_prev" => {
            let delta = if cmd == "chapter_next" { 1 } else { -1 };
            match player.chapter_step(delta) {
                Ok(index) => CommandResult::ok_with_data(
                    format!("chapter {}", index),
                    json!({ "index": index }),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }

        // ─── A-B loop ─────────────────────────────────────────────────────
        "ab_loop" => {
            let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("status");
            // Omitting `position` means "use where playback is right now",
            // which is what the [ and ] hotkeys want.
            let position = args.get("position").and_then(|v| v.as_f64());

            let result = match action {
                "status" => Ok(player.ab_loop_status()),
                "a" | "set_a" => player.ab_loop_set_a(position),
                "b" | "set_b" => player.ab_loop_set_b(position),
                "clear" => player.ab_loop_clear().map(|()| player.ab_loop_status()),
                other => {
                    return CommandResult::err(format!(
                        "unknown ab_loop action: {} (expected a | b | clear | status)",
                        other
                    ))
                }
            };

            match result {
                Ok(state) => {
                    let msg = match (state.a, state.b) {
                        (Some(a), Some(b)) => format!("looping {:.2}s → {:.2}s", a, b),
                        (Some(a), None) => format!("A set at {:.2}s", a),
                        (None, Some(b)) => format!("B set at {:.2}s", b),
                        (None, None) => "loop cleared".to_string(),
                    };
                    CommandResult::ok_with_data(msg, json!(state))
                }
                Err(e) => CommandResult::err(e.to_string()),
            }
        }

        // ─── Frame stepping ───────────────────────────────────────────────
        "frame_step" | "frame_back_step" => {
            let stepped = if cmd == "frame_step" {
                player.frame_step()
            } else {
                player.frame_back_step()
            };
            match stepped {
                Ok(()) => CommandResult::ok_with_data(
                    format!("{:.3}s", player.status().position),
                    json!({ "position": player.status().position }),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }

        // ─── Subtitle styling ─────────────────────────────────────────────
        "subtitle_style_get" => {
            CommandResult::ok_with_data("subtitle style", player.subtitle_style())
        }
        "subtitle_style_set" => {
            let Some(name) = args.get("name").and_then(|v| v.as_str()) else {
                return CommandResult::err("name is required");
            };
            let value = args.get("value").cloned().unwrap_or(Value::Null);
            match player.set_subtitle_style(name, &value) {
                Ok(()) => CommandResult::ok_with_data(
                    format!("{} = {}", name, value),
                    player.subtitle_style(),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }

        // ─── Playlist repeat / shuffle ────────────────────────────────────
        "playlist_repeat" => {
            if let Some(mode) = args.get("mode").and_then(|v| v.as_str()) {
                match RepeatMode::parse(mode) {
                    Some(m) => playlist.set_repeat_mode(m),
                    None => {
                        return CommandResult::err(format!(
                            "unknown repeat mode: {} (expected off | one | all)",
                            mode
                        ))
                    }
                }
            }
            let mode = playlist.repeat_mode();
            CommandResult::ok_with_data(
                format!("repeat {}", mode.as_str()),
                json!({ "mode": mode.as_str() }),
            )
        }
        "playlist_shuffle" => {
            if let Some(enabled) = args.get("enabled").and_then(|v| v.as_bool()) {
                playlist.set_shuffle(enabled);
            }
            let enabled = playlist.shuffle_enabled();
            CommandResult::ok_with_data(
                format!("shuffle {}", if enabled { "on" } else { "off" }),
                json!({ "enabled": enabled }),
            )
        }

        // ─── Transcript (v0.10 Phase 2) ───────────────────────────────────
        // These are the tools an external wrapper can't offer: they read the
        // subtitle track the player already has open.
        "transcript_get" => match crate::core::transcript::load_current(player) {
            Ok(t) => CommandResult::ok_with_data(
                format!("{} cue(s) from {}", t.cues.len(), t.origin),
                json!(t),
            ),
            Err(e) => CommandResult::err(e.to_string()),
        },
        "transcript_search" => {
            let Some(query) = args.get("query").and_then(|v| v.as_str()) else {
                return CommandResult::err("query is required");
            };
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(20)
                .clamp(1, 200) as usize;

            let transcript = match crate::core::transcript::load_current(player) {
                Ok(t) => t,
                Err(e) => return CommandResult::err(e.to_string()),
            };
            let hits = crate::core::transcript::search(&transcript.cues, query, limit);
            CommandResult::ok_with_data(
                format!("{} match(es) for {:?}", hits.len(), query),
                json!({
                    "query": query,
                    "source": transcript.source,
                    "origin": transcript.origin,
                    "matches": hits,
                }),
            )
        }
        "transcript_seek" => {
            let Some(query) = args.get("query").and_then(|v| v.as_str()) else {
                return CommandResult::err("query is required");
            };
            // 1-based, so "the 2nd time they mention it" reads naturally.
            let occurrence = args
                .get("occurrence")
                .and_then(|v| v.as_u64())
                .unwrap_or(1)
                .max(1) as usize;

            let transcript = match crate::core::transcript::load_current(player) {
                Ok(t) => t,
                Err(e) => return CommandResult::err(e.to_string()),
            };
            let hits = crate::core::transcript::search(&transcript.cues, query, occurrence);
            let Some(cue) = hits.get(occurrence - 1) else {
                return CommandResult::err(format!(
                    "no match #{} for {:?} ({} found)",
                    occurrence,
                    query,
                    hits.len()
                ));
            };

            // Land slightly before the line so the sentence is heard from
            // its start rather than mid-word.
            let target = (cue.start - 0.5).max(0.0);
            match player.seek(target) {
                Ok(()) => CommandResult::ok_with_data(
                    format!("{:.2}s — {}", cue.start, cue.text),
                    json!({ "position": target, "cue": cue }),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }

        // ─── Chapter generation ───────────────────────────────────────────
        "chapters_generate" => {
            let target = args
                .get("count")
                .and_then(|v| v.as_u64())
                .unwrap_or(8)
                .clamp(2, 50) as usize;

            let transcript = match crate::core::transcript::load_current(player) {
                Ok(t) => t,
                Err(e) => return CommandResult::err(e.to_string()),
            };
            let duration = player.status().duration;
            let derived =
                crate::core::transcript::derive_chapters(&transcript.cues, target, duration);
            if derived.is_empty() {
                return CommandResult::err(
                    "could not derive chapters: the transcript is too short or has no pauses",
                );
            }
            match player.set_virtual_chapters(derived) {
                Ok(count) => CommandResult::ok_with_data(
                    format!("generated {} chapter(s)", count),
                    json!(player.chapter_list()),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "chapters_set" => {
            let Some(items) = args.get("chapters").and_then(|v| v.as_array()) else {
                return CommandResult::err("chapters must be an array of {time, title}");
            };
            let mut entries = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                let Some(time) = item.get("time").and_then(|v| v.as_f64()) else {
                    return CommandResult::err(format!("chapter {} has no numeric `time`", i));
                };
                let title = item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Chapter")
                    .to_string();
                entries.push((time, title));
            }
            match player.set_virtual_chapters(entries) {
                Ok(count) => CommandResult::ok_with_data(
                    format!("set {} chapter(s)", count),
                    json!(player.chapter_list()),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "chapters_clear" => {
            player.clear_virtual_chapters();
            CommandResult::ok("cleared generated chapters")
        }

        // ─── Vision ───────────────────────────────────────────────────────
        "describe_frame" => {
            let seek_to = args.get("position").and_then(|v| v.as_f64());
            let max_edge = args.get("max_edge").and_then(|v| v.as_u64()).map(|v| v as u32);
            let frame = match crate::core::vision::capture_frame(player, seek_to, max_edge) {
                Ok(f) => f,
                Err(e) => return CommandResult::err(e.to_string()),
            };

            // With `output`, write the file and report the path — that's the
            // CLI's shape. Without it, hand back base64 for MCP to wrap in an
            // image content block. Printing a megabyte of base64 into a
            // terminal helps nobody.
            if let Some(path) = args.get("output").and_then(|v| v.as_str()) {
                if let Err(e) = frame.write_to(path) {
                    return CommandResult::err(e.to_string());
                }
                return CommandResult::ok_with_data(
                    format!("frame at {:.2}s → {}", frame.position, path),
                    json!({
                        "position": frame.position,
                        "mime_type": frame.mime_type,
                        "bytes": frame.bytes.len(),
                        "path": path,
                    }),
                );
            }

            CommandResult::ok_with_data(
                format!("frame at {:.2}s ({} bytes)", frame.position, frame.bytes.len()),
                json!({
                    "position": frame.position,
                    "mime_type": frame.mime_type,
                    "bytes": frame.bytes.len(),
                    "base64": frame.to_base64(),
                }),
            )
        }

        // ─── Keyboard bindings ────────────────────────────────────────────
        // Settings-file operations, so they work with or without a player.
        "keybind_list" => match crate::core::keybind::list() {
            Ok(rows) => {
                let n = rows.as_array().map(|a| a.len()).unwrap_or(0);
                CommandResult::ok_with_data(format!("{} action(s)", n), rows)
            }
            Err(e) => CommandResult::err(e.to_string()),
        },
        "keybind_set" => {
            let Some(action) = args.get("action").and_then(|v| v.as_str()) else {
                return CommandResult::err("action is required");
            };
            let Some(key) = args.get("key").and_then(|v| v.as_str()) else {
                return CommandResult::err("key is required");
            };
            match crate::core::keybind::set(action, key) {
                Ok(normalized) => CommandResult::ok_with_data(
                    format!("{} → {}", action, normalized),
                    json!({ "action": action, "key": normalized }),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "keybind_reset" => {
            let action = args.get("action").and_then(|v| v.as_str());
            match crate::core::keybind::reset(action) {
                Ok(count) => CommandResult::ok_with_data(
                    match action {
                        Some(a) if count > 0 => format!("{} reset to its default", a),
                        Some(a) => format!("{} was already at its default", a),
                        None => format!("{} binding(s) reset", count),
                    },
                    json!({ "reset": count }),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }

        "record_play" => {
            let Some(path) = args.get("path").and_then(|v| v.as_str()) else {
                return CommandResult::err("path is required");
            };
            if ctx.incognito.load(std::sync::atomic::Ordering::Relaxed) {
                return CommandResult::ok("incognito is on; not recorded");
            }
            match db.record_play(path) {
                Ok(()) => CommandResult::ok(format!("recorded {}", path)),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "incognito" => {
            use std::sync::atomic::Ordering;
            if let Some(enabled) = args.get("enabled").and_then(|v| v.as_bool()) {
                ctx.incognito.store(enabled, Ordering::Relaxed);
            }
            let enabled = ctx.incognito.load(Ordering::Relaxed);
            CommandResult::ok_with_data(
                format!("incognito {}", if enabled { "on" } else { "off" }),
                json!({ "enabled": enabled }),
            )
        }

        // ─── Picture geometry ─────────────────────────────────────────────
        "video_get" => CommandResult::ok_with_data("video transform", player.video_transform()),
        "video_set" => {
            let Some(name) = args.get("name").and_then(|v| v.as_str()) else {
                return CommandResult::err("name is required");
            };
            let value = args.get("value").cloned().unwrap_or(Value::Null);
            match player.set_video_transform(name, &value) {
                Ok(()) => CommandResult::ok_with_data(
                    format!("{} = {}", name, value),
                    player.video_transform(),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "video_reset" => match player.reset_video_transform() {
            Ok(()) => CommandResult::ok_with_data("video transform reset", player.video_transform()),
            Err(e) => CommandResult::err(e.to_string()),
        },

        // ─── Recently played ──────────────────────────────────────────────
        "recent_list" => {
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(20)
                .clamp(1, 200) as usize;
            match db.recent(limit) {
                Ok(entries) => CommandResult::ok_with_data(
                    format!("{} recently played", entries.len()),
                    json!(entries),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "recent_clear" => match db.clear_recent() {
            Ok(n) => CommandResult::ok_with_data(
                format!("cleared {} history entr(ies)", n),
                json!({ "cleared": n }),
            ),
            Err(e) => CommandResult::err(e.to_string()),
        },

        // ─── Bookmarks ────────────────────────────────────────────────────
        //
        // Bookmarks are written even in incognito mode. Incognito suppresses
        // the history that accumulates on its own; a bookmark is something
        // the user asked for by name, and silently discarding it would look
        // like the feature is broken.
        "bookmark_add" => {
            let status = player.status();
            let path = match args.get("file").and_then(|v| v.as_str()) {
                Some(f) => f.to_string(),
                None => match status.file.clone() {
                    Some(f) => f,
                    None => return CommandResult::err(NOTHING_PLAYING),
                },
            };
            // Position defaults to where playback is, but only when the
            // bookmark is for the file that's playing — "now" means nothing
            // for some other file, and 0 would be a lie dressed as a value.
            let position = match args.get("position").and_then(|v| v.as_f64()) {
                Some(p) => p,
                None if Some(&path) == status.file.as_ref() => status.position,
                None => {
                    return CommandResult::err(
                        "position is required when bookmarking a file that isn't playing",
                    )
                }
            };
            let name = args
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty());

            match db.add_bookmark(&path, position, name) {
                Ok(b) => CommandResult::ok_with_data(
                    format!("bookmark {} at {}", b.id, format_timestamp(b.position)),
                    json!(b),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "bookmark_list" => {
            let path = match bookmark_scope(player, args) {
                Ok(p) => p,
                Err(e) => return e,
            };
            match db.list_bookmarks(path.as_deref()) {
                Ok(list) => CommandResult::ok_with_data(
                    format!("{} bookmark(s)", list.len()),
                    json!(list),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "bookmark_goto" => {
            let Some(id) = args.get("id").and_then(|v| v.as_i64()) else {
                return CommandResult::err("id is required");
            };
            let bookmark = match db.get_bookmark(id) {
                Ok(Some(b)) => b,
                Ok(None) => return CommandResult::err(format!("no bookmark with id {}", id)),
                Err(e) => return CommandResult::err(e.to_string()),
            };

            // Already on the right file: a seek, not a reload. Reloading
            // would blank the window and lose the audio/subtitle track the
            // user picked, to arrive at the same timestamp.
            if player.status().file.as_deref() == Some(bookmark.path.as_str()) {
                return match player.seek(bookmark.position) {
                    Ok(()) => CommandResult::ok_with_data(
                        format!("jumped to {}", describe_bookmark(&bookmark)),
                        json!(bookmark),
                    ),
                    Err(e) => CommandResult::err(e.to_string()),
                };
            }

            // Different file: go through `play` so the outgoing file's
            // resume point, history and yt-dlp resolution all behave exactly
            // as they do for any other way of opening a file.
            let mut result = dispatch_command(
                ctx,
                "play",
                &json!({ "file": bookmark.path, "seek": bookmark.position }),
            );
            if result.success {
                result.message = format!("opened {}", describe_bookmark(&bookmark));
                result.data = Some(json!(bookmark));
            }
            result
        }
        "bookmark_rename" => {
            let Some(id) = args.get("id").and_then(|v| v.as_i64()) else {
                return CommandResult::err("id is required");
            };
            let name = args
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty());
            match db.rename_bookmark(id, name) {
                Ok(b) => CommandResult::ok_with_data(
                    format!("bookmark {} is now {}", b.id, describe_bookmark(&b)),
                    json!(b),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "bookmark_remove" => {
            let Some(id) = args.get("id").and_then(|v| v.as_i64()) else {
                return CommandResult::err("id is required");
            };
            match db.remove_bookmark(id) {
                Ok(true) => CommandResult::ok_with_data(
                    format!("removed bookmark {}", id),
                    json!({ "removed": id }),
                ),
                Ok(false) => CommandResult::err(format!("no bookmark with id {}", id)),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "bookmark_clear" => {
            let path = match bookmark_scope(player, args) {
                Ok(p) => p,
                Err(e) => return e,
            };
            match db.clear_bookmarks(path.as_deref()) {
                Ok(n) => CommandResult::ok_with_data(
                    format!("cleared {} bookmark(s)", n),
                    json!({ "cleared": n }),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }

        // ─── Mouse bindings ───────────────────────────────────────────────
        "mouse_list" => match crate::core::mousebind::list() {
            Ok(rows) => {
                let n = rows.as_array().map(|a| a.len()).unwrap_or(0);
                CommandResult::ok_with_data(format!("{} trigger(s)", n), rows)
            }
            Err(e) => CommandResult::err(e.to_string()),
        },
        "mouse_set" => {
            let Some(trigger) = args.get("trigger").and_then(|v| v.as_str()) else {
                return CommandResult::err("trigger is required");
            };
            let Some(action) = args.get("action").and_then(|v| v.as_str()) else {
                return CommandResult::err("action is required");
            };
            match crate::core::mousebind::set(trigger, action) {
                Ok(a) => CommandResult::ok_with_data(
                    format!("{} → {}", trigger, a),
                    json!({ "trigger": trigger, "action": a }),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }
        "mouse_reset" => {
            let trigger = args.get("trigger").and_then(|v| v.as_str());
            match crate::core::mousebind::reset(trigger) {
                Ok(count) => CommandResult::ok_with_data(
                    match trigger {
                        Some(t) if count > 0 => format!("{} reset to its default", t),
                        Some(t) => format!("{} was already at its default", t),
                        None => format!("{} binding(s) reset", count),
                    },
                    json!({ "reset": count }),
                ),
                Err(e) => CommandResult::err(e.to_string()),
            }
        }

        // ─── Timeline previews ────────────────────────────────────────────
        "thumbnail" => {
            let Some(position) = args.get("position").and_then(|v| v.as_f64()) else {
                return CommandResult::err("position is required");
            };
            let width = args
                .get("width")
                .and_then(|v| v.as_u64())
                .unwrap_or(160) as u32;

            let status = player.status();
            let Some(file) = status.file else {
                return CommandResult::err("nothing is playing");
            };

            let thumb = match crate::core::thumbnail::thumbnail_at(
                &file,
                position,
                status.duration,
                width,
            ) {
                Ok(t) => t,
                Err(e) => return CommandResult::err(e.to_string()),
            };

            // Same split as `describe_frame`: a path for the CLI, base64
            // for callers that want the bytes inline.
            if let Some(out) = args.get("output").and_then(|v| v.as_str()) {
                if let Err(e) = std::fs::write(out, &thumb.bytes) {
                    return CommandResult::err(format!("failed to write {}: {}", out, e));
                }
                return CommandResult::ok_with_data(
                    format!("preview at {:.1}s → {}", thumb.bucket_seconds, out),
                    json!({
                        "position": thumb.bucket_seconds,
                        "bytes": thumb.bytes.len(),
                        "path": out,
                    }),
                );
            }

            CommandResult::ok_with_data(
                format!("preview at {:.1}s ({} bytes)", thumb.bucket_seconds, thumb.bytes.len()),
                json!({
                    "position": thumb.bucket_seconds,
                    "bytes": thumb.bytes.len(),
                    "mime_type": "image/jpeg",
                    "base64": crate::core::vision::base64_encode(&thumb.bytes),
                }),
            )
        }

        "shutdown" => {
            if ctx.embedded {
                // Hosted by the GUI. Killing the process here would close
                // the user's window out from under them, so decline and say
                // why — `unflick shutdown` is for the headless daemon.
                return CommandResult::err(
                    "control port is held by the unflick GUI; close the window to stop it",
                );
            }
            std::process::exit(0);
        }
        _ => CommandResult::err(format!("unknown command: {}", cmd)),
    }
}

/// Send a command to the running daemon. Returns the response.
pub fn send_to_daemon(cmd: &str, args: Value) -> Result<CommandResult, String> {
    let mut stream = TcpStream::connect(control_addr())
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
    TcpStream::connect(control_addr()).is_ok()
}

/// Ask whoever holds the control port to step aside, then wait for it to
/// free up. Used by the GUI at startup: a headless daemon started earlier
/// by a CLI call would otherwise keep serving its invisible `vo=null`
/// player while the user stares at a window that ignores `unflick pause`.
///
/// An embedded (GUI-hosted) server declines the shutdown, so a second GUI
/// instance simply leaves the first one in charge. Returns true when the
/// port is free to bind.
pub fn request_port_handover() -> bool {
    if !is_daemon_running() {
        return true;
    }
    if let Ok(res) = send_to_daemon("shutdown", json!({})) {
        // An embedded host answers with success=false and keeps the port.
        if !res.success {
            return false;
        }
    }
    // The daemon exits without replying, so a transport error here is the
    // expected path. Poll for the socket to actually close.
    for _ in 0..20 {
        if !is_daemon_running() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

// --- bookmarks -------------------------------------------------------------

const NOTHING_PLAYING: &str = "nothing is playing — pass a file";

/// Which file a `bookmark list` / `bookmark clear` applies to.
///
/// `Ok(None)` means every file, asked for explicitly with `all`. Defaulting
/// to every file when nothing is playing would make `bookmark clear` delete
/// the lot on a mistimed call, so the wide scope is never reached by
/// accident — it has to be named.
fn bookmark_scope(player: &Player, args: &Value) -> Result<Option<String>, CommandResult> {
    if args.get("all").and_then(|v| v.as_bool()).unwrap_or(false) {
        return Ok(None);
    }
    if let Some(file) = args.get("file").and_then(|v| v.as_str()) {
        return Ok(Some(file.to_string()));
    }
    match player.status().file {
        Some(f) => Ok(Some(f)),
        None => Err(CommandResult::err(format!("{} or all", NOTHING_PLAYING))),
    }
}

/// `1:23` / `1:02:03` — how a timestamp reads to a person, for the one-line
/// message. The structured `data` keeps the raw seconds.
fn format_timestamp(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{}:{:02}:{:02}", h, m, s)
    } else {
        format!("{}:{:02}", m, s)
    }
}

fn describe_bookmark(b: &crate::db::Bookmark) -> String {
    match &b.name {
        Some(name) => format!("\"{}\" ({})", name, format_timestamp(b.position)),
        None => format_timestamp(b.position),
    }
}

/// Default output directory for AI-generated subtitle files (matches GUI behavior).
// --- audio processing ------------------------------------------------------

/// The audio state as the wire sees it: the settings, plus the band
/// frequencies so a caller can label sliders without hardcoding our table,
/// plus pitch correction, which lives in mpv rather than in our state.
/// Persist the audio state, then shape it for the wire.
///
/// Saving here rather than in `Player` keeps `core::player` free of settings
/// I/O, and puts every mutation through one place - an equaliser that resets
/// on restart is the kind of bug that only shows up a day later.
fn audio_saved_json(settings: &AudioSettings, player: &Player) -> Value {
    if let Err(e) = crate::core::audio::save(settings) {
        eprintln!("[unflick] could not persist audio settings: {}", e);
    }
    audio_state_json(settings, player)
}

fn audio_state_json(settings: &AudioSettings, player: &Player) -> Value {
    json!({
        "enabled": settings.equalizer,
        "bands": settings.bands,
        "frequencies": crate::core::audio::BANDS,
        "preamp": settings.preamp,
        "normalize": settings.normalize,
        "flat": settings.is_flat(),
        "max_gain": crate::core::audio::MAX_GAIN_DB,
        "pitch_correction": player.pitch_correction(),
        // What mpv is actually running, for when it disagrees with the above.
        "chain": player.audio_chain(),
    })
}

/// A one-line summary for the CLI's `message`, since a wall of ten gains
/// tells a human nothing about what just changed.
fn describe_audio(s: &AudioSettings) -> String {
    let mut parts = Vec::new();
    parts.push(if !s.equalizer {
        "equalizer off".to_string()
    } else if s.is_flat() {
        "equalizer on (flat)".to_string()
    } else {
        format!(
            "equalizer on ({})",
            s.bands
                .iter()
                .enumerate()
                .filter(|(_, g)| g.abs() > 0.0)
                .map(|(i, g)| format!("{}Hz {:+}", crate::core::audio::BANDS[i], g))
                .collect::<Vec<_>>()
                .join(", ")
        )
    });
    if s.preamp.abs() > 0.0 {
        parts.push(format!("preamp {:+} dB", s.preamp));
    }
    if s.normalize {
        parts.push("normalize on".to_string());
    }
    parts.join(", ")
}

// --- online subtitles ------------------------------------------------------
//
// Thin wrappers over `core::opensubtitles`: resolve "the playing file" as the
// default target, hand off, and load whatever came back. The GUI calls the
// same core functions directly, so nothing here is logic anyone else needs.

/// Default the search target to whatever is playing.
///
/// The overwhelmingly common case is "subtitles for this, now"; an explicit
/// path still works for scripting against files that aren't loaded.
fn subtitle_request(player: &Player, args: &Value) -> opensubtitles::SearchRequest {
    opensubtitles::SearchRequest {
        query: args
            .get("query")
            .and_then(|v| v.as_str())
            .map(String::from),
        file: args
            .get("file")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| player.status().file),
        languages: args
            .get("languages")
            .and_then(|v| v.as_str())
            .map(String::from),
        hash: args.get("hash").and_then(|v| v.as_bool()).unwrap_or(true),
    }
}

fn subtitle_search(player: &Player, args: &Value) -> CommandResult {
    match opensubtitles::run_search(&subtitle_request(player, args)) {
        Ok(outcome) => CommandResult::ok_with_data(
            format!("{} result(s)", outcome.results.len()),
            serde_json::to_value(&outcome).unwrap_or(Value::Null),
        ),
        Err(e) => CommandResult::err(e.to_string()),
    }
}

fn subtitle_download(player: &Player, args: &Value) -> CommandResult {
    let file_id = match args.get("file_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return CommandResult::err("file_id required"),
    };
    let video = args
        .get("file")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| player.status().file);

    let dl = opensubtitles::run_download(
        file_id,
        video.as_deref(),
        args.get("language").and_then(|v| v.as_str()).unwrap_or(""),
        args.get("name").and_then(|v| v.as_str()),
        std::path::Path::new(&default_subtitle_output_dir()),
    );
    match dl {
        Ok(dl) => {
            let load = args.get("load").and_then(|v| v.as_bool()).unwrap_or(true);
            CommandResult::ok_with_data(
                format!("{} ({} downloads left today)", dl.file_name, dl.remaining),
                downloaded_json(player, &dl, load, None),
            )
        }
        Err(e) => CommandResult::err(e.to_string()),
    }
}

fn subtitle_auto(player: &Player, args: &Value) -> CommandResult {
    let req = subtitle_request(player, args);
    let fallback = default_subtitle_output_dir();
    match opensubtitles::run_auto(&req, std::path::Path::new(&fallback)) {
        Ok((dl, best, outcome)) => {
            let load = args.get("load").and_then(|v| v.as_bool()).unwrap_or(true);
            CommandResult::ok_with_data(
                format!("{} ({} downloads left today)", dl.file_name, dl.remaining),
                downloaded_json(player, &dl, load, Some((&best, &outcome))),
            )
        }
        Err(e) => CommandResult::err(e.to_string()),
    }
}

/// Shape a finished download for the wire, loading it into the player first.
///
/// A failed load is reported rather than raised: the file is on disk and the
/// quota is already spent, so calling the whole thing an error would be
/// misleading about what actually happened.
fn downloaded_json(
    player: &Player,
    dl: &opensubtitles::Downloaded,
    load: bool,
    chosen: Option<(&opensubtitles::SubtitleResult, &opensubtitles::SearchOutcome)>,
) -> Value {
    let mut load_error: Option<String> = None;
    if load {
        if let Err(e) = player.subtitle_load(&dl.path) {
            load_error = Some(e.to_string());
        }
    }
    let mut out = json!({
        "path": dl.path,
        "file_name": dl.file_name,
        "requests": dl.requests,
        "remaining": dl.remaining,
        "reset_time": dl.reset_time,
        "loaded": load && load_error.is_none(),
        "load_error": load_error,
    });
    if let Some((best, outcome)) = chosen {
        out["moviehash_match"] = json!(best.moviehash_match);
        out["language"] = json!(best.language);
        out["release"] = json!(best.release);
        out["candidates"] = json!(outcome.results.len());
        out["query"] = json!(outcome.query);
    }
    out
}

fn default_subtitle_output_dir() -> String {
    dirs_next::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("unflick")
        .to_string_lossy()
        .into_owned()
}
