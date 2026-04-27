use std::path::Path;

use anyhow::Result;

use crate::core::player::Player;
use crate::db::{Database, MediaEntry};

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "mpg", "mpeg", "ts", "vob", "3gp",
    "ogv", "rmvb",
];

pub fn scan_directory(db: &Database, dir: &str) -> Result<Vec<MediaEntry>> {
    let path = Path::new(dir);
    if !path.is_dir() {
        anyhow::bail!("not a directory: {}", dir);
    }

    let mut added = Vec::new();

    for entry in walkdir(path)? {
        let ext = entry
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        if !VIDEO_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }

        let file_path = entry.to_string_lossy().to_string();
        let title = entry
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let file_size = std::fs::metadata(&entry).ok().map(|m| m.len() as i64);

        // Probe metadata with a temporary mpv instance
        let (duration, width, height, video_codec, audio_codec) = probe_file(&file_path);

        let media = MediaEntry {
            id: 0,
            path: file_path,
            title,
            duration,
            width,
            height,
            video_codec,
            audio_codec,
            file_size,
            added_at: String::new(),
            last_played: None,
            play_count: 0,
        };

        if db.upsert_media(&media).is_ok() {
            added.push(media);
        }
    }

    Ok(added)
}

fn walkdir(dir: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    walk_recursive(dir, &mut files)?;
    Ok(files)
}

fn walk_recursive(dir: &Path, files: &mut Vec<std::path::PathBuf>) -> Result<()> {
    let entries = std::fs::read_dir(dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            // Silently skip directories we can't read
            let _ = walk_recursive(&path, files);
        } else {
            files.push(path);
        }
    }
    Ok(())
}

fn probe_file(
    path: &str,
) -> (
    Option<f64>,
    Option<i64>,
    Option<i64>,
    Option<String>,
    Option<String>,
) {
    let probe = match Player::new() {
        Ok(p) => p,
        Err(_) => return (None, None, None, None, None),
    };

    if probe.play(path, None, None, None).is_err() {
        return (None, None, None, None, None);
    }

    // Wait for file to load
    std::thread::sleep(std::time::Duration::from_millis(500));

    let duration = probe.status().duration;
    let duration = if duration > 0.0 { Some(duration) } else { None };
    let width = probe.get_property_i64("width").ok();
    let height = probe.get_property_i64("height").ok();
    let video_codec = probe.get_property_string("video-codec").ok();
    let audio_codec = probe.get_property_string("audio-codec").ok();

    (duration, width, height, video_codec, audio_codec)
}
