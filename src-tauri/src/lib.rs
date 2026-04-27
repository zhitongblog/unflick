pub mod core;
pub mod mpv;
pub mod cli;
pub mod mcp;
pub mod gui;

use gui::commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|_app| {
            // Auto-start daemon when GUI launches
            std::thread::spawn(|| {
                commands::ensure_daemon_startup();
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::player_play,
            commands::player_pause,
            commands::player_resume,
            commands::player_stop,
            commands::player_seek,
            commands::player_set_volume,
            commands::player_set_speed,
            commands::player_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
