use serde_json::{json, Value};

use crate::core::daemon;

/// Handle an MCP tool call by routing to the daemon.
pub fn handle_tool_via_daemon(name: &str, args: &Value) -> Value {
    // `describe_frame` is the one tool whose result isn't text: it comes
    // back as an MCP image block so a multimodal model can actually look at
    // the frame. Handled ahead of the generic path, which stringifies
    // everything.
    if name == "describe_frame" {
        return handle_describe_frame(args);
    }

    // `cleanup` reads and deletes files; it has no business starting a
    // player to do it, and it has to work on the machine where the app is
    // too broken to start — which is exactly the machine someone is trying
    // to reclaim disk on. Same reason the CLI skips `ensure_daemon`.
    if name == "cleanup" {
        return handle_cleanup(args);
    }

    // `startup` reads a log file. Same reasoning: launching a player to ask
    // how long the last launch took would overwrite the answer.
    if name == "startup" {
        return handle_startup();
    }

    let (cmd, daemon_args) = match name {
        "cast" => {
            let mut a = json!({"action": args["action"].as_str().unwrap_or("status")});
            for key in ["renderer", "file"] {
                if let Some(v) = args.get(key).and_then(|v| v.as_str()) {
                    a[key] = json!(v);
                }
            }
            if let Some(v) = args.get("seconds").and_then(|v| v.as_f64()) {
                a["seconds"] = json!(v);
            }
            ("cast", a)
        }
        "disc_list" => {
            let mut a = json!({});
            if let Some(p) = args.get("path").and_then(|v| v.as_str()) {
                a["path"] = json!(p);
            }
            ("disc_list", a)
        }
        "session" => (
            "session",
            json!({"action": args["action"].as_str().unwrap_or("show")}),
        ),
        "play" => ("play", args.clone()),
        "pause" => ("pause", json!({})),
        "resume" => ("resume", json!({})),
        "stop" => ("stop", json!({})),
        "seek" => ("seek", json!({"seconds": args["seconds"]})),
        "set_volume" => ("volume", json!({"level": args["level"]})),
        "set_speed" => (
            "speed",
            json!({"rate": args["rate"], "relative": args["relative"]}),
        ),
        "get_status" => ("status", json!({})),
        "window_mode" => ("window_mode", json!({"mode": args["mode"]})),
        "now_playing" => ("nowplaying", json!({"cover": args["cover"]})),
        "file_info" => ("info", json!({"file": args["file"]})),
        "playlist_add" => ("playlist_add", json!({"file": args["file"]})),
        "playlist_remove" => ("playlist_remove", json!({"index": args["index"]})),
        "playlist_list" => ("playlist_list", json!({})),
        "playlist_next" => ("playlist_next", json!({})),
        "playlist_prev" => ("playlist_prev", json!({})),
        "playlist_clear" => ("playlist_clear", json!({})),
        "playlist_play" => ("playlist_play", json!({"index": args["index"]})),
        "load_subtitle" => ("subtitle_load", json!({"file": args["file"]})),
        "subtitle_list" => ("subtitle_list", json!({})),
        "subtitle_select" => ("subtitle_select", json!({"id": args["id"]})),
        "audio_list" => ("audio_list", json!({})),
        "audio_select" => ("audio_select", json!({"id": args["id"]})),
        "equalizer_get" => ("audio_eq_get", json!({})),
        "equalizer_set" => ("audio_eq_set", args.clone()),
        "equalizer_preset" => ("audio_eq_preset", args.clone()),
        "equalizer_presets" => ("audio_eq_presets", json!({})),
        "equalizer_reset" => ("audio_eq_reset", json!({})),
        "pitch_correction" => ("audio_pitch", args.clone()),
        "generate_subtitles" => ("subtitle_generate", args.clone()),
        "find_subtitles" => ("subtitle_search", args.clone()),
        "download_subtitle" => ("subtitle_download", args.clone()),
        "get_subtitles" => ("subtitle_auto", args.clone()),
        "translate_subtitles" => ("subtitle_translate", args.clone()),
        "settings_path" => ("settings_path", json!({})),
        "settings_get" => ("settings_get", args.clone()),
        "settings_set" => ("settings_set", args.clone()),
        "settings_unset" => ("settings_unset", args.clone()),
        "filter_list" => ("filter_list", json!({})),
        "filter_set" => ("filter_set", args.clone()),
        "filter_reset" => ("filter_reset", json!({})),
        "library_scan" => ("library_scan", json!({"dir": args["dir"]})),
        "library_search" => ("library_search", json!({"query": args["query"]})),
        "library_list" => ("library_list", json!({})),
        "library_remove" => ("library_remove", json!({"id": args["id"]})),
        "clip" => ("clip", args.clone()),
        "screenshot" => ("screenshot", json!({"output": args.get("output")})),
        "save_position" => ("save_position", json!({"path": args["path"], "position": args["position"]})),
        "get_position" => ("get_position", json!({"path": args["path"]})),
        "sponsor_segments" => ("sponsor_segments", json!({"url": args["url"]})),
        "subtitle_delay" => ("subtitle_delay", args.clone()),
        "audio_delay" => ("audio_delay", args.clone()),
        "subtitle_style_get" => ("subtitle_style_get", json!({})),
        "subtitle_style_set" => ("subtitle_style_set", args.clone()),
        "chapter_list" => ("chapter_list", json!({})),
        "chapter_seek" => ("chapter_seek", json!({"index": args["index"]})),
        "chapter_next" => ("chapter_next", json!({})),
        "chapter_prev" => ("chapter_prev", json!({})),
        "ab_loop" => ("ab_loop", args.clone()),
        "frame_step" => ("frame_step", json!({})),
        "frame_back_step" => ("frame_back_step", json!({})),
        "playlist_repeat" => ("playlist_repeat", args.clone()),
        "playlist_shuffle" => ("playlist_shuffle", args.clone()),
        "transcript_get" => ("transcript_get", json!({})),
        "search_transcript" => ("transcript_search", args.clone()),
        "seek_to_text" => ("transcript_seek", args.clone()),
        "generate_chapters" => ("chapters_generate", args.clone()),
        "set_chapters" => ("chapters_set", args.clone()),
        "clear_chapters" => ("chapters_clear", json!({})),
        "keybind_list" => ("keybind_list", json!({})),
        "keybind_set" => ("keybind_set", args.clone()),
        "keybind_reset" => ("keybind_reset", args.clone()),
        "mouse_list" => ("mouse_list", json!({})),
        "mouse_set" => ("mouse_set", args.clone()),
        "mouse_reset" => ("mouse_reset", args.clone()),
        "recent_files" => ("recent_list", args.clone()),
        "record_play" => ("record_play", args.clone()),
        "incognito" => ("incognito", args.clone()),
        "video_transform_get" => ("video_get", json!({})),
        "video_transform_set" => ("video_set", args.clone()),
        "video_transform_reset" => ("video_reset", json!({})),
        "recent_clear" => ("recent_clear", json!({})),
        "bookmark_add" => ("bookmark_add", args.clone()),
        "bookmark_list" => ("bookmark_list", args.clone()),
        "bookmark_goto" => ("bookmark_goto", args.clone()),
        "bookmark_rename" => ("bookmark_rename", args.clone()),
        "bookmark_remove" => ("bookmark_remove", args.clone()),
        "bookmark_clear" => ("bookmark_clear", args.clone()),
        "shutdown" => ("shutdown", json!({})),
        _ => {
            return tool_result(true, json!([{"type": "text", "text": format!("unknown tool: {}", name)}]));
        }
    };

    match daemon::send_to_daemon(cmd, daemon_args) {
        Ok(result) => {
            let text = if let Some(data) = &result.data {
                serde_json::to_string_pretty(data).unwrap()
            } else {
                result.message.clone()
            };
            tool_result(!result.success, json!([{"type": "text", "text": text}]))
        }
        Err(e) => {
            tool_result(true, json!([{"type": "text", "text": e}]))
        }
    }
}

/// Capture a frame and return it as an MCP image block.
///
/// The daemon hands back base64 JPEG; we wrap it in `{"type": "image"}` so
/// the model receives a picture rather than a wall of base64 text. A short
/// text line goes alongside it, because "which frame is this?" is only
/// answerable from the timestamp.
/// Disk housekeeping, answered without a player.
///
/// Every other tool routes through the daemon, which means starting mpv.
/// This one reads and deletes files, and the machine most in need of it is
/// the one where the app will not start — so it runs here.
fn handle_cleanup(args: &Value) -> Value {
    use crate::core::cleanup;

    if args["apply"].as_bool().unwrap_or(false) {
        return match cleanup::remove_leftovers() {
            Ok(report) => cleanup_result(
                format!("removed {}", cleanup::human_size(report.total_bytes)),
                &report,
                false,
            ),
            Err(e) => tool_result(true, json!([{"type": "text", "text": e.to_string()}])),
        };
    }

    let report = cleanup::scan();
    let summary = match (&report.directory, report.items.len()) {
        (None, _) => "nothing left behind by an earlier install".to_string(),
        (Some(_), 0) => "nothing left to remove".to_string(),
        (Some(dir), n) => format!(
            "{} in {} item(s) at {}",
            cleanup::human_size(report.total_bytes),
            n,
            dir
        ),
    };
    cleanup_result(summary, &report, false)
}

/// A one-line summary a model can act on, plus the itemised report.
fn cleanup_result(summary: String, report: &crate::core::cleanup::Report, is_error: bool) -> Value {
    let detail = serde_json::to_string_pretty(report).unwrap_or_default();
    tool_result(
        is_error,
        json!([{"type": "text", "text": format!("{}
{}", summary, detail)}]),
    )
}

fn handle_startup() -> Value {
    use crate::core::boot;

    let path = boot::log_path();
    let Ok(body) = std::fs::read_to_string(&path) else {
        return tool_result(
            true,
            json!([{"type": "text", "text": format!("no startup log at {}", path.display())}]),
        );
    };
    let phases = boot::parse_last_launch(&body);
    let summary = match phases.last() {
        None => format!("no startup marks in {}", path.display()),
        Some(last) => format!(
            "last launch: {} phases, reaching \"{}\" at {} ms",
            phases.len(),
            last.label,
            last.at_ms
        ),
    };
    let detail = serde_json::to_string_pretty(&phases).unwrap_or_default();
    tool_result(
        false,
        json!([{"type": "text", "text": format!("{}
{}", summary, detail)}]),
    )
}

fn handle_describe_frame(args: &Value) -> Value {
    // Never forward `output` — writing files is the CLI's job. An agent
    // asking to see a frame wants the pixels, not a path on someone's disk.
    let mut forwarded = json!({});
    for key in ["position", "max_edge"] {
        if let Some(v) = args.get(key) {
            forwarded[key] = v.clone();
        }
    }

    let result = match daemon::send_to_daemon("describe_frame", forwarded) {
        Ok(r) => r,
        Err(e) => return tool_result(true, json!([{"type": "text", "text": e}])),
    };
    if !result.success {
        return tool_result(true, json!([{"type": "text", "text": result.message}]));
    }

    let data = result.data.unwrap_or(Value::Null);
    let Some(base64) = data.get("base64").and_then(|v| v.as_str()) else {
        return tool_result(
            true,
            json!([{"type": "text", "text": "frame capture returned no image data"}]),
        );
    };
    let position = data.get("position").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let mime = data
        .get("mime_type")
        .and_then(|v| v.as_str())
        .unwrap_or("image/jpeg");

    tool_result(
        false,
        json!([
            {"type": "text", "text": format!("Frame at {:.2}s of the currently playing file.", position)},
            {"type": "image", "data": base64, "mimeType": mime},
        ]),
    )
}

fn tool_result(is_error: bool, content: Value) -> Value {
    json!({
        "content": content,
        "isError": is_error,
    })
}

/// Return the list of tools for tools/list response.
pub fn tool_definitions() -> Value {
    let mut all = Vec::new();
    for group in [
        tools_core(),
        tools_v010(),
        tools_understanding(),
        tools_audio(),
        tools_window(),
    ] {
        if let Value::Array(items) = group {
            all.extend(items);
        }
    }
    Value::Array(all)
}

/// The window itself, and what a person would say is playing.
fn tools_window() -> Value {
    json!([
        {
            "name": "window_mode",
            "description": "Get or set the shape of the player window the user is looking at. `normal` is the full player, `pip` is a small always-on-top video window, `music` is the compact audio layout with cover art and tags. Call with no arguments to read the current mode. Only works while the unflick GUI is running — the headless daemon has no window and says so.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "mode": {
                        "type": "string",
                        "enum": ["normal", "pip", "music"],
                        "description": "Mode to switch to. Omit to read the current one."
                    }
                }
            }
        },
        {
            "name": "cleanup",
            "description": "Find files an older unflick left behind — on Windows, upgrading from v0.9 moved the install directory and stranded roughly half a gigabyte with no uninstall entry. Reports by default; pass `apply` to delete. The live thumbnail and cover caches share that folder and are never touched.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "apply": {
                        "type": "boolean",
                        "description": "Delete what was found. Omit to report only."
                    }
                }
            }
        },
        {
            "name": "cast",
            "description": "Send what is playing to a television on the network over DLNA. \"list\" finds renderers; \"to\" starts a cast (naming a renderer, or omitting it when there is only one) and pauses playback here so it is not heard twice; \"stop\", \"pause\", \"resume\", \"seek\" and \"status\" drive it once it is running. Only local files can be cast — the television fetches them from this machine.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list", "to", "stop", "status", "pause", "resume", "seek"],
                        "description": "Defaults to status."
                    },
                    "renderer": {
                        "type": "string",
                        "description": "Which renderer, by name or part of it. Omit when there is only one."
                    },
                    "file": {
                        "type": "string",
                        "description": "A file to cast instead of what is playing."
                    },
                    "seconds": {
                        "type": "number",
                        "description": "For seek, where to go. For list and to, how long to wait for renderers to answer (default 3)."
                    }
                }
            }
        },
        {
            "name": "disc_list",
            "description": "Optical drives on this machine and whether each holds a DVD or Blu-ray, plus whether this build can play them at all. Play one with `play` and the drive path (\"D:\\\\\"), a disc image (\"film.iso\"), or a folder holding VIDEO_TS / BDMV; a specific title is mpv's own syntax, \"dvd://3\".",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Ask about one path instead of listing drives — reports what it is without opening it."
                    }
                }
            }
        },
        {
            "name": "session",
            "description": "What the user was last watching and how far in — and, with action \"restore\", reopen it there. unflick writes this down every few seconds while playing, so it survives a closed window or a crash. Use it when someone asks to get back to what they were watching without naming the file. \"clear\" forgets it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["show", "restore", "clear"],
                        "description": "Defaults to show, which only reports."
                    }
                }
            }
        },
        {
            "name": "startup",
            "description": "The last launch's startup timeline, phase by phase, in milliseconds from process start — process init, video pipeline, window shown, the launch file opening, React mounting, the control port. Use it to say where a slow start went rather than that it was slow.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "now_playing",
            "description": "What is playing, described the way a person would: title, artist, album, and whether there is any picture (an embedded cover is not video). Use this rather than `get_status` when the question is 'what is this', and `get_status` when it is 'where are we'.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "cover": {
                        "type": "boolean",
                        "description": "Also extract the embedded cover art and return its path on disk. Costs one ffmpeg run the first time per file."
                    }
                }
            }
        }
    ])
}

/// Audio processing: the equaliser, loudness normalisation, and pitch.
///
/// A separate group for the same reason the others are: `json!` has a macro
/// recursion limit, and one array of every tool blows through it.
fn tools_audio() -> Value {
    json!([
        {
            "name": "equalizer_get",
            "description": "Read the 10-band equaliser: per-band gains in dB, the band centre frequencies they correspond to, the preamp, whether loudness normalisation is on, and the filter chain mpv is actually running. Read this before changing a band so you adjust the user's curve instead of replacing it.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "equalizer_set",
            "description": "Adjust the equaliser. Set one band with `band` (0-9, low to high) plus `gain` in dB, or the whole curve at once with `bands`. `enabled` switches the equaliser in and out without discarding the curve, `normalize` evens out quiet dialogue against loud action, and `preamp` makes headroom so boosted bands don't clip. Gains are clamped to ±12 dB. Every change rebuilds mpv's audio filter chain, so set a whole curve in one call rather than nine.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "band": { "type": "integer", "description": "Band index 0-9: 31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000 Hz" },
                    "gain": { "type": "number", "description": "Gain in dB for that band, -12 to 12" },
                    "bands": {
                        "type": "array",
                        "items": { "type": "number" },
                        "description": "All ten gains in dB, low frequency first"
                    },
                    "enabled": { "type": "boolean", "description": "Switch the equaliser on or off, keeping the curve" },
                    "normalize": { "type": "boolean", "description": "Dynamic loudness normalisation" },
                    "preamp": { "type": "number", "description": "Preamp in dB, -20 to 12" }
                }
            }
        },
        {
            "name": "equalizer_preset",
            "description": "Apply a named equaliser curve and switch the equaliser on. Use equalizer_presets to see what exists and what each is for. 'speech' lifts dialogue out of a loud mix and 'night' tames explosions while keeping voices — those two answer most requests about not being able to hear what people are saying.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Preset name, e.g. speech, night, bass, treble, headphones, flat" }
                },
                "required": ["name"]
            }
        },
        {
            "name": "equalizer_presets",
            "description": "List the available equaliser presets with a description of what each is for and the curve it applies.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "equalizer_reset",
            "description": "Remove every audio filter: flat curve, no preamp, no normalisation.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "pitch_correction",
            "description": "Read or set whether changing playback speed keeps the original pitch. On by default, which is what makes 1.5x speech listenable rather than chipmunk-like. Turn it off only when the pitch shift is the point, such as checking a musical tempo change.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "enabled": { "type": "boolean", "description": "Omit to read the current setting" }
                }
            }
        }
    ])
}

/// Playback, playlist, library, subtitles, settings, filters — everything
/// that shipped through v0.9.
fn tools_core() -> Value {
    json!([
        {
            "name": "play",
            "description": "Play a video file or URL. URLs from supported streaming sites (YouTube, Bilibili, Twitch, Vimeo, etc.) are extracted via the bundled yt-dlp before playback.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Path to a local video file, or HTTP(S) URL to a streaming site." },
                    "seek": { "type": "number", "description": "Seek to position in seconds" },
                    "volume": { "type": "integer", "description": "Volume level (0-100)" },
                    "speed": { "type": "number", "description": "Playback speed multiplier" },
                    "proxy": { "type": "string", "description": "Optional proxy URL forwarded to yt-dlp for URL extraction (e.g. http://127.0.0.1:7890)" }
                },
                "required": ["file"]
            }
        },
        {
            "name": "pause",
            "description": "Pause playback. Acts on the player the user is watching — when the unflick window is open, this pauses the video on screen.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "resume",
            "description": "Resume playback after a pause. From end-of-file this rewinds to the start rather than doing nothing.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "stop",
            "description": "Stop playback and unload the file. The resume point is saved unless the file was watched to the end.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "seek",
            "description": "Seek to a position in seconds",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "seconds": { "type": "number", "description": "Position in seconds" }
                },
                "required": ["seconds"]
            }
        },
        {
            "name": "set_volume",
            "description": "Set volume level",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "level": { "type": "integer", "description": "Volume level (0-100)" }
                },
                "required": ["level"]
            }
        },
        {
            "name": "set_speed",
            "description": "Get or set the playback speed of the window the user is watching. Call with no arguments to read the current rate. Pitch is corrected by default, so speech stays natural — see the `pitch` CLI command to turn that off.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "rate": { "type": "number", "description": "Speed multiplier between 0.01 and 100 (e.g. 1.5). Omit to read the current rate." },
                    "relative": { "type": "boolean", "description": "Treat `rate` as an offset from the current speed instead of an absolute multiplier. Clamped to the valid range." }
                }
            }
        },
        {
            "name": "get_status",
            "description": "Get current playback status including state, file, position, duration, volume, and speed",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "file_info",
            "description": "Get media file metadata (duration, resolution, codecs) without affecting current playback",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Path to the media file" }
                },
                "required": ["file"]
            }
        },
        {
            "name": "playlist_add",
            "description": "Add a file to the playlist",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Path to the media file" }
                },
                "required": ["file"]
            }
        },
        {
            "name": "playlist_remove",
            "description": "Remove a playlist entry by index",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "index": { "type": "integer", "description": "Index of the entry to remove" }
                },
                "required": ["index"]
            }
        },
        {
            "name": "playlist_list",
            "description": "List all playlist entries with index, path, and current track indicator",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "playlist_next",
            "description": "Advance to and play the next track in the playlist",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "playlist_prev",
            "description": "Go back to and play the previous track in the playlist",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "playlist_clear",
            "description": "Clear all entries from the playlist",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "playlist_play",
            "description": "Play a specific playlist entry by index",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "index": { "type": "integer", "description": "Index of the playlist entry to play" }
                },
                "required": ["index"]
            }
        },
        {
            "name": "load_subtitle",
            "description": "Load an external subtitle file",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Path to the subtitle file (.srt, .ass, .sub)" }
                },
                "required": ["file"]
            }
        },
        {
            "name": "subtitle_list",
            "description": "List all subtitle tracks (embedded and external)",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "subtitle_select",
            "description": "Select a subtitle track by ID (0 to disable subtitles)",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer", "description": "Subtitle track ID (0 to disable)" }
                },
                "required": ["id"]
            }
        },
        {
            "name": "audio_list",
            "description": "List all audio tracks (embedded)",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "audio_select",
            "description": "Select an audio track by ID (0 to disable audio)",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer", "description": "Audio track ID (0 to disable)" }
                },
                "required": ["id"]
            }
        },
        {
            "name": "generate_subtitles",
            "description": "Generate subtitles for a video using whisper. Mode 'local' uses bundled or supplied whisper.cpp; mode 'api' uses OpenAI. Returns the path of the generated .srt file.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "video": { "type": "string", "description": "Path to the video file" },
                    "mode": { "type": "string", "enum": ["local", "api"], "description": "Transcription mode (auto-detected from arguments if omitted)" },
                    "whisper": { "type": "string", "description": "Path to whisper-cli (local mode; auto-detects bundled if omitted)" },
                    "model": { "type": "string", "description": "Path to whisper model (local mode; auto-detects bundled if omitted)" },
                    "api_key": { "type": "string", "description": "OpenAI API key (api mode)" },
                    "output_dir": { "type": "string", "description": "Output directory for the .srt file" }
                },
                "required": ["video"]
            }
        },
        {
            "name": "translate_subtitles",
            "description": "Translate an SRT file to another language using OpenAI. Returns the path of the translated .srt file.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "srt": { "type": "string", "description": "Path to the source .srt file" },
                    "target_lang": { "type": "string", "description": "Target language (e.g. 'Chinese', 'Spanish')" },
                    "api_key": { "type": "string", "description": "OpenAI API key" },
                    "output_dir": { "type": "string", "description": "Output directory" }
                },
                "required": ["srt", "target_lang", "api_key"]
            }
        },
        {
            "name": "library_scan",
            "description": "Scan a directory for video files and add them to the library",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "dir": { "type": "string", "description": "Directory path to scan" }
                },
                "required": ["dir"]
            }
        },
        {
            "name": "library_search",
            "description": "Search the media library by title or path",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" }
                },
                "required": ["query"]
            }
        },
        {
            "name": "library_list",
            "description": "List all media files in the library",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "library_remove",
            "description": "Remove a media entry from the library by ID",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer", "description": "Media entry ID to remove" }
                },
                "required": ["id"]
            }
        },
        {
            "name": "clip",
            "description": "Extract a video clip segment, optionally as GIF. Requires ffmpeg in PATH.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Input file path (uses currently playing file if omitted)" },
                    "start": { "type": "number", "description": "Start time in seconds" },
                    "end": { "type": "number", "description": "End time in seconds" },
                    "output": { "type": "string", "description": "Output file path (auto-generated if omitted)" },
                    "gif": { "type": "boolean", "description": "Export as GIF instead of MP4" }
                },
                "required": ["start", "end"]
            }
        },
        {
            "name": "screenshot",
            "description": "Take a screenshot of the current video frame",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "output": { "type": "string", "description": "Output file path (optional, auto-generated if omitted)" }
                }
            }
        },
        {
            "name": "save_position",
            "description": "Save playback position for a file (for resume playback)",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the media file" },
                    "position": { "type": "number", "description": "Position in seconds" }
                },
                "required": ["path", "position"]
            }
        },
        {
            "name": "get_position",
            "description": "Get saved playback position for a file",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the media file" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "settings_path",
            "description": "Get the absolute path of the settings.json file",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "settings_get",
            "description": "Read all settings, or a single key if provided",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Optional key name; omit to read all settings" }
                }
            }
        },
        {
            "name": "settings_set",
            "description": "Set a single settings key to the given JSON value",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Key name" },
                    "value": { "description": "Any JSON value (string, number, bool, object, array)" }
                },
                "required": ["key", "value"]
            }
        },
        {
            "name": "settings_unset",
            "description": "Remove a single settings key",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Key name to remove" }
                },
                "required": ["key"]
            }
        },
        {
            "name": "filter_list",
            "description": "List current video filter values (brightness, contrast, saturation, gamma, hue)",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "filter_set",
            "description": "Set a video filter (brightness | contrast | saturation | gamma | hue) to a value in -100..100",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "enum": ["brightness", "contrast", "saturation", "gamma", "hue"] },
                    "value": { "type": "integer", "minimum": -100, "maximum": 100 }
                },
                "required": ["name", "value"]
            }
        },
        {
            "name": "filter_reset",
            "description": "Reset all video filters to 0 (neutral)",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "sponsor_segments",
            "description": "Fetch SponsorBlock skip segments for a YouTube URL. Returns the configured categories plus a list of {start, end, category, action_type, uuid} segments. Empty list when none exist (404). Errors only on network/parse failure or non-YouTube URLs.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "YouTube URL (watch / shorts / youtu.be / embed)" }
                },
                "required": ["url"]
            }
        },
    ])
}

/// Tools added in v0.10: timing correction, chapters, A-B loop, frame
/// stepping, playlist modes.
///
/// Split from `tools_core` purely for the compiler's benefit — one `json!`
/// literal holding every tool blows past the macro recursion limit.
fn tools_v010() -> Value {
    json!([
        {
            "name": "subtitle_delay",
            "description": "Get or set the subtitle delay in seconds. Positive values show subtitles later. Call with no arguments to read the current delay. Useful for correcting AI-generated subtitles, which often drift a few hundred milliseconds from the dialogue.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "seconds": { "type": "number", "description": "Delay in seconds. Omit to read the current value." },
                    "relative": { "type": "boolean", "description": "Treat `seconds` as an offset from the current delay instead of an absolute value" }
                }
            }
        },
        {
            "name": "audio_delay",
            "description": "Get or set the audio delay in seconds, for fixing lip-sync. Positive values play audio later. Call with no arguments to read the current delay.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "seconds": { "type": "number", "description": "Delay in seconds. Omit to read the current value." },
                    "relative": { "type": "boolean", "description": "Treat `seconds` as an offset from the current delay instead of an absolute value" }
                }
            }
        },
        {
            "name": "subtitle_style_get",
            "description": "Read subtitle appearance: scale, vertical position, colour, border size, bold.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "subtitle_style_set",
            "description": "Set one subtitle appearance property. Returns the full style after the change.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "enum": ["scale", "pos", "color", "border_size", "bold"] },
                    "value": { "description": "scale: 0.1-10 · pos: 0-150 (100 = bottom) · color: #RRGGBBAA · border_size: 0-20 · bold: boolean" }
                },
                "required": ["name", "value"]
            }
        },
        {
            "name": "chapter_list",
            "description": "List the chapters of the currently playing file: index, title, start time, and which one is playing. Returns an empty list for files without chapters.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "chapter_seek",
            "description": "Jump to a chapter by 0-based index.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "index": { "type": "integer", "description": "0-based chapter index" }
                },
                "required": ["index"]
            }
        },
        {
            "name": "chapter_next",
            "description": "Jump to the next chapter. Clamps at the last chapter.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "chapter_prev",
            "description": "Jump to the previous chapter. Clamps at the first chapter.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "ab_loop",
            "description": "Control the A-B loop, which repeats a section of the current file. Set point A and point B to start looping; clear to stop. Omit `position` to use the current playback position.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["a", "b", "clear", "status"], "description": "a/b set a loop bound, clear removes both, status reads them" },
                    "position": { "type": "number", "description": "Position in seconds for the a/b actions. Defaults to the current position." }
                },
                "required": ["action"]
            }
        },
        {
            "name": "frame_step",
            "description": "Step forward exactly one frame. Pauses playback, as frame stepping is for inspecting a still.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "frame_back_step",
            "description": "Step back exactly one frame. Pauses playback.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "playlist_repeat",
            "description": "Get or set the playlist repeat mode. 'off' stops after the last entry, 'one' loops the current file, 'all' wraps around. Call with no arguments to read the current mode.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "mode": { "type": "string", "enum": ["off", "one", "all"] }
                }
            }
        },
        {
            "name": "playlist_shuffle",
            "description": "Get or set playlist shuffle. Enabling it reorders playback without renumbering entries, and keeps the current track playing. Call with no arguments to read the current setting.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "enabled": { "type": "boolean" }
                }
            }
        },
        {
            "name": "shutdown",
            "description": "Shut down the unflick daemon. Declined when the control port is held by the GUI — close the window instead.",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

/// Tools that read what the player can see and hear.
///
/// These are the ones that need to live *inside* the player: they work off
/// the subtitle track it already has open and the frame it is currently
/// showing. A script driving a player from outside can seek; it cannot
/// answer "where does she mention the refund policy?".
fn tools_understanding() -> Value {
    json!([
        {
            "name": "get_subtitles",
            "description": "Find and load subtitles for the playing video from OpenSubtitles, in one step. Picks the best match: a subtitle synced against this exact file if one exists (checked by file hash, so the timing is correct), otherwise the most-downloaded subtitle for the title. Prefer this over generate_subtitles when the video is a released film or show — a human-made subtitle is more accurate and arrives in a second rather than minutes. Requires the user's own OpenSubtitles API key in the `opensubtitles_api_key` setting; it fails with instructions if unset. Costs one of the user's limited daily downloads, so do not call it repeatedly.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search text. Omit to derive a title from the playing file's name." },
                    "file": { "type": "string", "description": "Video to match against. Defaults to the playing file." },
                    "languages": { "type": "string", "description": "Comma-separated OpenSubtitles language codes, e.g. \"zh-CN,en\". Defaults to the opensubtitles_languages setting, else English." },
                    "load": { "type": "boolean", "description": "Load the subtitle into the player after downloading (default true)" }
                }
            }
        },
        {
            "name": "find_subtitles",
            "description": "Search OpenSubtitles and return the candidates without downloading anything. Use this when the user should choose, or to check what exists before spending a download. Each result carries a file_id for download_subtitle, plus `moviehash_match` — true means it is synced to this exact file. Costs no download quota. Requires the user's own OpenSubtitles API key in the `opensubtitles_api_key` setting.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search text. Omit to derive a title from the playing file's name." },
                    "file": { "type": "string", "description": "Video to match against. Defaults to the playing file." },
                    "languages": { "type": "string", "description": "Comma-separated OpenSubtitles language codes, e.g. \"zh-CN,en\"" },
                    "hash": { "type": "boolean", "description": "Match by file hash as well as title (default true). Set false to search by title only." }
                }
            }
        },
        {
            "name": "download_subtitle",
            "description": "Download one subtitle chosen from find_subtitles and load it into the player. Saved next to the video when that directory is writable, so it loads automatically next time. Costs one of the user's limited daily OpenSubtitles downloads.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file_id": { "type": "integer", "description": "The file_id from a find_subtitles result" },
                    "file": { "type": "string", "description": "Video to save the subtitle beside. Defaults to the playing file." },
                    "language": { "type": "string", "description": "Language code of the chosen subtitle, used in the saved filename" },
                    "load": { "type": "boolean", "description": "Load it into the player after downloading (default true)" }
                },
                "required": ["file_id"]
            }
        },
        {
            "name": "search_transcript",
            "description": "Search the currently playing file's subtitles for a phrase and return every match with its timestamp. Works with loaded subtitle files, sidecar .srt/.vtt files, embedded text tracks, and subtitles you generated with generate_subtitles. Use this to answer questions about what is said in a video, or to find the moment to jump to. Case-insensitive substring match, not regex.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Phrase to look for" },
                    "limit": { "type": "integer", "description": "Maximum matches to return (default 20, max 200)" }
                },
                "required": ["query"]
            }
        },
        {
            "name": "seek_to_text",
            "description": "Jump playback to where a phrase is spoken. Finds the phrase in the subtitles and seeks just before that line, so the sentence is heard from its start. This is the 'skip to the part where they talk about X' tool.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Phrase to jump to" },
                    "occurrence": { "type": "integer", "description": "Which occurrence, 1-based (default 1)" }
                },
                "required": ["query"]
            }
        },
        {
            "name": "transcript_get",
            "description": "Return the full transcript of the currently playing file as timed cues, plus where it came from. Use it to read or summarise a video, or as input for writing your own chapter list with set_chapters. Long videos produce a lot of text — prefer search_transcript when you only need one passage.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "generate_chapters",
            "description": "Give a file that has no chapters a set of them, derived from the pauses in its transcript. The result becomes real navigation: it shows up in chapter_list, marks the progress bar, and responds to chapter_seek. This is a rough heuristic — if you have read the transcript, set_chapters will give better titles and boundaries.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "count": { "type": "integer", "description": "Roughly how many chapters to aim for (2-50, default 8)" }
                }
            }
        },
        {
            "name": "set_chapters",
            "description": "Set the chapter list for a file that has none, from your own reading of its content. Times are in seconds; they are sorted and clamped to the file automatically. These become real chapters — navigable with chapter_seek and drawn on the progress bar. Fails if the file already ships with its own chapters.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "chapters": {
                        "type": "array",
                        "description": "Chapter marks in playback order",
                        "items": {
                            "type": "object",
                            "properties": {
                                "time": { "type": "number", "description": "Start time in seconds" },
                                "title": { "type": "string", "description": "Chapter title" }
                            },
                            "required": ["time"]
                        }
                    }
                },
                "required": ["chapters"]
            }
        },
        {
            "name": "clear_chapters",
            "description": "Remove chapters added by generate_chapters or set_chapters. A file's own built-in chapters are not affected.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "keybind_list",
            "description": "List every keyboard shortcut: action id, label, current key, default, and whether the user has changed it. Keys read as Mod+Alt+Shift+key, where Mod is Ctrl on Windows/Linux and Cmd on macOS.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "keybind_set",
            "description": "Rebind a keyboard shortcut. Fails if the key is already taken by another action, naming the conflict rather than silently stealing it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "Action id from keybind_list, e.g. play_pause" },
                    "key": { "type": "string", "description": "Key, e.g. k · Shift+z · Mod+o · PageUp" }
                },
                "required": ["action", "key"]
            }
        },
        {
            "name": "keybind_reset",
            "description": "Restore a shortcut to its default. Omit `action` to reset every binding at once.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "Action id. Omit to reset all." }
                }
            }
        },
        {
            "name": "video_transform_get",
            "description": "Read how the picture is fitted to the window: aspect override, rotation, zoom multiplier, pan-scan and deinterlace. Distinct from filter_list, which is colour rather than geometry.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "video_transform_set",
            "description": "Fix a squashed aspect ratio, straighten a video recorded sideways, crop black bars, or deinterlace broadcast footage.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "enum": ["aspect", "rotate", "zoom", "panscan", "deinterlace"] },
                    "value": { "description": "aspect: \"auto\" | \"16:9\" | 1.78 · rotate: 0|90|180|270 · zoom: multiplier, 1 = fit · panscan: 0-1 · deinterlace: boolean" }
                },
                "required": ["name", "value"]
            }
        },
        {
            "name": "video_transform_reset",
            "description": "Restore the file's own geometry — no aspect override, no rotation, no zoom or pan-scan, deinterlace off.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "incognito",
            "description": "Read or set incognito mode. While on, nothing is written to the play history — including plays started from the CLI or by you. Call with no arguments to read it.",
            "inputSchema": {
                "type": "object",
                "properties": { "enabled": { "type": "boolean" } }
            }
        },
        {
            "name": "recent_files",
            "description": "Recently played files, newest first, with how many times each was played and when. Covers files opened directly, not just ones scanned into the library.",
            "inputSchema": {
                "type": "object",
                "properties": { "limit": { "type": "integer", "description": "How many to return (1-200, default 20)" } }
            }
        },
        {
            "name": "recent_clear",
            "description": "Forget the play history. Scanned library metadata is kept — only the played-at times and counts are cleared.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "bookmark_add",
            "description": "Save a named position in a file, so it can be jumped back to later. Defaults to where playback is right now in the file the user is watching. Bookmarks persist across sessions and are recorded even in incognito mode, since they are asked for explicitly.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Label for the spot. Omit and it shows as its timestamp." },
                    "position": { "type": "number", "description": "Seconds into the file. Defaults to the current position; required when bookmarking a file that isn't playing." },
                    "file": { "type": "string", "description": "Path or URL. Defaults to what's playing." }
                }
            }
        },
        {
            "name": "bookmark_list",
            "description": "Bookmarks for the file being watched, in timeline order. Pass all=true for every file, or file to ask about a specific one. Each entry carries the id that bookmark_goto takes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Path or URL. Defaults to what's playing." },
                    "all": { "type": "boolean", "description": "Every file instead of just one." }
                }
            }
        },
        {
            "name": "bookmark_goto",
            "description": "Jump to a bookmark by id. Seeks if its file is already playing; otherwise opens that file at the bookmarked position, saving a resume point for the outgoing one.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "integer", "description": "Bookmark id from bookmark_list" } },
                "required": ["id"]
            }
        },
        {
            "name": "bookmark_rename",
            "description": "Give a bookmark a name, or drop the one it has by omitting name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer", "description": "Bookmark id from bookmark_list" },
                    "name": { "type": "string", "description": "New label. Omit to remove the name." }
                },
                "required": ["id"]
            }
        },
        {
            "name": "bookmark_remove",
            "description": "Delete one bookmark by id.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "integer", "description": "Bookmark id from bookmark_list" } },
                "required": ["id"]
            }
        },
        {
            "name": "bookmark_clear",
            "description": "Delete every bookmark for the file being watched, or for the file named. Pass all=true to delete them for every file — that scope is never reached by default.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Path or URL. Defaults to what's playing." },
                    "all": { "type": "boolean", "description": "Every bookmark, for every file." }
                }
            }
        },
        {
            "name": "mouse_list",
            "description": "List mouse bindings: wheel up/down, click, double click, middle click, and the four right-drag gestures, each with the action it runs.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "mouse_set",
            "description": "Point a mouse trigger at an action. Unlike keys, two triggers may share an action — wheel-up and drag-up both raising volume is fine, since the inputs are distinct. Pass \"none\" as the action to disable a trigger.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "trigger": { "type": "string", "description": "Trigger id from mouse_list, e.g. wheel_up, gesture_left" },
                    "action": { "type": "string", "description": "Action id from keybind_list, or \"none\"" }
                },
                "required": ["trigger", "action"]
            }
        },
        {
            "name": "mouse_reset",
            "description": "Restore a mouse trigger to its default. Omit `trigger` to reset all of them.",
            "inputSchema": {
                "type": "object",
                "properties": { "trigger": { "type": "string" } }
            }
        },
        {
            "name": "describe_frame",
            "description": "Return the frame showing right now as an image, so you can see what is on screen. Optionally seek to a position first. The picture is the one the viewer is watching — GUI, CLI and MCP share a single player. Downscaled to keep the response small; subtitles and on-screen controls are not burned in.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "position": { "type": "number", "description": "Seek here (seconds) before capturing" },
                    "max_edge": { "type": "integer", "description": "Longest edge in pixels (64-2048, default 768)" }
                }
            }
        }
    ])
}
