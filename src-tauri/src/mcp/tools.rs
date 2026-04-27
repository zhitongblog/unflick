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
        "load_subtitle" => ("subtitle_load", json!({"file": args["file"]})),
        "subtitle_list" => ("subtitle_list", json!({})),
        "subtitle_select" => ("subtitle_select", json!({"id": args["id"]})),
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
            "description": "Play a video file",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Path to the video file" },
                    "seek": { "type": "number", "description": "Seek to position in seconds" },
                    "volume": { "type": "integer", "description": "Volume level (0-100)" },
                    "speed": { "type": "number", "description": "Playback speed multiplier" }
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
            "name": "shutdown",
            "description": "Shut down the unflick daemon",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}
