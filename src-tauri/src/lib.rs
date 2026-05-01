pub mod core;
pub mod db;
pub mod mpv;
pub mod cli;
pub mod mcp;
pub mod gui;
pub mod video;

use std::sync::Arc;

use core::i18n::{menu_strings, read_locale_from_settings};
use core::player::Player;
use core::render_loop::RenderLoop;
use gui::{commands, state::{GuiPlayer, PendingFile}};
use video::VideoSurface;
use tauri::menu::{Menu, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // single-instance MUST come first so a second-launch is short-
        // circuited before any other plugin spends time on initialization.
        // The callback runs inside the *first* (already-running) process —
        // we focus its window and forward the second launch's file argument
        // (if any) to the frontend so it plays in the existing window.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            use tauri::{Emitter, Manager};

            // Bring the existing window forward.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();

                // Forward a "open this file" arg, mirroring main.rs's logic.
                let file = args
                    .iter()
                    .skip(1)
                    .find(|a| !a.starts_with('-') && std::path::Path::new(a).is_file())
                    .cloned();
                if let Some(path) = file {
                    let _ = window.emit("open-file", path);
                }
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(GuiPlayer::new())
        .manage(PendingFile::from_env())
        .setup(|app| {
            let handle = app.handle();

            // Pull the user's saved locale once, here at startup. Tauri's
            // native menu can't re-render its labels at runtime, so
            // switching languages requires a restart (the Settings panel
            // tells the user this explicitly).
            let m = menu_strings(&read_locale_from_settings());

            // Build File menu
            let open_item = MenuItemBuilder::with_id("open", m.open_file)
                .accelerator("CmdOrCtrl+O")
                .build(handle)?;
            let open_url_item = MenuItemBuilder::with_id("open_url", m.open_url)
                .accelerator("CmdOrCtrl+U")
                .build(handle)?;
            let sep1 = PredefinedMenuItem::separator(handle)?;
            let quit_item = MenuItemBuilder::with_id("quit", m.quit)
                .accelerator("CmdOrCtrl+Q")
                .build(handle)?;
            let file_menu = SubmenuBuilder::new(handle, m.file)
                .items(&[&open_item, &open_url_item, &sep1, &quit_item])
                .build()?;

            // Build Playback menu
            let play_pause_item = MenuItemBuilder::with_id("play_pause", m.play_pause)
                .build(handle)?;
            let stop_item = MenuItemBuilder::with_id("stop", m.stop)
                .build(handle)?;
            let sep2 = PredefinedMenuItem::separator(handle)?;
            let vol_up_item = MenuItemBuilder::with_id("volume_up", m.volume_up)
                .build(handle)?;
            let vol_down_item = MenuItemBuilder::with_id("volume_down", m.volume_down)
                .build(handle)?;
            let playback_menu = SubmenuBuilder::new(handle, m.playback)
                .items(&[&play_pause_item, &stop_item, &sep2, &vol_up_item, &vol_down_item])
                .build()?;

            // Build View menu
            let fullscreen_item = MenuItemBuilder::with_id("fullscreen", m.fullscreen)
                .accelerator("F11")
                .build(handle)?;
            let pip_item = MenuItemBuilder::with_id("pip", m.pip)
                .build(handle)?;
            let library_item = MenuItemBuilder::with_id("library", m.library)
                .build(handle)?;
            let view_menu = SubmenuBuilder::new(handle, m.view)
                .items(&[&fullscreen_item, &pip_item, &library_item])
                .build()?;

            // Build Help menu
            let about_item = MenuItemBuilder::with_id("about", m.about)
                .build(handle)?;
            let check_updates_item = MenuItemBuilder::with_id("check_updates", m.check_updates)
                .build(handle)?;
            let sep_help = PredefinedMenuItem::separator(handle)?;
            let help_menu = SubmenuBuilder::new(handle, m.help)
                .items(&[&check_updates_item, &sep_help, &about_item])
                .build()?;

            let menu = Menu::with_items(handle, &[&file_menu, &playback_menu, &view_menu, &help_menu])?;
            app.set_menu(menu)?;

            // ── v0.8 video pipeline bring-up ───────────────────────────────
            // Build the embedded video surface beneath the WebView, spin up
            // a render thread that drives mpv → GL, and stash both in the
            // GuiPlayer state. The surface starts hidden so v0.7's HTML5
            // playback path keeps working unchanged in this commit; P5
            // flips it visible and routes commands through render_player.
            #[cfg(target_os = "windows")]
            if let Some(window) = app.get_webview_window("main") {
                if let Err(e) = bring_up_video_pipeline(&window, app.state::<GuiPlayer>()) {
                    eprintln!("[unflick] video pipeline init failed: {e}");
                    // Falls through — HTML5 path still works.
                }
            }

            Ok(())
        })
        .on_menu_event(|app, event| {
            let window = app.get_webview_window("main");
            match event.id().as_ref() {
                "quit" => {
                    app.exit(0);
                }
                "open" | "open_url" | "play_pause" | "stop" | "fullscreen" | "pip" | "library"
                | "volume_up" | "volume_down" | "about" | "check_updates" => {
                    if let Some(win) = &window {
                        let id: String = event.id().as_ref().to_string();
                        let _ = win.emit("menu-event", id);
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::player_init,
            commands::consume_pending_file,
            commands::open_default_apps_settings,
            commands::video_surface_set_geometry,
            commands::player_play,
            commands::player_pause,
            commands::player_resume,
            commands::player_stop,
            commands::player_seek,
            commands::player_set_volume,
            commands::player_set_speed,
            commands::player_status,
            commands::player_screenshot,
            commands::toggle_pip,
            commands::set_fullscreen,
            commands::exit_fullscreen,
            commands::open_file_dialog,
            commands::open_folder_dialog,
            // Library
            commands::library_list,
            commands::library_search,
            commands::library_scan,
            commands::library_clear,
            // Subtitles
            commands::subtitle_load,
            commands::subtitle_list,
            commands::subtitle_select,
            // Playlist
            commands::playlist_add,
            commands::playlist_list,
            commands::playlist_next,
            commands::playlist_prev,
            commands::playlist_remove,
            commands::playlist_play_index,
            commands::playlist_clear,
            commands::open_subtitle_dialog,
            // Clip extraction
            commands::extract_clip,
            commands::save_file_dialog,
            commands::write_file_bytes,
            commands::read_text_file,
            commands::find_sidecar_subtitles,
            commands::check_yt_dlp,
            commands::extract_stream_url,
            commands::yt_dlp_info,
            commands::update_yt_dlp,
            commands::get_system_proxy,
            // Audio tracks
            commands::audio_list,
            commands::audio_select,
            // Playback position / history
            commands::save_position,
            commands::get_position,
            commands::clear_position,
            commands::record_play,
            // AI subtitle generation
            commands::generate_subtitles,
            commands::translate_subtitles,
            // Settings persistence
            commands::save_settings,
            commands::load_settings,
            commands::check_bundled_whisper,
            commands::check_for_updates,
            // Video filters
            commands::set_video_filter,
            commands::get_video_filters,
            commands::reset_video_filters,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                if let Some(gui_player) = window.try_state::<GuiPlayer>() {
                    let mut lock = gui_player.player.lock().unwrap();
                    if let Some(player) = lock.take() {
                        drop(player);
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Bring up the v0.8 video pipeline on the main window.
///
/// Order matters: surface must be created before the render thread starts
/// (the thread immediately calls `make_current` on it), and the player needs
/// to exist before render-context construction (the context binds to mpv).
/// We tolerate any error here — the GUI will simply fall back to the
/// HTML5 path that's still wired up. The surface starts hidden.
#[cfg(target_os = "windows")]
fn bring_up_video_pipeline(
    window: &tauri::WebviewWindow,
    state: tauri::State<'_, GuiPlayer>,
) -> Result<(), String> {
    use windows_sys::Win32::Foundation::HWND;

    // Tauri's `hwnd()` returns the `windows` crate's HWND newtype; our
    // video surface wants windows-sys's HWND alias (`*mut c_void`). Both
    // wrap the same kernel handle — extract the raw pointer through the
    // newtype's `.0` field.
    let tauri_hwnd = window
        .hwnd()
        .map_err(|e| format!("get_hwnd: {e}"))?;
    let hwnd: HWND = tauri_hwnd.0 as HWND;
    let size = window
        .inner_size()
        .map_err(|e| format!("inner_size: {e}"))?;

    // Hidden child window beneath WebView. Geometry is bogus until the
    // frontend calls video_surface_set_geometry once it's mounted.
    let surface = video::windows::WindowsVideoSurface::new(
        hwnd,
        size.width as i32,
        size.height as i32,
    )
    .map_err(|e| format!("create video surface: {e}"))?;
    let surface: Arc<dyn VideoSurface> = Arc::new(surface);

    let player = Arc::new(Player::new_for_render().map_err(|e| format!("create render player: {e}"))?);

    let render_loop = RenderLoop::start(Arc::clone(&player), Arc::clone(&surface))
        .map_err(|e| format!("start render loop: {e}"))?;

    state
        .render_player
        .set(player)
        .map_err(|_| "render_player already initialised".to_string())?;
    state
        .render_loop
        .set(render_loop)
        .map_err(|_| "render_loop already initialised".to_string())?;

    Ok(())
}
