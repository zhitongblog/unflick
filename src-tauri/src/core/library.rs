use std::path::Path;

use anyhow::Result;

use crate::db::{Database, MediaEntry};

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "mpg", "mpeg", "ts", "vob", "3gp",
    "ogv", "rmvb",
];

/// Maximum directory depth to recurse into during a scan. Prevents runaway
/// traversal of root drives or symlink loops.
const MAX_DEPTH: u32 = 8;

/// Folder name fragments to skip during scanning (case-insensitive). These are
/// either system folders, build outputs, or other places no user keeps videos.
const SKIP_DIR_NAMES: &[&str] = &[
    "$recycle.bin",
    "system volume information",
    "windows",
    "program files",
    "program files (x86)",
    "programdata",
    "appdata",
    ".git",
    ".svn",
    ".hg",
    "node_modules",
    "target",
    "build",
    "dist",
    ".cache",
    ".tmp",
];

pub fn scan_directory(db: &Database, dir: &str) -> Result<Vec<MediaEntry>> {
    let path = Path::new(dir);
    if !path.is_dir() {
        anyhow::bail!("not a directory: {}", dir);
    }

    let mut added = Vec::new();
    let mut files = Vec::new();
    walk_recursive(path, &mut files, 0)?;

    for entry in files {
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

        // Skip mpv-based probing — it adds 500ms+ per file and blocks scans of
        // large libraries. Metadata can be filled in lazily later (e.g. on
        // first playback) without blocking the user's directory pick.
        let media = MediaEntry {
            id: 0,
            path: file_path,
            title,
            duration: None,
            width: None,
            height: None,
            video_codec: None,
            audio_codec: None,
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

fn should_skip_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    let lower = name.to_lowercase();
    if lower.starts_with('.') {
        return true; // skip dotfolders
    }
    SKIP_DIR_NAMES.iter().any(|s| lower == *s)
}

fn walk_recursive(dir: &Path, files: &mut Vec<std::path::PathBuf>, depth: u32) -> Result<()> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(it) => it,
        Err(_) => return Ok(()), // permission denied / not readable — skip
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // Skip symlinks to avoid loops
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            if should_skip_dir(&path) {
                continue;
            }
            let _ = walk_recursive(&path, files, depth + 1);
        } else {
            files.push(path);
        }
    }
    Ok(())
}
