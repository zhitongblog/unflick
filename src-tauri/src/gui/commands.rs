use tauri::{command, State, AppHandle, Emitter, Manager};
use serde_json::{json, Value};

use crate::core::library;
use super::state::{GuiPlayer, PendingFile};

/// GUI playback uses HTMLVideoElement (rendered in WebView2), not mpv.
/// mpv stays available for CLI/MCP usage. This command is a no-op kept for
/// compatibility with frontend code that still calls it.
#[command]
pub async fn player_init(_app: AppHandle, _gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    Ok(json!({"status": "ready"}))
}

/// Take and clear the file Explorer asked us to open at launch.
/// Returns `null` when there's nothing pending. Single-shot: a second call
/// returns `null` so refreshes don't re-trigger playback.
#[command]
pub fn consume_pending_file(pending: State<'_, PendingFile>) -> Option<String> {
    pending.0.lock().ok().and_then(|mut g| g.take())
}

/// Move + resize the embedded video surface in physical pixels relative to
/// the parent window's client area. Frontend calls this whenever the
/// transparent video region in the WebView reflows: window resize, panel
/// slide-in, fullscreen toggle. The first successful call also unhides the
/// surface (it's created hidden by the Tauri setup hook so the WebView
/// finishes its first paint without a stray child window flashing).
#[command]
pub fn video_surface_set_geometry(
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    gui_player: State<'_, GuiPlayer>,
) -> Result<(), String> {
    if let Some(rl) = gui_player.render_loop.get() {
        rl.set_geometry(x, y, w, h).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Show or hide the video surface popup. The frontend toggles this on
/// playback state changes — popup only renders something useful while
/// a file is loaded, and hiding it during the idle state lets the
/// React drop-zone (which lives in the WebView under the popup) be
/// visible.
#[command]
pub fn video_surface_set_visible(
    visible: bool,
    gui_player: State<'_, GuiPlayer>,
) -> Result<(), String> {
    if let Some(rl) = gui_player.render_loop.get() {
        rl.set_visible(visible);
    }
    Ok(())
}

/// Set the main window — and the video popup — to always-on-top mode.
/// Settings persists this; on app start the frontend re-applies the
/// stored preference once the window is up.
#[command]
pub fn set_always_on_top(
    enabled: bool,
    app: AppHandle,
    gui_player: State<'_, GuiPlayer>,
) -> Result<(), String> {
    let window = app.get_webview_window("main").ok_or("no main window")?;
    window
        .set_always_on_top(enabled)
        .map_err(|e| e.to_string())?;
    if let Some(rl) = gui_player.render_loop.get() {
        rl.set_always_on_top(enabled);
    }
    Ok(())
}

/// Fade the video popup to a given alpha (0–255). Called by the frontend
/// when context menus / popovers open in the WebView so the menu shows
/// through the otherwise-opaque popup. 255 restores normal playback.
#[command]
pub fn video_surface_set_alpha(
    alpha: u8,
    gui_player: State<'_, GuiPlayer>,
) -> Result<(), String> {
    if let Some(rl) = gui_player.render_loop.get() {
        rl.set_alpha(alpha);
    }
    Ok(())
}

#[derive(serde::Deserialize)]
pub struct NativeMenuItem {
    pub label: String,
    #[serde(default)]
    pub separator: bool,
    #[serde(default)]
    pub disabled: bool,
}

/// Show a Win32 TrackPopupMenu at the given screen coordinates and
/// return the index of the selected item (or `null` for dismiss).
///
/// This bypasses the React-rendered context menu for one reason: the
/// React menu lives in the WebView, but the video popup is a separate
/// top-level WS_POPUP window stacked above the WebView. A React menu
/// renders behind the popup → invisible while the popup is opaque,
/// while hiding the popup blacks out the video. The OS-level menu sits
/// above every app-owned window automatically, so the video keeps
/// playing and the menu is always readable. We accept the visual
/// styling tradeoff (Windows-native vs unflick's purple theme) for the
/// z-order win.
#[cfg(target_os = "windows")]
#[command]
pub async fn show_native_context_menu(
    items: Vec<NativeMenuItem>,
    x: i32,
    y: i32,
    above: Option<bool>,
    app: AppHandle,
) -> Result<Option<u32>, String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, DestroyMenu, GetForegroundWindow, SetForegroundWindow,
        TrackPopupMenu, MF_GRAYED, MF_SEPARATOR, MF_STRING, TPM_BOTTOMALIGN, TPM_LEFTALIGN,
        TPM_NONOTIFY, TPM_RETURNCMD, TPM_TOPALIGN,
    };

    let main = app.get_webview_window("main").ok_or("no main window")?;

    // TrackPopupMenu MUST run on the message-pump thread, so dispatch it
    // through the main window's runtime and return the selection via a
    // oneshot. The call blocks the UI thread for the duration of the
    // menu (the user is interacting with it — that's expected).
    let (tx, rx) = tokio::sync::oneshot::channel();
    main.run_on_main_thread(move || {
        let result = unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                let _ = tx.send(Err("CreatePopupMenu failed".to_string()));
                return;
            }
            for (i, item) in items.iter().enumerate() {
                if item.separator {
                    AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
                } else {
                    let wide: Vec<u16> = OsStr::new(&item.label)
                        .encode_wide()
                        .chain(std::iter::once(0))
                        .collect();
                    let mut flags = MF_STRING;
                    if item.disabled {
                        flags |= MF_GRAYED;
                    }
                    // Use 1-based IDs so 0 can mean "dismissed".
                    AppendMenuW(menu, flags, (i + 1) as usize, wide.as_ptr());
                }
            }

            // TrackPopupMenu wants a foreground window as owner so the
            // menu blocks correctly. Use the *current* foreground window
            // (which should be unflick's main HWND because the user just
            // right-clicked it).
            let owner: HWND = GetForegroundWindow();
            // Quirk per MSDN: foreground window must be re-set when
            // showing menu from a non-foreground process to avoid menu
            // dismissing immediately.
            SetForegroundWindow(owner);

            let v_align = if above.unwrap_or(false) {
                TPM_BOTTOMALIGN
            } else {
                TPM_TOPALIGN
            };
            let chosen = TrackPopupMenu(
                menu,
                TPM_LEFTALIGN | v_align | TPM_RETURNCMD | TPM_NONOTIFY,
                x,
                y,
                0,
                owner,
                ptr::null(),
            );
            DestroyMenu(menu);
            Ok::<Option<u32>, String>(if chosen == 0 {
                None
            } else {
                Some(chosen as u32 - 1)
            })
        };
        let _ = tx.send(result);
    })
    .map_err(|e| e.to_string())?;

    rx.await.map_err(|e| e.to_string())?
}

/// User-data path for an auto-updated copy of yt-dlp. Updates are written here
/// so the bundled (read-only) resource stays untouched.
fn yt_dlp_user_path() -> Option<std::path::PathBuf> {
    let dir = dirs_next::data_local_dir()?.join("unflick").join("bin");
    let _ = std::fs::create_dir_all(&dir);
    let name = if cfg!(target_os = "windows") { "yt-dlp.exe" } else { "yt-dlp" };
    Some(dir.join(name))
}

/// Locate the bundled ffmpeg executable, falling back to PATH.
fn find_ffmpeg(app: &AppHandle) -> Option<std::path::PathBuf> {
    if let Ok(resource_dir) = app.path().resource_dir() {
        for sub in ["ffmpeg", ""] {
            for name in ["ffmpeg.exe", "ffmpeg"] {
                let p = if sub.is_empty() {
                    resource_dir.join(name)
                } else {
                    resource_dir.join(sub).join(name)
                };
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    let candidate = if cfg!(target_os = "windows") { "ffmpeg.exe" } else { "ffmpeg" };
    which::which(candidate).ok()
}

/// Returns true if the path string contains only ASCII characters.
fn path_is_ascii(p: &std::path::Path) -> bool {
    p.to_string_lossy().is_ascii()
}

/// Read the active Windows system proxy from registry. Returns the proxy URL
/// (e.g. "http://127.0.0.1:7890") if a proxy is enabled, else None.
#[command]
pub fn get_system_proxy() -> Result<Value, String> {
    #[cfg(target_os = "windows")]
    {
        // Read HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings
        // We use PowerShell since rusqlite/winreg aren't pulled in. Quick and
        // dirty but works for the common Clash/V2Ray/SwitchyOmega setups.
        let mut cmd = std::process::Command::new("reg");
        cmd.args([
            "query",
            "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
            "/v",
            "ProxyEnable",
        ]);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }
        let enable_out = cmd.output().map_err(|e| e.to_string())?;
        let enable_text = String::from_utf8_lossy(&enable_out.stdout);
        let enabled = enable_text.contains("0x1");
        if !enabled {
            return Ok(json!({"enabled": false, "url": null}));
        }

        let mut cmd2 = std::process::Command::new("reg");
        cmd2.args([
            "query",
            "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
            "/v",
            "ProxyServer",
        ]);
        {
            use std::os::windows::process::CommandExt;
            cmd2.creation_flags(0x08000000);
        }
        let server_out = cmd2.output().map_err(|e| e.to_string())?;
        let server_text = String::from_utf8_lossy(&server_out.stdout);

        // Output looks like:
        //   ProxyServer    REG_SZ    127.0.0.1:7890
        // Extract the value after REG_SZ
        let server = server_text
            .lines()
            .find(|l| l.contains("REG_SZ"))
            .and_then(|l| l.split("REG_SZ").nth(1))
            .map(|s| s.trim().to_string());

        if let Some(s) = server.filter(|s| !s.is_empty()) {
            // Add http:// scheme if absent
            let url = if s.starts_with("http://") || s.starts_with("https://") || s.starts_with("socks5://") {
                s
            } else {
                format!("http://{}", s)
            };
            return Ok(json!({"enabled": true, "url": url}));
        }
        Ok(json!({"enabled": false, "url": null}))
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Could parse $http_proxy / gsettings on Linux, networksetup on macOS.
        // Skipped for now — return disabled.
        Ok(json!({"enabled": false, "url": null}))
    }
}

/// Mirror a source directory's files into `%LOCALAPPDATA%\unflick\bin\<sub>\`
/// so native CLIs (whisper-cli, ffmpeg) get an ASCII-safe path even when the
/// app is installed under a folder containing non-ASCII characters. Only
/// copies files whose size or mtime differs from the destination.
fn mirror_dir_to_safe(src: &std::path::Path, sub: &str) -> std::io::Result<std::path::PathBuf> {
    let dst = dirs_next::data_local_dir()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no local data dir"))?
        .join("unflick")
        .join("bin")
        .join(sub);
    std::fs::create_dir_all(&dst)?;
    for entry in std::fs::read_dir(src)? {
        let e = entry?;
        let from = e.path();
        if !from.is_file() {
            continue;
        }
        let Some(name) = from.file_name() else { continue };
        let to = dst.join(name);
        let from_meta = std::fs::metadata(&from)?;
        let needs = match std::fs::metadata(&to) {
            Ok(m) => m.len() != from_meta.len(),
            Err(_) => true,
        };
        if needs {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(dst)
}

/// Get an ASCII-safe path to ffmpeg.exe. Mirrors the bundled binary into
/// %LOCALAPPDATA% if the source path contains non-ASCII characters.
fn ffmpeg_safe_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let src = find_ffmpeg(app).ok_or_else(|| "ffmpeg not found".to_string())?;
    if path_is_ascii(&src) {
        return Ok(src);
    }
    let parent = src.parent().ok_or_else(|| "ffmpeg has no parent dir".to_string())?;
    let safe_dir = mirror_dir_to_safe(parent, "ffmpeg").map_err(|e| e.to_string())?;
    let name = src.file_name().ok_or_else(|| "ffmpeg has no filename".to_string())?;
    Ok(safe_dir.join(name))
}

/// Get an ASCII-safe path to the whisper-cli directory (with all DLLs and
/// the model side-by-side). Returns (binary_path, model_path).
fn whisper_safe_paths(
    binary: &str,
    model: &str,
) -> Result<(std::path::PathBuf, std::path::PathBuf), String> {
    let bin_path = std::path::Path::new(binary);
    let model_path = std::path::Path::new(model);

    // If both paths are already ASCII, no mirroring needed.
    if path_is_ascii(bin_path) && path_is_ascii(model_path) {
        return Ok((bin_path.to_path_buf(), model_path.to_path_buf()));
    }

    let parent = bin_path.parent().ok_or_else(|| "whisper has no parent dir".to_string())?;
    // Mirror the whole folder so DLLs come along with the .exe
    let safe_dir = mirror_dir_to_safe(parent, "whisper").map_err(|e| e.to_string())?;
    // Also ensure model is in safe dir (might be elsewhere)
    let model_in_safe = if model_path.starts_with(parent) {
        // Model lives next to the binary — already mirrored
        let name = model_path.file_name().ok_or_else(|| "model has no filename".to_string())?;
        safe_dir.join(name)
    } else if path_is_ascii(model_path) {
        model_path.to_path_buf()
    } else {
        // Model in a different non-ASCII path: copy it to safe_dir
        let name = model_path.file_name().ok_or_else(|| "model has no filename".to_string())?;
        let dst = safe_dir.join(name);
        if let Ok(meta) = std::fs::metadata(&dst) {
            if meta.len()
                != std::fs::metadata(model_path)
                    .map_err(|e| e.to_string())?
                    .len()
            {
                std::fs::copy(model_path, &dst).map_err(|e| e.to_string())?;
            }
        } else {
            std::fs::copy(model_path, &dst).map_err(|e| e.to_string())?;
        }
        dst
    };

    let bin_name = bin_path
        .file_name()
        .ok_or_else(|| "whisper-cli has no filename".to_string())?;
    Ok((safe_dir.join(bin_name), model_in_safe))
}

/// Locate the yt-dlp executable. Priority order:
/// 1. User data dir (auto-updated copy — newest version)
/// 2. Bundled `yt-dlp/` subdir of the app's resource directory (shipped version)
/// 3. PATH (system-installed)
fn find_yt_dlp(app: &AppHandle) -> Option<std::path::PathBuf> {
    // 1. User-data updated copy
    if let Some(p) = yt_dlp_user_path() {
        if p.exists() {
            return Some(p);
        }
    }
    // 2. Bundled inside the app
    if let Ok(resource_dir) = app.path().resource_dir() {
        for sub in ["yt-dlp", ""] {
            for name in ["yt-dlp.exe", "yt-dlp"] {
                let p = if sub.is_empty() {
                    resource_dir.join(name)
                } else {
                    resource_dir.join(sub).join(name)
                };
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    // 3. PATH
    let candidate = if cfg!(target_os = "windows") { "yt-dlp.exe" } else { "yt-dlp" };
    which::which(candidate).ok()
}

/// Run yt-dlp --version to read the installed version string.
fn yt_dlp_version(yt_dlp: &std::path::Path) -> Option<String> {
    let mut cmd = std::process::Command::new(yt_dlp);
    cmd.arg("--version");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Get installed yt-dlp version + which copy is in use.
#[command]
pub async fn yt_dlp_info(app: AppHandle) -> Result<Value, String> {
    let path = find_yt_dlp(&app);
    if let Some(p) = path {
        let version = tokio::task::spawn_blocking({
            let p = p.clone();
            move || yt_dlp_version(&p)
        })
        .await
        .ok()
        .flatten();
        let source = if let Some(user) = yt_dlp_user_path() {
            if p == user { "user" } else if which::which(p.file_name().unwrap_or_default()).ok().as_deref() == Some(&p) { "path" } else { "bundled" }
        } else {
            "bundled"
        };
        Ok(json!({
            "available": true,
            "path": p.to_string_lossy(),
            "version": version,
            "source": source,
        }))
    } else {
        Ok(json!({"available": false}))
    }
}

/// Download the latest yt-dlp from GitHub releases into the user-data
/// directory, replacing any previous auto-updated copy.
/// `proxy` is forwarded to the download for users behind a firewall.
#[command]
pub async fn update_yt_dlp(proxy: Option<String>) -> Result<Value, String> {
    let dest = yt_dlp_user_path().ok_or_else(|| "no user data dir".to_string())?;
    let url = if cfg!(target_os = "windows") {
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe"
    } else if cfg!(target_os = "macos") {
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos"
    } else {
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp"
    };

    let dest_for_response = dest.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let mut builder = ureq::AgentBuilder::new()
            .timeout_connect(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(120));
        if let Some(p) = proxy.as_deref().filter(|s| !s.trim().is_empty()) {
            let proxy_obj = ureq::Proxy::new(p).map_err(|e| format!("invalid proxy: {}", e))?;
            builder = builder.proxy(proxy_obj);
        }
        let agent = builder.build();
        let resp = agent.get(url).call().map_err(|e| format!("download failed: {}", e))?;
        let tmp = dest.with_extension("download");
        if let Some(parent) = tmp.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let mut writer = std::fs::File::create(&tmp).map_err(|e| format!("create tmp: {}", e))?;
        let mut reader = resp.into_reader();
        std::io::copy(&mut reader, &mut writer).map_err(|e| format!("write tmp: {}", e))?;
        drop(writer);
        if dest.exists() {
            let _ = std::fs::remove_file(&dest);
        }
        std::fs::rename(&tmp, &dest).map_err(|e| format!("rename: {}", e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;

    Ok(json!({"path": dest_for_response.to_string_lossy(), "updated": true}))
}

/// Check whether yt-dlp is available (bundled or on PATH).
#[command]
pub fn check_yt_dlp(app: AppHandle) -> Result<Value, String> {
    match find_yt_dlp(&app) {
        Some(path) => Ok(json!({
            "available": true,
            "path": path.to_string_lossy(),
        })),
        None => Ok(json!({"available": false})),
    }
}

/// Run yt-dlp to extract a direct stream URL from an upstream page (YouTube,
/// Bilibili, Twitch VOD, etc.). The returned URL can be fed straight to a
/// `<video>` element. `proxy` is optional and forwarded as `--proxy` if set.
#[command]
pub async fn extract_stream_url(
    app: AppHandle,
    url: String,
    proxy: Option<String>,
) -> Result<Value, String> {
    let yt_dlp = find_yt_dlp(&app).ok_or_else(|| {
        "yt-dlp not found. Install it from https://github.com/yt-dlp/yt-dlp or place yt-dlp.exe next to unflick.".to_string()
    })?;

    // Resolve proxy: "system" means read the OS proxy settings; empty/None means
    // no proxy; otherwise use the literal URL the user supplied.
    let effective_proxy = match proxy.as_deref() {
        Some("system") => {
            match get_system_proxy() {
                Ok(v) => v.get("url").and_then(|u| u.as_str()).map(|s| s.to_string()),
                Err(_) => None,
            }
        }
        Some(s) if !s.trim().is_empty() => Some(s.to_string()),
        _ => None,
    };

    let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let mut cmd = std::process::Command::new(&yt_dlp);
        cmd.arg("--get-url")
            .arg("--no-playlist")
            .arg("--no-warnings")
            // Prefer a single-URL format (avoids separate audio + video streams)
            .arg("-f")
            .arg("best[ext=mp4]/best");
        if let Some(p) = effective_proxy.as_deref().filter(|s| !s.trim().is_empty()) {
            cmd.arg("--proxy").arg(p);
        }
        cmd.arg(&url);

        // Suppress console window on Windows
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        let output = cmd.output().map_err(|e| format!("yt-dlp failed to run: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("yt-dlp error: {}", stderr.trim()));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        // yt-dlp may emit multiple URLs (one per format); take the first.
        let stream_url = stdout
            .lines()
            .find(|l| !l.trim().is_empty())
            .ok_or_else(|| "yt-dlp returned no URL".to_string())?
            .to_string();
        Ok(stream_url)
    })
    .await
    .map_err(|e| e.to_string())??;

    Ok(json!({"stream_url": result}))
}


#[command]
pub fn player_play(
    file: String,
    seek: Option<f64>,
    volume: Option<i64>,
    speed: Option<f64>,
    gui_player: State<'_, GuiPlayer>,
) -> Result<Value, String> {
    eprintln!("[unflick] player_play file={file:?} seek={seek:?}");
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    player.play(&file, seek, volume, speed).map_err(|e| e.to_string())?;
    eprintln!("[unflick] player_play loadfile dispatched");
    Ok(json!({"message": format!("playing {}", file)}))
}

#[command]
pub fn player_pause(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    player.pause().map_err(|e| e.to_string())?;
    Ok(json!({"message": "paused"}))
}

#[command]
pub fn player_resume(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    player.resume().map_err(|e| e.to_string())?;
    Ok(json!({"message": "resumed"}))
}

#[command]
pub fn player_stop(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    player.stop().map_err(|e| e.to_string())?;
    Ok(json!({"message": "stopped"}))
}

#[command]
pub fn player_seek(seconds: f64, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    player.seek(seconds).map_err(|e| e.to_string())?;
    Ok(json!({"message": format!("seeked to {}s", seconds)}))
}

#[command]
pub fn player_set_volume(level: i64, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    player.set_volume(level).map_err(|e| e.to_string())?;
    Ok(json!({"message": format!("volume set to {}", level)}))
}

#[command]
pub fn player_set_speed(rate: f64, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    player.set_speed(rate).map_err(|e| e.to_string())?;
    Ok(json!({"message": format!("speed set to {}x", rate)}))
}

#[command]
pub fn player_status(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    let status = player.status();
    serde_json::to_value(&status).map_err(|e| e.to_string())
}

#[command]
pub fn player_screenshot(output: Option<String>, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
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
pub fn toggle_pip(app: AppHandle, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::OnceLock;
    // Track PiP state independently of always-on-top. The previous
    // implementation inferred "in PiP" from is_always_on_top(), which
    // breaks the moment the user enables the global Always-on-Top
    // setting — toggle_pip then routes into the *exit* branch even
    // when not in PiP, so the button visibly does nothing the second
    // time it's pressed and PiP can only be undone via the right-click
    // menu's separate handler.
    static IN_PIP: OnceLock<AtomicBool> = OnceLock::new();
    static PRE_AOT: OnceLock<AtomicBool> = OnceLock::new();
    let in_pip = IN_PIP.get_or_init(|| AtomicBool::new(false));
    let pre_aot = PRE_AOT.get_or_init(|| AtomicBool::new(false));

    let window = app.get_webview_window("main").ok_or("no main window")?;

    if in_pip.load(Ordering::Relaxed) {
        // Exit PiP: restore the AOT state we captured on entry + normal size
        let restore_aot = pre_aot.load(Ordering::Relaxed);
        window
            .set_always_on_top(restore_aot)
            .map_err(|e| e.to_string())?;
        if let Some(rl) = gui_player.render_loop.get() {
            rl.set_always_on_top(restore_aot);
        }
        let _ = window.set_size(tauri::LogicalSize::new(1024.0, 640.0));
        let _ = window.center();
        in_pip.store(false, Ordering::Relaxed);
        Ok(json!({"pip": false}))
    } else {
        // Enter PiP: capture current AOT so we can restore on exit,
        // then force AOT on + shrink + park bottom-right.
        let cur_aot = window.is_always_on_top().unwrap_or(false);
        pre_aot.store(cur_aot, Ordering::Relaxed);
        window.set_always_on_top(true).map_err(|e| e.to_string())?;
        if let Some(rl) = gui_player.render_loop.get() {
            rl.set_always_on_top(true);
        }
        let _ = window.set_size(tauri::LogicalSize::new(400.0, 250.0));
        // Position bottom-right of the current monitor instead of a fixed
        // (1400, 700) hard-coded for a 1080p Windows display. The original
        // value would land off-screen on a small mac display.
        if let Some(monitor) = window.current_monitor().ok().flatten() {
            let scale = monitor.scale_factor();
            let mon_size = monitor.size();
            let mon_pos = monitor.position();
            // Convert physical → logical for set_position.
            let mon_w_logical = mon_size.width as f64 / scale;
            let mon_h_logical = mon_size.height as f64 / scale;
            let mon_x_logical = mon_pos.x as f64 / scale;
            let mon_y_logical = mon_pos.y as f64 / scale;
            // 16 px gap from screen edges.
            let x = mon_x_logical + mon_w_logical - 400.0 - 16.0;
            let y = mon_y_logical + mon_h_logical - 250.0 - 60.0; // extra room for taskbar/dock
            let _ = window.set_position(tauri::LogicalPosition::new(x, y));
        } else {
            let _ = window.center();
        }
        in_pip.store(true, Ordering::Relaxed);
        Ok(json!({"pip": true}))
    }
}

#[command]
pub fn set_fullscreen(app: AppHandle) -> Result<Value, String> {
    let window = app.get_webview_window("main").ok_or("no main window")?;
    let is_fullscreen = window.is_fullscreen().map_err(|e| e.to_string())?;
    let target = !is_fullscreen;
    window.set_fullscreen(target).map_err(|e| e.to_string())?;
    // Tell the frontend so it can hide TitleBar / PlayerBar — going
    // through a Rust-emitted event handles every entry point (F key,
    // double-click, native menu item, PlayerBar button) uniformly.
    let _ = app.emit("unflick:fullscreen-changed", target);
    Ok(json!({"fullscreen": target}))
}

#[command]
pub fn exit_fullscreen(app: AppHandle) -> Result<Value, String> {
    let window = app.get_webview_window("main").ok_or("no main window")?;
    let is_fullscreen = window.is_fullscreen().map_err(|e| e.to_string())?;
    if is_fullscreen {
        window.set_fullscreen(false).map_err(|e| e.to_string())?;
        let _ = app.emit("unflick:fullscreen-changed", false);
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

/// Multi-select variant of the open dialog. Used by the Playlist panel
/// so the user can hold Shift / Ctrl to drop a batch of files in at
/// once instead of clicking Add → Browse → pick one over and over.
#[command]
pub async fn open_files_dialog() -> Result<Value, String> {
    let result = tokio::task::spawn_blocking(|| {
        rfd::FileDialog::new()
            .add_filter("Video", &["mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "ts", "mpg", "mpeg"])
            .add_filter("All Files", &["*"])
            .pick_files()
    })
    .await
    .map_err(|e| e.to_string())?;

    match result {
        Some(paths) => {
            let stringified: Vec<String> = paths
                .into_iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            Ok(json!({ "paths": stringified }))
        }
        None => Ok(json!({ "paths": [] })),
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

/// Open the Windows "Default apps" Settings page, filtered to unflick on
/// Windows 11. Modern Windows refuses to set associations programmatically
/// (anti-malware), so the most we can do is jump the user to the page where
/// they can flip the toggles per file type.
///
/// On Windows 10 the registeredAppUser query is ignored and the user lands
/// on the generic page, which is still better than nothing.
#[command]
pub fn open_default_apps_settings() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        // `ms-settings:` is a URI handler, not an executable, so we route
        // through `cmd /c start` which dispatches it to the OS shell.
        let mut cmd = std::process::Command::new("cmd");
        cmd.args([
            "/c",
            "start",
            "",
            "ms-settings:defaultapps?registeredAppUser=unflick",
        ]);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW — hide the cmd flash
        }
        cmd.spawn().map_err(|e| format!("failed to open settings: {}", e))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        return Err("Setting default app from inside the app is Windows-only for now".to_string());
    }
    Ok(())
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
pub fn library_clear(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let db_lock = gui_player.db.lock().unwrap();
    let db = db_lock.as_ref().ok_or("database not available")?;
    let removed = db.clear_all().map_err(|e| e.to_string())?;
    Ok(json!({"removed": removed}))
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
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    player.subtitle_load(&path).map_err(|e| e.to_string())?;
    Ok(json!({"message": format!("subtitle loaded: {}", path)}))
}

#[command]
pub fn subtitle_list(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    let tracks = player.subtitle_list();
    serde_json::to_value(&tracks).map_err(|e| e.to_string())
}

#[command]
pub fn subtitle_select(id: i64, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
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
            // Active player drives the new track. Pre-pipeline-init we
            // silently skip — the playlist still advances.
            if let Ok(player) = gui_player.mpv() {
                let _ = player.play(&path, None, None, None);
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
            if let Ok(player) = gui_player.mpv() {
                let _ = player.play(&path, None, None, None);
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
    if let Ok(player) = gui_player.mpv() {
        let _ = player.play(&path, None, None, None);
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
pub async fn extract_clip(
    app: AppHandle,
    input: String,
    start: f64,
    end: f64,
    output: String,
    as_gif: bool,
) -> Result<Value, String> {
    let ffmpeg = ffmpeg_safe_path(&app)?;
    let ffmpeg_path = ffmpeg.to_string_lossy().to_string();

    let result = tokio::task::spawn_blocking(move || {
        crate::core::player::extract_clip(&input, start, end, &output, as_gif, &ffmpeg_path)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    Ok(json!({"output": result}))
}

#[command]
pub fn write_file_bytes(path: String, bytes: Vec<u8>) -> Result<Value, String> {
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    Ok(json!({"path": path, "size": bytes.len()}))
}

/// Find sidecar subtitle files alongside a video. Looks for files in the
/// same directory whose name starts with the video's basename and ends with
/// a known subtitle extension. Common patterns:
///   movie.mp4   →  movie.srt, movie.vtt, movie.en.srt, movie.zh-Hans.ass …
#[command]
pub fn find_sidecar_subtitles(video_path: String) -> Result<Value, String> {
    let path = std::path::Path::new(&video_path);
    let parent = match path.parent() {
        Some(p) => p,
        None => return Ok(json!({"subtitles": []})),
    };
    let stem = match path.file_stem().and_then(|s| s.to_str()) {
        Some(s) => s.to_lowercase(),
        None => return Ok(json!({"subtitles": []})),
    };

    const SUB_EXT: &[&str] = &["srt", "vtt", "ass", "ssa", "sub"];

    let mut matches = Vec::new();
    let entries = match std::fs::read_dir(parent) {
        Ok(it) => it,
        Err(_) => return Ok(json!({"subtitles": []})),
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let ext = match p.extension().and_then(|e| e.to_str()) {
            Some(e) => e.to_lowercase(),
            None => continue,
        };
        if !SUB_EXT.contains(&ext.as_str()) {
            continue;
        }
        let name_lower = match p.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_lowercase(),
            None => continue,
        };
        // Match if the subtitle filename starts with the video's basename
        // (handles both "movie.srt" and "movie.en.srt")
        if name_lower.starts_with(&stem) {
            // Extract language hint between basename and extension, if any
            // e.g. "movie.en.srt" → "en"
            let rest = &name_lower[stem.len()..];
            let lang = rest
                .trim_start_matches('.')
                .trim_end_matches(&format!(".{}", ext))
                .trim_end_matches(ext.as_str())
                .trim_end_matches('.')
                .to_string();
            matches.push(json!({
                "path": p.to_string_lossy(),
                "lang": if lang.is_empty() { Value::Null } else { json!(lang) },
                "ext": ext,
            }));
        }
    }
    Ok(json!({"subtitles": matches}))
}

/// Read a text file (subtitle, transcript, etc.) and return its contents.
/// Tries UTF-8 first, falls back to GBK for Chinese subtitle files.
#[command]
pub fn read_text_file(path: String) -> Result<Value, String> {
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let text = match std::str::from_utf8(&bytes) {
        Ok(s) => s.to_string(),
        Err(_) => {
            // Most legacy Chinese SRTs are GB18030. Tauri 2 doesn't ship an
            // encoding decoder, so we fall back to a lossy ISO-8859-1 read.
            // For UTF-16 BOM-prefixed files, strip the BOM byte-pair first.
            if bytes.starts_with(&[0xFF, 0xFE]) {
                let u16s: Vec<u16> = bytes[2..]
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16_lossy(&u16s)
            } else if bytes.starts_with(&[0xFE, 0xFF]) {
                let u16s: Vec<u16> = bytes[2..]
                    .chunks_exact(2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16_lossy(&u16s)
            } else {
                String::from_utf8_lossy(&bytes).to_string()
            }
        }
    };
    Ok(json!({"text": text, "bytes": bytes.len()}))
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
    app: AppHandle,
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

    let ffmpeg = ffmpeg_safe_path(&app)?;
    let ffmpeg_path = ffmpeg.to_string_lossy().to_string();

    let srt_path = tokio::task::spawn_blocking(move || {
        match mode.as_str() {
            "local" => {
                // Fall back to the bundled whisper binary/model if the frontend
                // didn't pass paths. This matches the CLI/MCP behavior so the
                // GUI works out of the box when the AI installer is used.
                let (binary, model) = match (whisper_binary, model_path) {
                    (Some(b), Some(m)) => (b, m),
                    _ => match crate::core::whisper::find_bundled_whisper() {
                        Some((b, m)) => (
                            b.to_string_lossy().into_owned(),
                            m.to_string_lossy().into_owned(),
                        ),
                        None => anyhow::bail!(
                            "whisper binary/model path not set and no bundled whisper found"
                        ),
                    },
                };
                let (safe_bin, safe_model) = whisper_safe_paths(&binary, &model)
                    .map_err(|e| anyhow::anyhow!(e))?;
                let safe_bin_str = safe_bin.to_string_lossy().to_string();
                let safe_model_str = safe_model.to_string_lossy().to_string();
                crate::core::whisper::transcribe_local(
                    &video_path,
                    &safe_bin_str,
                    &safe_model_str,
                    &output_dir,
                    &ffmpeg_path,
                )
            }
            "api" => {
                let key = api_key
                    .ok_or_else(|| anyhow::anyhow!("OpenAI API key not set"))?;
                crate::core::whisper::transcribe_api(
                    &video_path,
                    &key,
                    &output_dir,
                    &ffmpeg_path,
                )
            }
            _ => anyhow::bail!("unknown transcription mode: {}", mode),
        }
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    Ok(json!({"srt_path": srt_path}))
}

// ─── AI subtitle translation ─────────────────────────────────────────────────

#[command]
pub async fn translate_subtitles(
    srt_path: String,
    target_lang: String,
    api_key: String,
) -> Result<Value, String> {
    let output_dir = dirs_next::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("unflick")
        .to_string_lossy()
        .to_string();
    std::fs::create_dir_all(&output_dir).map_err(|e| e.to_string())?;

    let result = tokio::task::spawn_blocking(move || {
        crate::core::whisper::translate_srt(&srt_path, &target_lang, &api_key, &output_dir)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    Ok(json!({"translated_srt": result}))
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
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    let tracks = player.audio_list();
    serde_json::to_value(&tracks).map_err(|e| e.to_string())
}

#[command]
pub fn audio_select(id: i64, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    player.audio_select(id).map_err(|e| e.to_string())?;
    Ok(json!({"message": format!("audio track {} selected", id)}))
}

/// Check for available updates
#[command]
pub async fn check_for_updates(_app: AppHandle) -> Result<Value, String> {
    // Full auto-update with signing can be configured once release keys are generated.
    // For now, direct users to the releases page.
    Ok(json!({
        "message": "Update check not yet configured. Visit https://github.com/zhitongblog/unflick/releases for the latest version."
    }))
}

// ─── Video filter commands ─────────────────────────────────────────────────────

#[command]
pub fn set_video_filter(name: String, value: i64, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    player.set_property_i64(&name, value).map_err(|e| e.to_string())?;
    Ok(json!({"filter": name, "value": value}))
}

#[command]
pub fn get_video_filters(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    let brightness = player.get_property_i64("brightness").unwrap_or(0);
    let contrast = player.get_property_i64("contrast").unwrap_or(0);
    let saturation = player.get_property_i64("saturation").unwrap_or(0);
    let gamma = player.get_property_i64("gamma").unwrap_or(0);
    let hue = player.get_property_i64("hue").unwrap_or(0);
    Ok(json!({
        "brightness": brightness,
        "contrast": contrast,
        "saturation": saturation,
        "gamma": gamma,
        "hue": hue,
    }))
}

#[command]
pub fn reset_video_filters(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    for prop in ["brightness", "contrast", "saturation", "gamma", "hue"] {
        let _ = player.set_property_i64(prop, 0);
    }
    Ok(json!({"message": "filters reset"}))
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
