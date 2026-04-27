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
            "name": "shutdown",
            "description": "Shut down the unflick daemon",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}
