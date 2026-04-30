use serde_json::{json, Value};

use crate::core::daemon;

/// Return the list of resources for resources/list response.
pub fn resource_definitions() -> Value {
    json!([
        {
            "uri": "unflick://now-playing",
            "name": "Now Playing",
            "description": "Current playback state: file, position, duration, volume, speed",
            "mimeType": "application/json"
        },
        {
            "uri": "unflick://playlist",
            "name": "Playlist",
            "description": "Current playlist entries with current-track indicator",
            "mimeType": "application/json"
        },
        {
            "uri": "unflick://library",
            "name": "Media Library",
            "description": "All media files in the library",
            "mimeType": "application/json"
        }
    ])
}

/// Read a resource by URI. Returns the JSON-RPC `result` object for resources/read.
pub fn read_resource(uri: &str) -> Result<Value, String> {
    let cmd = match uri {
        "unflick://now-playing" => "status",
        "unflick://playlist" => "playlist_list",
        "unflick://library" => "library_list",
        _ => return Err(format!("unknown resource: {}", uri)),
    };

    let result = daemon::send_to_daemon(cmd, json!({}))
        .map_err(|e| format!("daemon error: {}", e))?;

    if !result.success {
        return Err(result.message);
    }

    let payload = result.data.unwrap_or(Value::Null);
    let text = serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string());

    Ok(json!({
        "contents": [{
            "uri": uri,
            "mimeType": "application/json",
            "text": text,
        }]
    }))
}
