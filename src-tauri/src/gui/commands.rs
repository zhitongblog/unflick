use tauri::{command, State, AppHandle, Manager};
use serde_json::{json, Value};

use crate::core::player::Player;
use super::state::GuiPlayer;

#[command]
pub fn player_init(app: AppHandle, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let mut player_lock = gui_player.player.lock().unwrap();
    if player_lock.is_some() {
        return Ok(json!({"status": "already initialized"}));
    }

    // Get the main window's native handle (HWND on Windows)
    let window = app.get_webview_window("main").ok_or("no main window")?;

    let hwnd_value = {
        use raw_window_handle::HasWindowHandle;
        let handle = window.window_handle().map_err(|e| e.to_string())?;
        match handle.as_raw() {
            raw_window_handle::RawWindowHandle::Win32(h) => h.hwnd.get() as i64,
            _ => return Err("not a Win32 window".to_string()),
        }
    };

    let player = Player::new_with_wid(hwnd_value).map_err(|e| e.to_string())?;
    *player_lock = Some(player);

    Ok(json!({"status": "initialized"}))
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
        window.set_decorations(true).map_err(|e| e.to_string())?;
        let _ = window.set_size(tauri::LogicalSize::new(1024.0, 640.0));
        Ok(json!({"pip": false}))
    } else {
        // Enter PiP: small window, always-on-top, no decorations
        window.set_always_on_top(true).map_err(|e| e.to_string())?;
        window.set_decorations(false).map_err(|e| e.to_string())?;
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
