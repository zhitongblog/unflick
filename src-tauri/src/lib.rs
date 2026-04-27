pub mod core;
pub mod db;
pub mod mpv;
pub mod cli;
pub mod mcp;
pub mod gui;

use gui::{commands, state::GuiPlayer};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(GuiPlayer::new())
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
