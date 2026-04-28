use tauri::{command, State, AppHandle, Manager};
use serde_json::{json, Value};

use crate::core::player::Player;
use crate::core::library;
use super::state::GuiPlayer;

#[command]
pub fn player_init(app: AppHandle, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let mut player_lock = gui_player.player.lock().unwrap();
    if player_lock.is_some() {
        return Ok(json!({"status": "already initialized"}));
    }

    // Get the native window handle (HWND) to embed mpv into it
    let window = app.get_webview_window("main").ok_or("no main window")?;

    #[cfg(target_os = "windows")]
    let wid = {
        use raw_window_handle::HasWindowHandle;
        let handle = window.window_handle().map_err(|e| e.to_string())?;
        let raw = handle.as_raw();
        match raw {
            raw_window_handle::RawWindowHandle::Win32(h) => {
                h.hwnd.get() as i64
            }
            _ => return Err("unsupported window handle".to_string()),
        }
    };

    #[cfg(not(target_os = "windows"))]
    let wid = 0i64;

    let player = Player::new_with_wid(wid).map_err(|e| e.to_string())?;
    *player_lock = Some(player);

    Ok(json!({"status": "initialized", "wid": wid}))
}


#[command]
pub fn player_play(
    file: String,
    seek: Option<f64>,
    volume: Option<i64>,
    speed: Option<f64>,
    gui_player: State<'_, GuiPlayer>,
) -> Result<Value, String> {
    let lock = gui_player.player.lock().unwrap();
    let player = lock.as_ref().ok_or("player not initialized")?;
    player.play(&file, seek, volume, speed).map_err(|e| e.to_string())?;
    Ok(json!({"message": format!("playing {}", file)}))
}

#[command]
pub fn player_pause(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let lock = gui_player.player.lock().unwrap();
    let player = lock.as_ref().ok_or("player not initialized")?;
    player.pause().map_err(|e| e.to_string())?;
    Ok(json!({"message": "paused"}))
}

#[command]
pub fn player_resume(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let lock = gui_player.player.lock().unwrap();
    let player = lock.as_ref().ok_or("player not initialized")?;
    player.resume().map_err(|e| e.to_string())?;
    Ok(json!({"message": "resumed"}))
}

#[command]
pub fn player_stop(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let lock = gui_player.player.lock().unwrap();
    let player = lock.as_ref().ok_or("player not initialized")?;
    player.stop().map_err(|e| e.to_string())?;
    Ok(json!({"message": "stopped"}))
}

#[command]
pub fn player_seek(seconds: f64, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let lock = gui_player.player.lock().unwrap();
    let player = lock.as_ref().ok_or("player not initialized")?;
    player.seek(seconds).map_err(|e| e.to_string())?;
    Ok(json!({"message": format!("seeked to {}s", seconds)}))
}

#[command]
pub fn player_set_volume(level: i64, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let lock = gui_player.player.lock().unwrap();
    let player = lock.as_ref().ok_or("player not initialized")?;
    player.set_volume(level).map_err(|e| e.to_string())?;
    Ok(json!({"message": format!("volume set to {}", level)}))
}

#[command]
pub fn player_set_speed(rate: f64, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let lock = gui_player.player.lock().unwrap();
    let player = lock.as_ref().ok_or("player not initialized")?;
    player.set_speed(rate).map_err(|e| e.to_string())?;
    Ok(json!({"message": format!("speed set to {}x", rate)}))
}

#[command]
pub fn player_status(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let lock = gui_player.player.lock().unwrap();
    let player = lock.as_ref().ok_or("player not initialized")?;
    let status = player.status();
    serde_json::to_value(&status).map_err(|e| e.to_string())
}

#[command]
pub fn player_screenshot(output: Option<String>, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let lock = gui_player.player.lock().unwrap();
    let player = lock.as_ref().ok_or("player not initialized")?;
    let path = output.unwrap_or_else(|| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        format!("unflick-screenshot-{}.png", ts)
    });
    player.screenshot(&path).map_err(|e| e.to_string())?;
    Ok(json!({"path": path}))
}

#[command]
pub fn toggle_pip(app: AppHandle) -> Result<Value, String> {
    let window = app.get_webview_window("main").ok_or("no main window")?;
    let is_on_top = window.is_always_on_top().map_err(|e| e.to_string())?;

    if is_on_top {
        // Exit PiP: restore normal size, disable always-on-top
        window.set_always_on_top(false).map_err(|e| e.to_string())?;
        let _ = window.set_size(tauri::LogicalSize::new(1024.0, 640.0));
        // Center the window on screen
        let _ = window.center();
        Ok(json!({"pip": false}))
    } else {
        // Enter PiP: small window, always-on-top
        window.set_always_on_top(true).map_err(|e| e.to_string())?;
        let _ = window.set_size(tauri::LogicalSize::new(400.0, 250.0));
        // Move to bottom-right corner
        let _ = window.set_position(tauri::LogicalPosition::new(1400.0, 700.0));
        Ok(json!({"pip": true}))
    }
}

#[command]
pub fn set_fullscreen(app: AppHandle) -> Result<Value, String> {
    let window = app.get_webview_window("main").ok_or("no main window")?;
    let is_fullscreen = window.is_fullscreen().map_err(|e| e.to_string())?;
    window.set_fullscreen(!is_fullscreen).map_err(|e| e.to_string())?;
    Ok(json!({"fullscreen": !is_fullscreen}))
}

#[command]
pub fn exit_fullscreen(app: AppHandle) -> Result<Value, String> {
    let window = app.get_webview_window("main").ok_or("no main window")?;
    let is_fullscreen = window.is_fullscreen().map_err(|e| e.to_string())?;
    if is_fullscreen {
        window.set_fullscreen(false).map_err(|e| e.to_string())?;
    }
    Ok(json!({"fullscreen": false}))
}

#[command]
pub async fn open_file_dialog() -> Result<Value, String> {
    let result = tokio::task::spawn_blocking(|| {
        rfd::FileDialog::new()
            .add_filter("Video", &["mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "ts", "mpg", "mpeg"])
            .add_filter("All Files", &["*"])
            .pick_file()
    })
    .await
    .map_err(|e| e.to_string())?;

    match result {
        Some(path) => Ok(json!({"path": path.to_string_lossy().to_string()})),
        None => Ok(json!({"path": null})),
    }
}

#[command]
pub async fn open_folder_dialog() -> Result<Value, String> {
    let result = tokio::task::spawn_blocking(|| rfd::FileDialog::new().pick_folder())
        .await
        .map_err(|e| e.to_string())?;

    match result {
        Some(path) => Ok(json!({"path": path.to_string_lossy().to_string()})),
        None => Ok(json!({"path": null})),
    }
}

// ─── Library commands ────────────────────────────────────────────────────────

#[command]
pub fn library_list(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let db_lock = gui_player.db.lock().unwrap();
    let db = db_lock.as_ref().ok_or("database not available")?;
    let entries = db.list_all().map_err(|e| e.to_string())?;
    serde_json::to_value(&entries).map_err(|e| e.to_string())
}

#[command]
pub fn library_search(query: String, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let db_lock = gui_player.db.lock().unwrap();
    let db = db_lock.as_ref().ok_or("database not available")?;
    let entries = db.search(&query).map_err(|e| e.to_string())?;
    serde_json::to_value(&entries).map_err(|e| e.to_string())
}

#[command]
pub fn library_scan(dir: String, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let db_lock = gui_player.db.lock().unwrap();
    let db = db_lock.as_ref().ok_or("database not available")?;
    let added = library::scan_directory(db, &dir).map_err(|e| e.to_string())?;
    Ok(json!({
        "scanned_dir": dir,
        "added": added.len(),
        "entries": serde_json::to_value(&added).unwrap_or(json!([])),
    }))
}

// ─── Playback position / history commands ────────────────────────────────────

#[command]
pub fn save_position(path: String, position: f64, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let db_lock = gui_player.db.lock().unwrap();
    if let Some(db) = db_lock.as_ref() {
        db.save_position(&path, position).map_err(|e| e.to_string())?;
    }
    Ok(json!({"saved": true}))
}

#[command]
pub fn get_position(path: String, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let db_lock = gui_player.db.lock().unwrap();
    if let Some(db) = db_lock.as_ref() {
        let pos = db.get_position(&path).map_err(|e| e.to_string())?;
        return Ok(json!({"position": pos}));
    }
    Ok(json!({"position": null}))
}

#[command]
pub fn clear_position(path: String, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let db_lock = gui_player.db.lock().unwrap();
    if let Some(db) = db_lock.as_ref() {
        db.clear_position(&path).map_err(|e| e.to_string())?;
    }
    Ok(json!({"cleared": true}))
}

#[command]
pub fn record_play(path: String, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let db_lock = gui_player.db.lock().unwrap();
    if let Some(db) = db_lock.as_ref() {
        db.record_play(&path).map_err(|e| e.to_string())?;
    }
    Ok(json!({"recorded": true}))
}

// ─── Subtitle commands ───────────────────────────────────────────────────────

#[command]
pub fn subtitle_load(path: String, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let lock = gui_player.player.lock().unwrap();
    let player = lock.as_ref().ok_or("player not initialized")?;
    player.subtitle_load(&path).map_err(|e| e.to_string())?;
    Ok(json!({"message": format!("subtitle loaded: {}", path)}))
}

#[command]
pub fn subtitle_list(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let lock = gui_player.player.lock().unwrap();
    let player = lock.as_ref().ok_or("player not initialized")?;
    let tracks = player.subtitle_list();
    serde_json::to_value(&tracks).map_err(|e| e.to_string())
}

#[command]
pub fn subtitle_select(id: i64, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let lock = gui_player.player.lock().unwrap();
    let player = lock.as_ref().ok_or("player not initialized")?;
    player.subtitle_select(id).map_err(|e| e.to_string())?;
    Ok(json!({"message": format!("subtitle track {} selected", id)}))
}

// ─── Playlist commands ───────────────────────────────────────────────────────

#[command]
pub fn playlist_add(path: String, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    gui_player.playlist.add(&path);
    Ok(json!({"message": format!("added to playlist: {}", path)}))
}

#[command]
pub fn playlist_list(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let entries = gui_player.playlist.list();
    serde_json::to_value(&entries).map_err(|e| e.to_string())
}

#[command]
pub fn playlist_next(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    match gui_player.playlist.next() {
        Some(path) => {
            // If a player is active, start playing the next track immediately
            if let Ok(lock) = gui_player.player.lock() {
                if let Some(player) = lock.as_ref() {
                    let _ = player.play(&path, None, None, None);
                }
            }
            Ok(json!({"path": path}))
        }
        None => Ok(json!({"path": null, "message": "no next track"})),
    }
}

#[command]
pub fn playlist_prev(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    match gui_player.playlist.prev() {
        Some(path) => {
            if let Ok(lock) = gui_player.player.lock() {
                if let Some(player) = lock.as_ref() {
                    let _ = player.play(&path, None, None, None);
                }
            }
            Ok(json!({"path": path}))
        }
        None => Ok(json!({"path": null, "message": "no previous track"})),
    }
}

#[command]
pub fn playlist_remove(index: usize, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    gui_player.playlist.remove(index).map_err(|e| e)?;
    Ok(json!({"message": format!("removed index {}", index)}))
}

#[command]
pub fn playlist_play_index(index: usize, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let path = gui_player.playlist.set_current(index).map_err(|e| e)?;
    if let Ok(lock) = gui_player.player.lock() {
        if let Some(player) = lock.as_ref() {
            let _ = player.play(&path, None, None, None);
        }
    }
    Ok(json!({"path": path}))
}

#[command]
pub fn playlist_clear(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    gui_player.playlist.clear();
    Ok(json!({"message": "playlist cleared"}))
}

#[command]
pub async fn open_subtitle_dialog() -> Result<Value, String> {
    let result = tokio::task::spawn_blocking(|| {
        rfd::FileDialog::new()
            .add_filter("Subtitles", &["srt", "ass", "ssa", "vtt", "sub", "idx"])
            .add_filter("All Files", &["*"])
            .pick_file()
    })
    .await
    .map_err(|e| e.to_string())?;

    match result {
        Some(path) => Ok(json!({"path": path.to_string_lossy().to_string()})),
        None => Ok(json!({"path": null})),
    }
}

// ─── Clip extraction ─────────────────────────────────────────────────────────

#[command]
pub async fn extract_clip(input: String, start: f64, end: f64, output: String, as_gif: bool) -> Result<Value, String> {
    let result = tokio::task::spawn_blocking(move || {
        crate::core::player::extract_clip(&input, start, end, &output, as_gif)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    Ok(json!({"output": result}))
}

#[command]
pub async fn save_file_dialog(default_name: String) -> Result<Value, String> {
    let result = tokio::task::spawn_blocking(move || {
        rfd::FileDialog::new()
            .set_file_name(&default_name)
            .save_file()
    })
    .await
    .map_err(|e| e.to_string())?;

    match result {
        Some(path) => Ok(json!({"path": path.to_string_lossy().to_string()})),
        None => Ok(json!({"path": null})),
    }
}

// ─── AI subtitle generation ──────────────────────────────────────────────────

#[command]
pub async fn generate_subtitles(
    video_path: String,
    mode: String,
    whisper_binary: Option<String>,
    model_path: Option<String>,
    api_key: Option<String>,
) -> Result<Value, String> {
    let output_dir = dirs_next::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("unflick")
        .to_string_lossy()
        .to_string();
    std::fs::create_dir_all(&output_dir).map_err(|e| e.to_string())?;

    let srt_path = tokio::task::spawn_blocking(move || {
        match mode.as_str() {
            "local" => {
                let binary = whisper_binary
                    .ok_or_else(|| anyhow::anyhow!("whisper binary path not set"))?;
                let model = model_path
                    .ok_or_else(|| anyhow::anyhow!("whisper model path not set"))?;
                crate::core::whisper::transcribe_local(&video_path, &binary, &model, &output_dir)
            }
            "api" => {
                let key = api_key
                    .ok_or_else(|| anyhow::anyhow!("OpenAI API key not set"))?;
                crate::core::whisper::transcribe_api(&video_path, &key, &output_dir)
            }
            _ => anyhow::bail!("unknown transcription mode: {}", mode),
        }
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    Ok(json!({"srt_path": srt_path}))
}

// ─── Settings persistence ─────────────────────────────────────────────────────

#[command]
pub fn save_settings(settings: String) -> Result<Value, String> {
    let settings_path = dirs_next::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("unflick")
        .join("settings.json");
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&settings_path, &settings).map_err(|e| e.to_string())?;
    Ok(json!({"saved": true}))
}

#[command]
pub fn load_settings() -> Result<Value, String> {
    let settings_path = dirs_next::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("unflick")
        .join("settings.json");
    match std::fs::read_to_string(&settings_path) {
        Ok(content) => {
            let value: serde_json::Value =
                serde_json::from_str(&content).map_err(|e| e.to_string())?;
            Ok(value)
        }
        Err(_) => Ok(json!({})),
    }
}

// ─── Audio track commands ─────────────────────────────────────────────────────

#[command]
pub fn audio_list(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let lock = gui_player.player.lock().unwrap();
    let player = lock.as_ref().ok_or("player not initialized")?;
    let tracks = player.audio_list();
    serde_json::to_value(&tracks).map_err(|e| e.to_string())
}

#[command]
pub fn audio_select(id: i64, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let lock = gui_player.player.lock().unwrap();
    let player = lock.as_ref().ok_or("player not initialized")?;
    player.audio_select(id).map_err(|e| e.to_string())?;
    Ok(json!({"message": format!("audio track {} selected", id)}))
}

/// Check if whisper is bundled with this installation
#[command]
pub fn check_bundled_whisper(app: AppHandle) -> Result<Value, String> {
    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    let whisper_bin = resource_dir.join("whisper").join("whisper-cli.exe");
    let whisper_model = resource_dir.join("whisper").join("ggml-tiny.bin");

    let bundled = whisper_bin.exists() && whisper_model.exists();
    if bundled {
        Ok(json!({
            "bundled": true,
            "whisper_binary": whisper_bin.to_string_lossy(),
            "model_path": whisper_model.to_string_lossy()
        }))
    } else {
        Ok(json!({"bundled": false}))
    }
}
