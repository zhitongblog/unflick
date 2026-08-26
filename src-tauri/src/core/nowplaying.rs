//! What is playing, as a person would describe it: a title, who made it, and
//! the picture on the front.
//!
//! `status` answers a machine's question — path, position, state. Music mode
//! needs the other kind: the tags, and whether there is any picture at all.
//! Both CLI and MCP get it too, because "what am I listening to" is a
//! reasonable thing to ask from a script or an agent, and reading it out of
//! a file path is guesswork.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

use super::player::{find_ffmpeg, Player};

/// Longest edge of an extracted cover, in pixels. Cover art in a music file
/// is routinely 1400px square; the window shows it at a few hundred.
const COVER_MAX_EDGE: u32 = 512;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct NowPlaying {
    /// Path or URL of the open file, if any.
    pub file: Option<String>,
    /// mpv's display title: the `title` tag when there is one, the file name
    /// otherwise. What to show when there is room for exactly one line.
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// Real moving pictures — an embedded cover counts as a video track to
    /// mpv, and calling that "has video" would put a still image in a
    /// player window.
    pub has_video: bool,
    /// Extracted cover art on disk, when the file carries any.
    pub cover: Option<String>,
    pub duration: f64,
}

/// Read the tags of whatever is loaded.
///
/// `with_cover` controls the expensive half: pulling the artwork out costs an
/// ffmpeg run the first time per file, which the GUI wants and a `status`-like
/// poll does not.
pub fn now_playing(player: &Player, with_cover: bool) -> NowPlaying {
    let status = player.status();
    let file = status.file.clone();

    // mpv normalises tag-name casing itself, so `metadata/by-key/Artist`
    // works whether the container spelled it ARTIST, artist or ©ART.
    let tag = |key: &str| {
        player
            .get_property_string(&format!("metadata/by-key/{}", key))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };

    let title = player
        .get_property_string("media-title")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| tag("Title"));

    let has_video = has_moving_pictures(player);

    let cover = if with_cover {
        file.as_deref().and_then(|f| cover_art(f).ok())
    } else {
        None
    };

    NowPlaying {
        file,
        title,
        artist: tag("Artist").or_else(|| tag("Album_Artist")),
        album: tag("Album"),
        has_video,
        cover,
        duration: status.duration,
    }
}

/// Whether the open file has moving pictures, as opposed to none or a single
/// embedded cover.
///
/// mpv presents album art as a video track — same properties, same width and
/// height — so the naive check reports every tagged mp3 as a video. The
/// distinguishing flag is `albumart` on the selected track.
fn has_moving_pictures(player: &Player) -> bool {
    let has_track = player
        .get_property_i64("width")
        .map(|w| w > 0)
        .unwrap_or(false);
    if !has_track {
        return false;
    }
    !player
        .get_property_bool("current-tracks/video/albumart")
        .unwrap_or(false)
}

/// Extract embedded cover art to a cached file and return its path.
///
/// Cached by path, size and mtime like the timeline previews: re-running
/// ffmpeg for every poll of the same track would be absurd, and a file
/// edited in a tag editor has to lose its stale cover.
pub fn cover_art(file: &str) -> Result<String> {
    if file.is_empty() {
        bail!("no file is playing");
    }
    // A stream would mean re-fetching the whole thing to reach the art, and
    // for a yt-dlp-resolved URL the token may have expired already.
    if file.starts_with("http://") || file.starts_with("https://") {
        bail!("cover art is only read from local files");
    }
    let path = Path::new(file);
    if !path.exists() {
        bail!("file not found: {}", file);
    }

    let out = cache_path(path)?;
    if out.exists() {
        return Ok(out.to_string_lossy().into_owned());
    }
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow!("failed to create cover cache dir: {}", e))?;
    }

    let ffmpeg = find_ffmpeg().ok_or_else(|| anyhow!("ffmpeg not found"))?;
    // Re-encoded rather than stream-copied: cover art is PNG about as often
    // as it is JPEG, and one output format means one code path here and one
    // in the UI. `-frames:v 1` keeps a multi-picture file (some releases
    // embed front and back) to the one that gets shown.
    let result = std::process::Command::new(&ffmpeg)
        .args([
            "-v", "error",
            "-y",
            "-i", file,
            "-an",
            "-map", "0:v:0",
            "-frames:v", "1",
            "-vf", &format!("scale='min({m},iw)':-2", m = COVER_MAX_EDGE),
        ])
        .arg(&out)
        .output()
        .map_err(|e| anyhow!("failed to run ffmpeg: {}", e))?;

    if !result.status.success() || !out.exists() {
        // Overwhelmingly this is "the file has no cover", which is not an
        // error worth a stack trace — the caller renders a placeholder.
        let _ = std::fs::remove_file(&out);
        bail!("no cover art in {}", file);
    }
    Ok(out.to_string_lossy().into_owned())
}

fn cache_path(file: &Path) -> Result<PathBuf> {
    let meta = std::fs::metadata(file)
        .map_err(|e| anyhow!("failed to stat {}: {}", file.display(), e))?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut hash: u64 = 0xcbf29ce484222325;
    let mut mix = |bytes: &[u8]| {
        for b in bytes {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
    };
    mix(file.to_string_lossy().as_bytes());
    mix(&meta.len().to_le_bytes());
    mix(&mtime.to_le_bytes());

    Ok(dirs_next::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("unflick")
        .join("covers")
        .join(format!("{:016x}.jpg", hash)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_art_refuses_sources_it_cannot_reach() {
        assert!(cover_art("").unwrap_err().to_string().contains("no file"));
        assert!(cover_art("https://example.com/a.mp3")
            .unwrap_err()
            .to_string()
            .contains("local files"));
    }

    #[test]
    fn cache_paths_follow_the_file_not_just_its_name() {
        // Two different files never share a cover, and the same file keeps
        // its cover across calls — that is the whole point of the cache.
        let dir = std::env::temp_dir().join("unflick-cover-cache-test");
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.mp3");
        let b = dir.join("b.mp3");
        std::fs::write(&a, b"aaaa").unwrap();
        std::fs::write(&b, b"bbbbbb").unwrap();

        assert_eq!(cache_path(&a).unwrap(), cache_path(&a).unwrap());
        assert_ne!(cache_path(&a).unwrap(), cache_path(&b).unwrap());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
