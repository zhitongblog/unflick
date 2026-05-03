pub mod player;
pub mod playlist;
pub mod render_loop;
pub mod types;
pub mod daemon;
pub mod i18n;
pub mod library;
pub mod settings;
pub mod whisper;

#[cfg(target_os = "windows")]
pub mod win_assoc;

#[cfg(target_os = "linux")]
pub mod linux_assoc;
