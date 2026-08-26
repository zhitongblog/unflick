//! Progress-bar thumbnail previews.
//!
//! Hovering a timeline should show the frame you'd land on. Every player
//! people compare us to does this — IINA, mpv via thumbfast, PotPlayer —
//! and its absence is felt on every single scrub.
//!
//! ## Why on-demand rather than a sprite sheet
//!
//! The obvious design is to decode the whole file once on load and build a
//! sprite sheet. Web players do it that way because they can prepare it
//! server-side ahead of time. Locally it means a full decode pass — minutes
//! for a long film — during which the feature simply doesn't work, and it
//! spends that time on a film the user may scrub twice.
//!
//! So: extract one frame per hover, but never more than one per *bucket*.
//! The timeline is divided into ~200 buckets; scrubbing across a two-hour
//! film costs at most 200 extractions instead of one per pixel, and the
//! second pass over the same region is served from disk.
//!
//! Extraction uses ffmpeg's *input* seek (`-ss` before `-i`), which jumps
//! to the nearest keyframe without decoding everything before it. That is
//! the difference between ~50 ms and several seconds per thumbnail. Landing
//! on a keyframe rather than the exact frame is invisible in a 160px
//! preview.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};

use super::player::find_ffmpeg;

/// Roughly how many previews a timeline is divided into. Chosen so a long
/// film's cache stays a few hundred KB while adjacent buckets are still
/// visually distinct while scrubbing.
const BUCKETS_PER_FILE: f64 = 200.0;

/// Bucket size floor and ceiling, in seconds. The floor stops a short clip
/// from extracting a frame every few hundred milliseconds; the ceiling
/// stops a very long recording from jumping a minute per step.
const MIN_BUCKET_SECS: f64 = 2.0;
const MAX_BUCKET_SECS: f64 = 30.0;

/// Total disk the thumbnail cache may occupy before the oldest videos are
/// evicted. Previews are ~4 KB each, so this holds a lot of films — but a
/// player people keep for years needs *some* ceiling.
const CACHE_BUDGET_BYTES: u64 = 200 * 1024 * 1024;

pub struct Thumbnail {
    pub bytes: Vec<u8>,
    /// Timestamp the preview actually represents — the bucket's start, not
    /// the hovered position. The UI labels the tooltip with the hovered
    /// time, so this is only for callers that want to dedupe.
    pub bucket_seconds: f64,
}

/// Preview frame for `seconds` into `video`.
///
/// `duration` sizes the buckets; pass 0 if unknown and a default step is
/// used. `width` is the preview's width in pixels — height follows the
/// source aspect ratio.
pub fn thumbnail_at(video: &str, seconds: f64, duration: f64, width: u32) -> Result<Thumbnail> {
    if video.is_empty() {
        bail!("no file is playing");
    }
    // Network sources would mean a fresh HTTP range request per preview,
    // and for a yt-dlp-resolved URL the token may already have expired.
    if video.starts_with("http://") || video.starts_with("https://") {
        bail!("thumbnail previews are only available for local files");
    }
    let path = Path::new(video);
    if !path.exists() {
        bail!("file not found: {}", video);
    }

    let width = width.clamp(80, 480);
    let step = bucket_step(duration);
    let bucket_index = (seconds.max(0.0) / step).floor() as u64;
    let bucket_seconds = bucket_index as f64 * step;

    let cache_file = cache_path(path, bucket_index, width)?;
    if let Ok(bytes) = std::fs::read(&cache_file) {
        if !bytes.is_empty() {
            return Ok(Thumbnail { bytes, bucket_seconds });
        }
    }

    let bytes = extract(video, bucket_seconds, width, &cache_file)?;
    Ok(Thumbnail { bytes, bucket_seconds })
}

fn bucket_step(duration: f64) -> f64 {
    if !(duration.is_finite() && duration > 0.0) {
        return MIN_BUCKET_SECS * 2.0;
    }
    (duration / BUCKETS_PER_FILE).clamp(MIN_BUCKET_SECS, MAX_BUCKET_SECS)
}

fn extract(video: &str, at: f64, width: u32, out: &Path) -> Result<Vec<u8>> {
    let ffmpeg = find_ffmpeg().ok_or_else(|| {
        anyhow!("ffmpeg not found; thumbnail previews need it to decode frames")
    })?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow!("failed to create thumbnail cache dir: {}", e))?;
    }

    let mut cmd = std::process::Command::new(&ffmpeg);
    cmd.args([
        "-y",
        "-loglevel", "error",
        // Before -i: seek by jumping to the nearest keyframe instead of
        // decoding from the start. This is the whole performance story.
        "-ss", &format!("{:.3}", at),
        "-i", video,
        "-frames:v", "1",
        "-vf", &format!("scale={}:-2", width),
        // Previews are small and transient; quality 5 keeps them ~4 KB.
        "-q:v", "5",
    ]);
    cmd.arg(out);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }

    let result = cmd
        .output()
        .map_err(|e| anyhow!("failed to run ffmpeg: {}", e))?;

    if !result.status.success() || !out.exists() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        bail!(
            "could not extract a preview at {:.1}s: {}",
            at,
            stderr.chars().take(200).collect::<String>()
        );
    }

    let bytes = std::fs::read(out)
        .map_err(|e| anyhow!("failed to read extracted thumbnail: {}", e))?;
    if bytes.is_empty() {
        // Seeking past the last keyframe can produce an empty file rather
        // than an error. Don't leave it around to be served as a cache hit.
        let _ = std::fs::remove_file(out);
        bail!("no frame available at {:.1}s", at);
    }
    Ok(bytes)
}

/// `<cache>/unflick/thumbs/<video-key>/<width>-<bucket>.jpg`
///
/// The per-video key folds in the file's size and modification time, so
/// re-encoding a file under the same name produces a different key instead
/// of serving previews of the old content.
fn cache_path(video: &Path, bucket_index: u64, width: u32) -> Result<PathBuf> {
    let meta = std::fs::metadata(video)
        .map_err(|e| anyhow!("failed to stat {}: {}", video.display(), e))?;
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
    mix(video.to_string_lossy().as_bytes());
    mix(&meta.len().to_le_bytes());
    mix(&mtime.to_le_bytes());

    Ok(cache_root()
        .join(format!("{:016x}", hash))
        .join(format!("{}-{}.jpg", width, bucket_index)))
}

fn cache_root() -> PathBuf {
    dirs_next::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("unflick")
        .join("thumbs")
}

/// Drop the least recently used videos until the cache fits its budget.
///
/// Eviction is per-video rather than per-file: half a film's previews are
/// worse than none, because scrubbing would flicker between having a
/// preview and not.
pub fn prune_cache() {
    let root = cache_root();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };

    let mut dirs: Vec<(PathBuf, u64, std::time::SystemTime)> = Vec::new();
    let mut total: u64 = 0;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let mut size = 0u64;
        let mut newest = std::time::UNIX_EPOCH;
        if let Ok(files) = std::fs::read_dir(&path) {
            for file in files.flatten() {
                if let Ok(meta) = file.metadata() {
                    size += meta.len();
                    if let Ok(m) = meta.modified() {
                        newest = newest.max(m);
                    }
                }
            }
        }
        total += size;
        dirs.push((path, size, newest));
    }

    if total <= CACHE_BUDGET_BYTES {
        return;
    }

    // Oldest first.
    dirs.sort_by_key(|(_, _, newest)| *newest);
    for (path, size, _) in dirs {
        if total <= CACHE_BUDGET_BYTES {
            break;
        }
        if std::fs::remove_dir_all(&path).is_ok() {
            total = total.saturating_sub(size);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_step_scales_with_duration_and_stays_in_range() {
        // A 40-minute talk: 2400 / 200 = 12s steps.
        assert!((bucket_step(2400.0) - 12.0).abs() < 1e-9);
        // Short clip: floored, so we don't extract every 150 ms.
        assert_eq!(bucket_step(30.0), MIN_BUCKET_SECS);
        // Very long recording: capped, so steps stay useful.
        assert_eq!(bucket_step(36_000.0), MAX_BUCKET_SECS);
        // Unknown duration still yields something usable.
        assert!(bucket_step(0.0) > 0.0);
        assert!(bucket_step(f64::NAN) > 0.0);
    }

    #[test]
    fn positions_within_a_bucket_share_one_cache_entry() {
        let step = bucket_step(2400.0);
        let bucket = |s: f64| (s / step).floor() as u64;
        assert_eq!(bucket(0.0), bucket(step - 0.001));
        assert_ne!(bucket(step - 0.001), bucket(step));
    }
}
