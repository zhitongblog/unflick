pub mod core;
pub mod db;
pub mod mpv;
pub mod cli;
pub mod mcp;
pub mod gui;

use gui::{commands, state::GuiPlayer};
use tauri::menu::{Menu, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(GuiPlayer::new())
        .setup(|app| {
            let handle = app.handle();

            // Build File menu
            let open_item = MenuItemBuilder::with_id("open", "Open File...")
                .accelerator("CmdOrCtrl+O")
                .build(handle)?;
            let sep1 = PredefinedMenuItem::separator(handle)?;
            let quit_item = MenuItemBuilder::with_id("quit", "Quit")
                .accelerator("CmdOrCtrl+Q")
                .build(handle)?;
            let file_menu = SubmenuBuilder::new(handle, "File")
                .items(&[&open_item, &sep1, &quit_item])
                .build()?;

            // Build Playback menu
            let play_pause_item = MenuItemBuilder::with_id("play_pause", "Play/Pause")
                .build(handle)?;
            let stop_item = MenuItemBuilder::with_id("stop", "Stop")
                .build(handle)?;
            let sep2 = PredefinedMenuItem::separator(handle)?;
            let vol_up_item = MenuItemBuilder::with_id("volume_up", "Volume Up")
                .build(handle)?;
            let vol_down_item = MenuItemBuilder::with_id("volume_down", "Volume Down")
                .build(handle)?;
            let playback_menu = SubmenuBuilder::new(handle, "Playback")
                .items(&[&play_pause_item, &stop_item, &sep2, &vol_up_item, &vol_down_item])
                .build()?;

            // Build View menu
            let fullscreen_item = MenuItemBuilder::with_id("fullscreen", "Toggle Fullscreen")
                .accelerator("F11")
                .build(handle)?;
            let pip_item = MenuItemBuilder::with_id("pip", "Picture in Picture")
                .build(handle)?;
            let library_item = MenuItemBuilder::with_id("library", "Toggle Library")
                .build(handle)?;
            let view_menu = SubmenuBuilder::new(handle, "View")
                .items(&[&fullscreen_item, &pip_item, &library_item])
                .build()?;

            // Build Help menu
            let about_item = MenuItemBuilder::with_id("about", "About unflick")
                .build(handle)?;
            let help_menu = SubmenuBuilder::new(handle, "Help")
                .items(&[&about_item])
                .build()?;

            let menu = Menu::with_items(handle, &[&file_menu, &playback_menu, &view_menu, &help_menu])?;
            app.set_menu(menu)?;

            Ok(())
        })
        .on_menu_event(|app, event| {
            let window = app.get_webview_window("main");
            match event.id().as_ref() {
                "quit" => {
                    app.exit(0);
                }
                "open" | "play_pause" | "stop" | "fullscreen" | "pip" | "library"
                | "volume_up" | "volume_down" | "about" => {
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
            // Settings persistence
            commands::save_settings,
            commands::load_settings,
            commands::check_bundled_whisper,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
