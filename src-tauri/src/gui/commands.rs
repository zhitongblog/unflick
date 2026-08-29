use std::sync::Arc;

use tauri::{command, State, AppHandle, Emitter, Manager};
use serde_json::{json, Value};

use crate::core::library;
use super::state::{GuiPlayer, PendingFile, StartupOpen};

/// GUI playback uses HTMLVideoElement (rendered in WebView2), not mpv.
/// mpv stays available for CLI/MCP usage. This command is a no-op kept for
/// compatibility with frontend code that still calls it.
#[command]
pub async fn player_init(_app: AppHandle, _gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    Ok(json!({"status": "ready"}))
}

/// Let the WebView write onto the same startup timeline Rust writes to.
///
/// Most of a cold start is spent in places Rust cannot see: creating the
/// WebView, parsing the bundle, mounting React. Without a mark from the
/// other side of the IPC boundary those phases are one unattributed gap.
#[command]
pub fn boot_mark(label: String) {
    crate::core::boot::mark(&format!("ui: {label}"));
}

/// Take and clear the record of the file Explorer asked us to open.
///
/// By the time the frontend can ask, the backend has already opened it —
/// see `PendingFile`. So this reports rather than instructs: the caller
/// adopts the file that is playing, or shows `error` if it would not open.
/// Returns `null` when the launch had no file. Single-shot, so a refresh
/// does not replay anything.
#[command]
pub fn consume_pending_file(pending: State<'_, PendingFile>) -> Option<StartupOpen> {
    pending.take_outcome()
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
///
/// Response shape (additive — old callers see `stream_url` and ignore the
/// new optional fields):
///   `{ "stream_url": "https://...", "error_kind"?: string, "error_message"?: string }`
///
/// On categorized failure, `stream_url` is empty and the two error fields
/// describe what went wrong. The Tauri command itself returns Ok(...) in
/// both success and categorized-failure cases so the frontend can switch
/// on `error_kind` instead of parsing free-form error strings.
///
/// `quality` (optional, snake_case): `"auto"` | `"2160p"` | `"1440p"` |
/// `"1080p"` | `"720p"` | `"480p"` | `"audio_only"`. `None` or `"auto"`
/// falls back to `core::settings::preferred_quality()`. Numeric values
/// cap the format height; `"audio_only"` triggers `-x --audio-format m4a`.
///
/// `cookies_browser` (optional, snake_case): `"none"` | `"firefox"` |
/// `"chrome"` | `"chromium"` | `"safari"` | `"edge"` | `"brave"`. `None`
/// or `"none"` falls back to `core::settings::cookies_browser()`; that
/// `None` means no cookie injection. Browser must be closed when
/// extraction runs (yt-dlp limitation).
#[command]
pub async fn extract_stream_url(
    app: AppHandle,
    url: String,
    proxy: Option<String>,
    quality: Option<String>,
    cookies_browser: Option<String>,
) -> Result<Value, String> {
    let yt_dlp = find_yt_dlp(&app).ok_or_else(|| {
        "yt-dlp not found. Install it from https://github.com/yt-dlp/yt-dlp or place yt-dlp.exe next to unflick.".to_string()
    })?;

    // Resolve proxy: "system" means read the OS proxy settings; empty/None
    // means no proxy; otherwise use the literal URL the user supplied.
    let effective_proxy = match proxy.as_deref() {
        Some("system") => match get_system_proxy() {
            Ok(v) => v.get("url").and_then(|u| u.as_str()).map(|s| s.to_string()),
            Err(_) => None,
        },
        Some(s) if !s.trim().is_empty() => Some(s.to_string()),
        _ => None,
    };

    // Resolve effective quality + cookies-from-browser, falling back to
    // saved settings when the per-call arg is absent. `"auto"`/`"none"`
    // sentinels mean "force off, ignore the saved setting too" so a
    // one-off dialog override can opt out.
    let effective_quality: Option<String> = match quality.as_deref().map(str::trim) {
        Some("") | None => crate::core::settings::preferred_quality(),
        Some("auto") => None,
        Some(s) => Some(s.to_string()),
    };
    let effective_cookies_browser: Option<String> =
        match cookies_browser.as_deref().map(str::trim) {
            Some("") | None => crate::core::settings::cookies_browser(),
            Some("none") => None,
            Some(s) => Some(s.to_string()),
        };

    let result = crate::core::yt_dlp::extract_stream_url(
        &yt_dlp,
        &url,
        effective_proxy.as_deref(),
        effective_quality.as_deref(),
        effective_cookies_browser.as_deref(),
    )
    .await;
    Ok(serde_json::to_value(&result).unwrap_or_else(|_| json!({"stream_url": ""})))
}

/// Cancel any in-flight yt-dlp extraction (set a global flag; the wait loop
/// kills the child within ~100ms). Safe to call when nothing is running.
#[command]
pub fn cancel_url_extraction() -> Result<Value, String> {
    let cancelled = crate::core::yt_dlp::cancel();
    Ok(json!({"cancelled": cancelled}))
}

/// After the GUI plays a resolved stream URL, the frontend calls this with
/// the *original* page URL so we can fire SponsorBlock + auto-subtitle
/// hooks on the live render-thread Player. The CLI/MCP daemon does this
/// itself — see `core::daemon::dispatch_command`'s `play` arm — but the
/// GUI's split (extract-then-play sequence) would otherwise lose the
/// page URL by the time playback starts.
#[command]
pub fn arm_post_play_hooks(
    app: AppHandle,
    url: String,
    gui_player: State<'_, GuiPlayer>,
) -> Result<Value, String> {
    if !crate::core::yt_dlp::is_http_url(&url) {
        return Ok(json!({"armed": false, "reason": "not a URL"}));
    }
    // Reach into render_player directly because GuiPlayer::mpv() returns
    // &Player but the hook needs an owned Arc to keep the player alive
    // across the spawned tokio tasks.
    let player_arc = gui_player
        .render_player
        .get()
        .ok_or_else(|| "render player not initialised".to_string())?
        .clone();
    let yt_dlp_path = find_yt_dlp(&app);
    let settings = crate::core::url_post_play::read_settings_snapshot();
    crate::core::url_post_play::after_play_url_hooks(player_arc, url, yt_dlp_path, settings);
    Ok(json!({"armed": true}))
}


/// Runs off the main thread: `play` now waits for mpv's verdict, and an
/// unreachable share takes that wait to its full deadline. A sync command
/// would freeze the window for the duration.
#[command(async)]
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
    crate::core::boot::mark("play: mpv reports the file loaded");
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
    let mut value = serde_json::to_value(&status).map_err(|e| e.to_string())?;

    // Fold the A-B loop and timing offsets into the same poll the frontend
    // already runs. These are no longer GUI-owned state: since v0.10 the
    // CLI and MCP drive this very player, so `unflick loop a` or an agent
    // calling `subtitle_delay` has to show up on screen. Reading them here
    // costs three extra property lookups on a poll that was happening
    // anyway, and avoids a second round-trip.
    if let Some(obj) = value.as_object_mut() {
        obj.insert("ab_loop".into(), json!(player.ab_loop_status()));
        obj.insert("sub_delay".into(), json!(player.sub_delay()));
        obj.insert("audio_delay".into(), json!(player.audio_delay()));
        // Current chapter index, so the chapter list can highlight the
        // right row as playback crosses a boundary on its own.
        let chapters = player.chapter_list();
        obj.insert(
            "chapter".into(),
            json!(chapters.iter().position(|c| c.current).map(|i| i as i64)),
        );
        // Count too, so the frontend can notice chapters appearing without
        // re-fetching the whole list every 250 ms. An agent calling
        // generate_chapters mid-playback should put ticks on the progress
        // bar while the user is watching, not on the next file.
        obj.insert("chapter_count".into(), json!(chapters.len()));
    }
    Ok(value)
}

#[command]
pub fn player_screenshot(output: Option<String>, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    // `mut` is only exercised by the Linux branch below; without this the
    // other two platforms warn on every build.
    #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
    let mut path = output.unwrap_or_else(|| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        format!("unflick-screenshot-{}.png", ts)
    });
    // Linux: if the caller passed a bare filename (no leading /), drop it
    // into ~/Pictures/unflick/. The frontend skips the GTK save dialog on
    // Linux and just hands us the filename — see the
    // `captureScreenshot` comment in App.tsx for why. mpv's
    // `screenshot-to-file` would otherwise try to write into the
    // process CWD which is `/` for system installs.
    #[cfg(target_os = "linux")]
    {
        if !path.starts_with('/') {
            if let Some(home) = dirs_next::home_dir() {
                let dir = home.join("Pictures").join("unflick");
                let _ = std::fs::create_dir_all(&dir);
                path = dir.join(&path).to_string_lossy().into_owned();
            }
        }
    }
    player.screenshot(&path).map_err(|e| e.to_string())?;
    Ok(json!({"path": path}))
}

/// Toggle picture-in-picture. Thin wrapper: `gui::window` owns the modes and
/// the geometry to come back to, so this and `unflick window mode pip` land
/// in exactly the same place.
#[command]
pub fn toggle_pip(
    host: State<'_, Arc<crate::gui::window::TauriWindowHost>>,
) -> Result<Value, String> {
    let mode = host.toggle(crate::core::window::WindowMode::Pip)?;
    Ok(json!({"pip": mode == crate::core::window::WindowMode::Pip, "mode": mode.as_str()}))
}

/// Read or set the window mode from the UI. Same surface as the CLI's
/// `window mode`, against the same state.
#[command]
pub fn window_mode(
    mode: Option<String>,
    host: State<'_, Arc<crate::gui::window::TauriWindowHost>>,
) -> Result<Value, String> {
    use crate::core::window::{WindowHost, WindowMode};
    let target = match mode {
        None => host.mode(),
        Some(m) => {
            let parsed: WindowMode = m.parse()?;
            host.set_mode(parsed)?;
            parsed
        }
    };
    Ok(json!({"mode": target.as_str()}))
}

/// Flip between music mode and normal — what the button and the hotkey want.
#[command]
pub fn toggle_music_mode(
    host: State<'_, Arc<crate::gui::window::TauriWindowHost>>,
) -> Result<Value, String> {
    let mode = host.toggle(crate::core::window::WindowMode::Music)?;
    Ok(json!({"mode": mode.as_str()}))
}

/// What an earlier install left behind, and how much it is worth.
///
/// Split from `cleanup_apply` rather than taking a flag, so that showing the
/// number can never be one typo away from deleting half a gigabyte.
#[command]
pub fn cleanup_scan() -> Result<Value, String> {
    serde_json::to_value(crate::core::cleanup::scan()).map_err(|e| e.to_string())
}

#[command]
pub fn cleanup_apply() -> Result<Value, String> {
    let report = crate::core::cleanup::remove_leftovers().map_err(|e| e.to_string())?;
    serde_json::to_value(report).map_err(|e| e.to_string())
}

/// Tags and cover art for whatever is loaded — what music mode renders.
///
/// The cover comes back as a data URL on top of its path: the webview cannot
/// read an arbitrary file off disk, and the timeline previews already take
/// this route. It is one small JPEG per file, fetched once when the file
/// changes.
#[command]
pub fn now_playing(
    cover: Option<bool>,
    gui_player: State<'_, GuiPlayer>,
) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    let np = crate::core::nowplaying::now_playing(&player, cover.unwrap_or(true));
    let mut value = serde_json::to_value(&np).map_err(|e| e.to_string())?;

    if let Some(path) = np.cover.as_deref() {
        if let Ok(bytes) = std::fs::read(path) {
            value["cover_data_url"] = json!(format!(
                "data:image/jpeg;base64,{}",
                crate::core::vision::base64_encode(&bytes)
            ));
        }
    }
    Ok(value)
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
    // Duration comes from the live player rather than the caller: the
    // "is this file finished?" rule lives in `db::remember_position` so
    // GUI, CLI and MCP can't drift apart on it. Callers just report where
    // playback got to. A duration of 0 (unknown / already unloaded) makes
    // the rule skip its end-of-file check, which is the safe direction.
    let duration = gui_player
        .mpv()
        .map(|p| p.status().duration)
        .unwrap_or(0.0);
    let db_lock = gui_player.db.lock().unwrap();
    if let Some(db) = db_lock.as_ref() {
        db.remember_position(&path, position, duration)
            .map_err(|e| e.to_string())?;
    }
    Ok(json!({"saved": true}))
}

/// What the user was last watching, or null.
///
/// The window offers this on the drop zone. It is deliberately a read: the
/// frontend plays it through the ordinary `play`, which applies the resume
/// point, so a click here and a click in the recent list take the same
/// path and cannot land in different places.
#[command]
pub fn session_get(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let db_lock = gui_player.db.lock().unwrap();
    match db_lock.as_ref() {
        Some(db) => match db.get_session().map_err(|e| e.to_string())? {
            Some(s) => Ok(serde_json::to_value(&s).unwrap_or(json!(null))),
            None => Ok(json!(null)),
        },
        None => Ok(json!(null)),
    }
}

#[command]
pub fn session_clear(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let db_lock = gui_player.db.lock().unwrap();
    if let Some(db) = db_lock.as_ref() {
        db.clear_session().map_err(|e| e.to_string())?;
    }
    Ok(json!({"cleared": true}))
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

// --- online subtitles (OpenSubtitles) --------------------------------------
//
// Network work goes on a blocking thread: these are sync HTTP calls that can
// take seconds, and running them on the command thread would freeze the
// window for the duration. The player is only touched before (to learn what
// is playing) and after (to load the result), never across the await.

#[command]
pub fn opensubtitles_configured() -> Result<Value, String> {
    Ok(json!({
        "configured": crate::core::opensubtitles::is_configured(),
        "languages": crate::core::opensubtitles::default_languages(),
    }))
}

#[command]
pub async fn subtitle_search_online(
    query: Option<String>,
    file: Option<String>,
    languages: Option<String>,
    hash: Option<bool>,
    gui_player: State<'_, GuiPlayer>,
) -> Result<Value, String> {
    let target = file.or_else(|| {
        gui_player
            .mpv()
            .ok()
            .and_then(|p| p.status().file)
    });
    let req = crate::core::opensubtitles::SearchRequest {
        query,
        file: target,
        languages,
        hash: hash.unwrap_or(true),
    };

    let outcome = tokio::task::spawn_blocking(move || crate::core::opensubtitles::run_search(&req))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    serde_json::to_value(&outcome).map_err(|e| e.to_string())
}

#[command]
pub async fn subtitle_download_online(
    file_id: i64,
    file: Option<String>,
    language: Option<String>,
    load: Option<bool>,
    gui_player: State<'_, GuiPlayer>,
) -> Result<Value, String> {
    let video = file.or_else(|| gui_player.mpv().ok().and_then(|p| p.status().file));
    let language = language.unwrap_or_default();
    let fallback = subtitle_cache_dir();

    let video_for_task = video.clone();
    let dl = tokio::task::spawn_blocking(move || {
        crate::core::opensubtitles::run_download(
            file_id,
            video_for_task.as_deref(),
            &language,
            None,
            &fallback,
        )
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    let mut out = serde_json::to_value(&dl).map_err(|e| e.to_string())?;
    out["loaded"] = json!(load_subtitle_after_download(
        &gui_player,
        &dl.path,
        load.unwrap_or(true),
        &mut out
    ));
    Ok(out)
}

#[command]
pub async fn subtitle_auto_online(
    query: Option<String>,
    file: Option<String>,
    languages: Option<String>,
    load: Option<bool>,
    gui_player: State<'_, GuiPlayer>,
) -> Result<Value, String> {
    let target = file.or_else(|| gui_player.mpv().ok().and_then(|p| p.status().file));
    let req = crate::core::opensubtitles::SearchRequest {
        query,
        file: target,
        languages,
        hash: true,
    };
    let fallback = subtitle_cache_dir();

    let (dl, best, outcome) =
        tokio::task::spawn_blocking(move || crate::core::opensubtitles::run_auto(&req, &fallback))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;

    let mut out = serde_json::to_value(&dl).map_err(|e| e.to_string())?;
    out["moviehash_match"] = json!(best.moviehash_match);
    out["language"] = json!(best.language);
    out["release"] = json!(best.release);
    out["candidates"] = json!(outcome.results.len());
    out["query"] = json!(outcome.query);
    out["loaded"] = json!(load_subtitle_after_download(
        &gui_player,
        &dl.path,
        load.unwrap_or(true),
        &mut out
    ));
    Ok(out)
}

/// Where downloads go when the video's own directory can't take them.
fn subtitle_cache_dir() -> std::path::PathBuf {
    dirs_next::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("unflick")
}

/// Load a freshly downloaded subtitle, recording rather than raising a
/// failure: the file is on disk and the download quota is already spent, so
/// reporting the whole operation as failed would misdescribe it.
fn load_subtitle_after_download(
    gui_player: &State<'_, GuiPlayer>,
    path: &str,
    load: bool,
    out: &mut Value,
) -> bool {
    if !load {
        return false;
    }
    let loaded = gui_player
        .mpv()
        .map_err(|e| e.to_string())
        .and_then(|p| p.subtitle_load(path).map_err(|e| e.to_string()));
    match loaded {
        Ok(()) => true,
        Err(e) => {
            out["load_error"] = json!(e);
            false
        }
    }
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

#[command(async)]
pub fn playlist_next(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    match gui_player.playlist.next() {
        Some(path) => {
            // Active player drives the new track. Pre-pipeline-init we
            // silently skip — the playlist still advances.
            if let Ok(player) = gui_player.mpv() {
                player.play(&path, None, None, None).map_err(|e| e.to_string())?;
            }
            Ok(json!({"path": path}))
        }
        None => Ok(json!({"path": null, "message": "no next track"})),
    }
}

#[command(async)]
pub fn playlist_prev(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    match gui_player.playlist.prev() {
        Some(path) => {
            if let Ok(player) = gui_player.mpv() {
                player.play(&path, None, None, None).map_err(|e| e.to_string())?;
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

#[command(async)]
pub fn playlist_play_index(index: usize, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let path = gui_player.playlist.set_current(index).map_err(|e| e)?;
    if let Ok(player) = gui_player.mpv() {
        player.play(&path, None, None, None).map_err(|e| e.to_string())?;
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

// --- Audio processing (v0.12) ----------------------------------------------
//
// Straight to the player: unlike the online-subtitle commands there is no
// network here, so nothing needs a blocking thread. Persistence goes through
// `core::audio::save` so the GUI, CLI and MCP all restore the same curve.

/// Shape the audio state for the frontend, saving it first.
fn audio_payload(player: &crate::core::player::Player, save: bool) -> Value {
    let settings = player.audio_settings();
    if save {
        if let Err(e) = crate::core::audio::save(&settings) {
            eprintln!("[unflick] could not persist audio settings: {}", e);
        }
    }
    json!({
        "enabled": settings.equalizer,
        "bands": settings.bands,
        "frequencies": crate::core::audio::BANDS,
        "preamp": settings.preamp,
        "normalize": settings.normalize,
        "flat": settings.is_flat(),
        "max_gain": crate::core::audio::MAX_GAIN_DB,
        "pitch_correction": player.pitch_correction(),
    })
}

#[command]
pub fn equalizer_get(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    Ok(audio_payload(&player, false))
}

#[command]
pub fn equalizer_set(
    band: Option<i64>,
    gain: Option<f64>,
    bands: Option<Vec<f64>>,
    enabled: Option<bool>,
    normalize: Option<bool>,
    preamp: Option<f64>,
    gui_player: State<'_, GuiPlayer>,
) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    let mut next = player.audio_settings();
    let mut shape_changed = false;

    if let Some(on) = enabled {
        next.equalizer = on;
        shape_changed = true;
    }
    if let Some(on) = normalize {
        next.normalize = on;
        shape_changed = true;
    }
    if let Some(p) = preamp {
        next.preamp = p;
        shape_changed = true;
    }
    if let Some(list) = bands {
        next.bands = list;
        shape_changed = true;
    }

    // Shape first: a band set on a chain that is about to be rebuilt would be
    // overwritten by the rebuild.
    if shape_changed {
        player.set_audio_settings(next).map_err(|e| e.to_string())?;
    }
    if let Some(index) = band {
        let index = crate::core::audio::parse_band(index).map_err(|e| e.to_string())?;
        let gain = gain.ok_or_else(|| "gain required when setting a band".to_string())?;
        player.set_band(index, gain).map_err(|e| e.to_string())?;
    }

    Ok(audio_payload(&player, true))
}

#[command]
pub fn equalizer_preset(name: String, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    player.set_audio_preset(&name).map_err(|e| e.to_string())?;
    Ok(audio_payload(&player, true))
}

#[command]
pub fn equalizer_presets() -> Result<Value, String> {
    Ok(json!(crate::core::audio::PRESETS
        .iter()
        .map(|p| json!({
            "name": p.name,
            "description": p.description,
            "bands": p.bands,
        }))
        .collect::<Vec<_>>()))
}

#[command]
pub fn equalizer_reset(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    player.reset_audio().map_err(|e| e.to_string())?;
    Ok(audio_payload(&player, true))
}

#[command]
pub fn pitch_correction(
    enabled: Option<bool>,
    gui_player: State<'_, GuiPlayer>,
) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    if let Some(on) = enabled {
        player.set_pitch_correction(on).map_err(|e| e.to_string())?;
    }
    Ok(json!({ "enabled": player.pitch_correction() }))
}

// ─── Settings persistence ─────────────────────────────────────────────────────

/// Persist the frontend's settings blob, **merging** it into whatever is
/// already on disk.
///
/// Merging rather than overwriting is load-bearing. settings.json is shared
/// with the CLI and MCP server, which write keys the GUI has never heard of:
/// keybindings, mouse bindings, the OpenSubtitles key. The frontend builds
/// its payload from the fields it models, so a wholesale write silently
/// deleted every one of those the first time a user touched any setting in
/// the window. See `core::settings::merge`.
#[command]
pub fn save_settings(settings: String) -> Result<Value, String> {
    let incoming: Value = serde_json::from_str(&settings)
        .map_err(|e| format!("settings payload is not valid JSON: {}", e))?;
    crate::core::settings::merge(&incoming).map_err(|e| e.to_string())?;
    Ok(json!({"saved": true}))
}

/// Write a single settings key without touching anything else.
///
/// The blob write above is for the settings panel, which owns a known set of
/// fields. This is for one-off keys collected elsewhere in the UI - the
/// OpenSubtitles key entered in the subtitle dialog - where round-tripping
/// the whole settings object would be both pointless and a chance to lose
/// something.
#[command]
pub fn settings_set_key(key: String, value: Value) -> Result<Value, String> {
    if key.trim().is_empty() {
        return Err("settings key must not be empty".into());
    }
    crate::core::settings::set(key.trim(), value).map_err(|e| e.to_string())?;
    Ok(json!({"saved": true}))
}

#[command]
pub fn load_settings() -> Result<Value, String> {
    // Via `core::settings` so the GUI, CLI and MCP all resolve the same file,
    // including the UNFLICK_CONFIG_DIR override the tests rely on.
    crate::core::settings::read_all().map_err(|e| e.to_string())
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

// ─── Timing, chapters, A-B loop, frame stepping ────────────────────────────────
//
// Thin wrappers over `core::player`. The same logic is reachable from the CLI
// and MCP through `core::daemon`; these exist so the React layer doesn't have
// to round-trip through the control socket to talk to its own player.

/// Get or set the subtitle delay. `seconds` omitted reads the current value;
/// `relative` nudges it instead of replacing it.
#[command]
pub fn subtitle_delay(
    seconds: Option<f64>,
    relative: Option<bool>,
    gui_player: State<'_, GuiPlayer>,
) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    let current = player.sub_delay();
    let Some(value) = seconds else {
        return Ok(json!({"seconds": current}));
    };
    let target = if relative.unwrap_or(false) { current + value } else { value };
    player.set_sub_delay(target).map_err(|e| e.to_string())?;
    Ok(json!({"seconds": target}))
}

/// Get or set the audio delay (lip-sync correction).
#[command]
pub fn audio_delay(
    seconds: Option<f64>,
    relative: Option<bool>,
    gui_player: State<'_, GuiPlayer>,
) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    let current = player.audio_delay();
    let Some(value) = seconds else {
        return Ok(json!({"seconds": current}));
    };
    let target = if relative.unwrap_or(false) { current + value } else { value };
    player.set_audio_delay(target).map_err(|e| e.to_string())?;
    Ok(json!({"seconds": target}))
}

#[command]
pub fn subtitle_style_get(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    Ok(player.subtitle_style())
}

#[command]
pub fn subtitle_style_set(
    name: String,
    value: Value,
    gui_player: State<'_, GuiPlayer>,
) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    player.set_subtitle_style(&name, &value).map_err(|e| e.to_string())?;
    Ok(player.subtitle_style())
}

#[command]
pub fn chapter_list(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    Ok(json!(player.chapter_list()))
}

#[command]
pub fn chapter_seek(index: i64, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    player.chapter_seek(index).map_err(|e| e.to_string())?;
    Ok(json!({"index": index}))
}

/// Step one chapter in either direction. `delta` is +1 / -1.
#[command]
pub fn chapter_step(delta: i64, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    let index = player.chapter_step(delta).map_err(|e| e.to_string())?;
    Ok(json!({"index": index}))
}

/// A-B loop control. `action` is one of a | b | clear | status; `position`
/// defaults to wherever playback currently is.
#[command]
pub fn ab_loop(
    action: String,
    position: Option<f64>,
    gui_player: State<'_, GuiPlayer>,
) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    let state = match action.as_str() {
        "status" => player.ab_loop_status(),
        "a" => player.ab_loop_set_a(position).map_err(|e| e.to_string())?,
        "b" => player.ab_loop_set_b(position).map_err(|e| e.to_string())?,
        "clear" => {
            player.ab_loop_clear().map_err(|e| e.to_string())?;
            player.ab_loop_status()
        }
        other => return Err(format!("unknown ab_loop action: {}", other)),
    };
    Ok(json!(state))
}

/// Step one frame. `delta` is +1 forward or -1 back. Pauses playback.
#[command]
pub fn frame_step(delta: i64, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    if delta >= 0 {
        player.frame_step().map_err(|e| e.to_string())?;
    } else {
        player.frame_back_step().map_err(|e| e.to_string())?;
    }
    Ok(json!({"position": player.status().position}))
}

#[command]
pub fn playlist_repeat(
    mode: Option<String>,
    gui_player: State<'_, GuiPlayer>,
) -> Result<Value, String> {
    if let Some(m) = mode {
        let parsed = crate::core::types::RepeatMode::parse(&m)
            .ok_or_else(|| format!("unknown repeat mode: {} (expected off | one | all)", m))?;
        gui_player.playlist.set_repeat_mode(parsed);
    }
    Ok(json!({"mode": gui_player.playlist.repeat_mode().as_str()}))
}

#[command]
pub fn playlist_shuffle(
    enabled: Option<bool>,
    gui_player: State<'_, GuiPlayer>,
) -> Result<Value, String> {
    if let Some(e) = enabled {
        gui_player.playlist.set_shuffle(e);
    }
    Ok(json!({"enabled": gui_player.playlist.shuffle_enabled()}))
}

// ─── Keyboard bindings ─────────────────────────────────────────────────────
//
// Thin wrappers over `core::keybind`, which owns the action catalogue and
// the settings-file storage. The frontend builds its key → action map from
// `keybind_list` rather than keeping its own copy of the defaults.

#[command]
pub fn keybind_list() -> Result<Value, String> {
    crate::core::keybind::list().map_err(|e| e.to_string())
}

#[command]
pub fn keybind_set(action: String, key: String) -> Result<Value, String> {
    let normalized = crate::core::keybind::set(&action, &key).map_err(|e| e.to_string())?;
    Ok(json!({ "action": action, "key": normalized }))
}

#[command]
pub fn keybind_reset(action: Option<String>) -> Result<Value, String> {
    let count = crate::core::keybind::reset(action.as_deref()).map_err(|e| e.to_string())?;
    Ok(json!({ "reset": count }))
}

// ─── Picture geometry ──────────────────────────────────────────────────────

#[command]
pub fn video_transform_get(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    Ok(player.video_transform())
}

#[command]
pub fn video_transform_set(
    name: String,
    value: Value,
    gui_player: State<'_, GuiPlayer>,
) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    player.set_video_transform(&name, &value).map_err(|e| e.to_string())?;
    Ok(player.video_transform())
}

#[command]
pub fn video_transform_reset(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    player.reset_video_transform().map_err(|e| e.to_string())?;
    Ok(player.video_transform())
}

/// Mirror the window's incognito switch into the shared flag, so a CLI or
/// MCP `play` against this same player also leaves no history.
#[command]
pub fn set_incognito(enabled: bool, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    gui_player
        .incognito
        .store(enabled, std::sync::atomic::Ordering::Relaxed);
    Ok(json!({ "enabled": enabled }))
}

/// Recently played files, newest first. Used by the drop zone, which is
/// exactly where someone is looking when they want to reopen something.
#[command]
pub fn recent_list(limit: Option<usize>, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let db_lock = gui_player.db.lock().unwrap();
    let db = db_lock.as_ref().ok_or_else(|| "database unavailable".to_string())?;
    let entries = db
        .recent(limit.unwrap_or(12).clamp(1, 200))
        .map_err(|e| e.to_string())?;
    serde_json::to_value(entries).map_err(|e| e.to_string())
}

#[command]
pub fn recent_clear(gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let db_lock = gui_player.db.lock().unwrap();
    let db = db_lock.as_ref().ok_or_else(|| "database unavailable".to_string())?;
    let n = db.clear_recent().map_err(|e| e.to_string())?;
    Ok(json!({ "cleared": n }))
}

// ─── Bookmarks ────────────────────────────────────────────────────────────
//
// The GUI keeps the *jump* on its own side: seeking within the open file is
// one call, but opening a different one has to go through the frontend's
// play pipeline (yt-dlp extraction, sidecar subtitles, history). So these
// commands cover storage only, and `playerStore.gotoBookmark` decides which
// of the two it is. The CLI and MCP get a server-side `bookmark_goto`
// instead, since they have no frontend to route through.

#[command]
pub fn bookmark_add(
    name: Option<String>,
    position: Option<f64>,
    file: Option<String>,
    gui_player: State<'_, GuiPlayer>,
) -> Result<Value, String> {
    let status = gui_player.mpv().map_err(|e| e.to_string())?.status();
    let path = file
        .or(status.file.clone())
        .ok_or_else(|| "nothing is playing".to_string())?;
    let position = match position {
        Some(p) => p,
        None if Some(&path) == status.file.as_ref() => status.position,
        None => return Err("position is required for a file that isn't playing".to_string()),
    };
    let name = name.as_deref().map(str::trim).filter(|s| !s.is_empty());

    let db_lock = gui_player.db.lock().unwrap();
    let db = db_lock.as_ref().ok_or_else(|| "database unavailable".to_string())?;
    let bookmark = db.add_bookmark(&path, position, name).map_err(|e| e.to_string())?;
    serde_json::to_value(bookmark).map_err(|e| e.to_string())
}

#[command]
pub fn bookmark_list(
    file: Option<String>,
    all: Option<bool>,
    gui_player: State<'_, GuiPlayer>,
) -> Result<Value, String> {
    let scope = if all.unwrap_or(false) {
        None
    } else {
        Some(
            file.or(gui_player
                .mpv()
                .map_err(|e| e.to_string())?
                .status()
                .file)
                .ok_or_else(|| "nothing is playing".to_string())?,
        )
    };
    let db_lock = gui_player.db.lock().unwrap();
    let db = db_lock.as_ref().ok_or_else(|| "database unavailable".to_string())?;
    let list = db.list_bookmarks(scope.as_deref()).map_err(|e| e.to_string())?;
    serde_json::to_value(list).map_err(|e| e.to_string())
}

#[command]
pub fn bookmark_rename(
    id: i64,
    name: Option<String>,
    gui_player: State<'_, GuiPlayer>,
) -> Result<Value, String> {
    let name = name.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let db_lock = gui_player.db.lock().unwrap();
    let db = db_lock.as_ref().ok_or_else(|| "database unavailable".to_string())?;
    let bookmark = db.rename_bookmark(id, name).map_err(|e| e.to_string())?;
    serde_json::to_value(bookmark).map_err(|e| e.to_string())
}

#[command]
pub fn bookmark_remove(id: i64, gui_player: State<'_, GuiPlayer>) -> Result<Value, String> {
    let db_lock = gui_player.db.lock().unwrap();
    let db = db_lock.as_ref().ok_or_else(|| "database unavailable".to_string())?;
    if !db.remove_bookmark(id).map_err(|e| e.to_string())? {
        return Err(format!("no bookmark with id {}", id));
    }
    Ok(json!({ "removed": id }))
}

#[command]
pub fn bookmark_clear(
    file: Option<String>,
    all: Option<bool>,
    gui_player: State<'_, GuiPlayer>,
) -> Result<Value, String> {
    let scope = if all.unwrap_or(false) {
        None
    } else {
        Some(
            file.or(gui_player
                .mpv()
                .map_err(|e| e.to_string())?
                .status()
                .file)
                .ok_or_else(|| "nothing is playing".to_string())?,
        )
    };
    let db_lock = gui_player.db.lock().unwrap();
    let db = db_lock.as_ref().ok_or_else(|| "database unavailable".to_string())?;
    let n = db.clear_bookmarks(scope.as_deref()).map_err(|e| e.to_string())?;
    Ok(json!({ "cleared": n }))
}

#[command]
pub fn mouse_list() -> Result<Value, String> {
    crate::core::mousebind::list().map_err(|e| e.to_string())
}

#[command]
pub fn mouse_set(trigger: String, action: String) -> Result<Value, String> {
    let applied = crate::core::mousebind::set(&trigger, &action).map_err(|e| e.to_string())?;
    Ok(json!({ "trigger": trigger, "action": applied }))
}

#[command]
pub fn mouse_reset(trigger: Option<String>) -> Result<Value, String> {
    let count = crate::core::mousebind::reset(trigger.as_deref()).map_err(|e| e.to_string())?;
    Ok(json!({ "reset": count }))
}

/// Preview frame for a position on the timeline, as a `data:` URL the
/// progress bar can drop straight into an `<img>`.
///
/// Called on hover, so it must stay cheap: `core::thumbnail` buckets the
/// timeline and caches to disk, and the frontend debounces on top of that.
/// Errors are normal here — streams have no previews, and a file may not
/// have a decodable frame at the requested point — so callers should treat
/// a failure as "no preview", not as something to report.
#[command]
pub fn thumbnail_at(
    position: f64,
    width: Option<u32>,
    gui_player: State<'_, GuiPlayer>,
) -> Result<Value, String> {
    let player = gui_player.mpv().map_err(|e| e.to_string())?;
    let status = player.status();
    let file = status.file.ok_or_else(|| "nothing is playing".to_string())?;

    let thumb = crate::core::thumbnail::thumbnail_at(
        &file,
        position,
        status.duration,
        width.unwrap_or(160),
    )
    .map_err(|e| e.to_string())?;

    Ok(json!({
        "position": thumb.bucket_seconds,
        "dataUrl": format!(
            "data:image/jpeg;base64,{}",
            crate::core::vision::base64_encode(&thumb.bytes)
        ),
    }))
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
