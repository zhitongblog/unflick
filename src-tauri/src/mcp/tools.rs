use serde_json::{json, Value};

use crate::core::daemon;

/// Handle an MCP tool call by routing to the daemon.
pub fn handle_tool_via_daemon(name: &str, args: &Value) -> Value {
    let (cmd, daemon_args) = match name {
        "play" => ("play", args.clone()),
        "pause" => ("pause", json!({})),
        "resume" => ("resume", json!({})),
        "stop" => ("stop", json!({})),
        "seek" => ("seek", json!({"seconds": args["seconds"]})),
        "set_volume" => ("volume", json!({"level": args["level"]})),
        "set_speed" => ("speed", json!({"rate": args["rate"]})),
        "get_status" => ("status", json!({})),
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
        "generate_subtitles" => ("subtitle_generate", args.clone()),
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

fn tool_result(is_error: bool, content: Value) -> Value {
    json!({
        "content": content,
        "isError": is_error,
    })
}

/// Return the list of tools for tools/list response.
pub fn tool_definitions() -> Value {
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
            "description": "Pause playback",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "resume",
            "description": "Resume playback",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "stop",
            "description": "Stop playback",
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
            "description": "Set playback speed",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "rate": { "type": "number", "description": "Speed multiplier (e.g. 1.5)" }
                },
                "required": ["rate"]
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
            "name": "shutdown",
            "description": "Shut down the unflick daemon",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}
