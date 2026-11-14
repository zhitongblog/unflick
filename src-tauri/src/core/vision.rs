//! Hand the current frame to a multimodal model.
//!
//! An AI agent driving unflick can already read the transcript; this lets it
//! *look*. That only means anything because the CLI, MCP and the window
//! share one player — the frame an agent grabs is the frame on screen, not
//! one decoded by a second, invisible process.
//!
//! The frame is downscaled and re-encoded as JPEG before it leaves here. A
//! raw 4K PNG screenshot is several megabytes, and base64 inflates it by a
//! third; that blows a tool result budget for no benefit, since vision
//! models downscale anyway.

use std::path::PathBuf;

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

use super::player::{find_ffmpeg, Player};

/// Longest edge of the returned image, in pixels. Comfortably above what
/// vision models resolve, well below what would bloat a tool response.
const DEFAULT_MAX_EDGE: u32 = 768;

/// A captured frame. Kept as raw JPEG bytes so the two consumers can each
/// take what they need: MCP base64-encodes it into an image content block,
/// the CLI writes it to a file rather than printing a megabyte of base64 at
/// a terminal.
pub struct Frame {
    pub bytes: Vec<u8>,
    pub mime_type: &'static str,
    /// Playback position the frame was taken at, in seconds.
    pub position: f64,
}

impl Frame {
    pub fn to_base64(&self) -> String {
        base64_encode(&self.bytes)
    }

    /// Write the JPEG to `path`, creating parent directories as needed.
    pub fn write_to(&self, path: &str) -> Result<()> {
        let path = std::path::Path::new(path);
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)
                .map_err(|e| anyhow!("failed to create {}: {}", parent.display(), e))?;
        }
        std::fs::write(path, &self.bytes)
            .map_err(|e| anyhow!("failed to write {}: {}", path.display(), e))
    }
}

/// Grab the frame showing right now, scaled to fit `max_edge`.
///
/// `seek_to` optionally moves playback first — an agent asking about a
/// specific moment shouldn't have to issue a separate seek and guess how
/// long decoding takes.
pub fn capture_frame(player: &Player, seek_to: Option<f64>, max_edge: Option<u32>) -> Result<Frame> {
    if player.status().file.is_none() {
        bail!("nothing is playing");
    }

    if let Some(pos) = seek_to {
        player.seek(pos)?;
        // Let the decoder land on the target frame before we screenshot it;
        // mpv reports the new time-pos before the picture has caught up.
        std::thread::sleep(std::time::Duration::from_millis(400));
    }

    let max_edge = max_edge.unwrap_or(DEFAULT_MAX_EDGE).clamp(64, 2048);

    let raw = temp_path("png");
    if let Some(parent) = raw.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    // `screenshot-to-file ... video` captures the decoded frame without
    // subtitles or OSD burned in — the model should see the picture, not
    // our overlay.
    player
        .screenshot(&raw.to_string_lossy())
        .map_err(|e| anyhow!("failed to capture frame: {}", e))?;
    if !raw.exists() {
        bail!("mpv reported success but wrote no screenshot");
    }

    let scaled = temp_path("jpg");
    let encode = shrink_to_jpeg(&raw, &scaled, max_edge);
    let _ = std::fs::remove_file(&raw);
    encode?;

    let bytes = std::fs::read(&scaled)
        .map_err(|e| anyhow!("failed to read encoded frame: {}", e))?;
    let _ = std::fs::remove_file(&scaled);

    Ok(Frame {
        bytes,
        mime_type: "image/jpeg",
        position: player.status().position,
    })
}

/// Downscale with ffmpeg. `-2` on the height keeps the aspect ratio while
/// staying on an even number of pixels, which the JPEG encoder requires.
fn shrink_to_jpeg(input: &PathBuf, output: &PathBuf, max_edge: u32) -> Result<()> {
    let ffmpeg = find_ffmpeg()
        .ok_or_else(|| anyhow!("ffmpeg not found; cannot encode the captured frame"))?;

    // Scale only if the frame is larger than the target, so a small video
    // isn't upscaled into a bigger payload than the original.
    let filter = format!(
        "scale='if(gt(iw,ih),min({m},iw),-2)':'if(gt(iw,ih),-2,min({m},ih))'",
        m = max_edge
    );

    let mut cmd = std::process::Command::new(&ffmpeg);
    cmd.args(["-y", "-loglevel", "error", "-i"]);
    cmd.arg(input);
    cmd.args(["-vf", &filter, "-q:v", "4", "-frames:v", "1"]);
    cmd.arg(output);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }

    let result = cmd.output().map_err(|e| anyhow!("failed to run ffmpeg: {}", e))?;
    if !result.status.success() || !output.exists() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        bail!(
            "ffmpeg failed to encode the frame: {}",
            stderr.chars().take(200).collect::<String>()
        );
    }
    Ok(())
}

/// Downscale an already-encoded image held in memory, and hand back JPEG.
///
/// The window captures (`core::dev`) arrive as PNG bytes rather than as a
/// file, but want exactly the treatment a captured frame gets: one ffmpeg
/// downscale, JPEG out, small enough to sit in a tool response. Reusing that
/// path costs two temp files and keeps there being one place that knows how
/// to shrink a picture — the alternative was linking an image crate for a
/// second way to do the same thing.
pub(crate) fn shrink_bytes_to_jpeg(bytes: &[u8], ext: &str, max_edge: u32) -> Result<Vec<u8>> {
    let input = temp_path(ext);
    if let Some(parent) = input.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow!("failed to create {}: {}", parent.display(), e))?;
    }
    std::fs::write(&input, bytes)
        .map_err(|e| anyhow!("failed to stage the capture for encoding: {}", e))?;

    let output = temp_path("jpg");
    let encode = shrink_to_jpeg(&input, &output, max_edge);
    let _ = std::fs::remove_file(&input);
    encode?;

    let out = std::fs::read(&output)
        .map_err(|e| anyhow!("failed to read the encoded capture: {}", e))?;
    let _ = std::fs::remove_file(&output);
    Ok(out)
}

/// Multi-frame grid contact sheet for giving a vision model a single-image
/// overview of a time span.
#[derive(Debug, Clone)]
pub struct ContactSheet {
    pub bytes: Vec<u8>,
    pub mime_type: &'static str,
    pub cols: u32,
    pub rows: u32,
    pub start: f64,
    pub end: f64,
    pub timestamps: Vec<f64>,
}

impl ContactSheet {
    pub fn to_base64(&self) -> String {
        base64_encode(&self.bytes)
    }

    pub fn write_to(&self, path: &str) -> Result<()> {
        let path = std::path::Path::new(path);
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)
                .map_err(|e| anyhow!("failed to create {}: {}", parent.display(), e))?;
        }
        std::fs::write(path, &self.bytes)
            .map_err(|e| anyhow!("failed to write {}: {}", path.display(), e))
    }
}

/// Sample `cols * rows` frames evenly across `[start, end]` and tile them into
/// a single JPEG contact sheet.
pub fn capture_contact_sheet(
    player: &Player,
    file_override: Option<&str>,
    cols: Option<u32>,
    rows: Option<u32>,
    start: Option<f64>,
    end: Option<f64>,
    max_edge: Option<u32>,
) -> Result<ContactSheet> {
    let status = player.status();
    let file = match file_override.map(str::trim).filter(|f| !f.is_empty()) {
        Some(f) => f.to_string(),
        None => status.file.ok_or_else(|| anyhow!("nothing is playing"))?,
    };
    if super::disc::detect(&file).is_some() {
        bail!("cannot build a contact sheet directly from an optical disc");
    }

    let ffmpeg = find_ffmpeg()
        .ok_or_else(|| anyhow!("ffmpeg not found; cannot build contact sheet"))?;

    let cols = cols.unwrap_or(4).clamp(1, 6);
    let rows = rows.unwrap_or(3).clamp(1, 6);
    let total_cells = (cols * rows) as usize;
    let max_edge = max_edge.unwrap_or(1280).clamp(256, 2048);
    let cell_w = (max_edge / cols).max(120);

    let total_dur = if status.duration > 0.0 && file_override.is_none() {
        status.duration
    } else {
        probe_duration(&ffmpeg, &file).unwrap_or(10.0)
    };

    let t_start = start.unwrap_or(0.0).max(0.0);
    let t_end = end
        .unwrap_or(total_dur)
        .min(if total_dur > 0.0 { total_dur } else { f64::MAX })
        .max(t_start + 0.2);
    let span = (t_end - t_start).max(0.2);

    let timestamps: Vec<f64> = (0..total_cells)
        .map(|i| {
            let ratio = (i as f64 + 0.5) / total_cells as f64;
            ((t_start + ratio * span) * 100.0).round() / 100.0
        })
        .collect();

    let output = temp_path("jpg");
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let mut cmd = std::process::Command::new(&ffmpeg);
    cmd.args(["-y", "-loglevel", "error"]);
    for &ts in &timestamps {
        cmd.args(["-ss", &format!("{:.3}", ts), "-i", &file]);
    }

    let mut filter = String::new();
    for i in 0..total_cells {
        filter.push_str(&format!(
            "[{i}:v]scale={cell_w}:-2:flags=fast_bilinear,trim=end_frame=1,setpts=PTS-STARTPTS[v{i}];"
        ));
    }
    for i in 0..total_cells {
        filter.push_str(&format!("[v{i}]"));
    }
    filter.push_str(&format!(
        "concat=n={total_cells}:v=1:a=0,tile={cols}x{rows}:padding=4:margin=4"
    ));

    cmd.args(["-filter_complex", &filter, "-frames:v", "1", "-q:v", "4"]);
    cmd.arg(&output);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }

    let result = cmd
        .output()
        .map_err(|e| anyhow!("failed to run ffmpeg for contact sheet: {}", e))?;
    if !result.status.success() || !output.exists() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        bail!(
            "ffmpeg failed to build contact sheet: {}",
            stderr.chars().take(240).collect::<String>()
        );
    }

    let bytes = std::fs::read(&output)
        .map_err(|e| anyhow!("failed to read contact sheet: {}", e))?;
    let _ = std::fs::remove_file(&output);

    Ok(ContactSheet {
        bytes,
        mime_type: "image/jpeg",
        cols,
        rows,
        start: t_start,
        end: t_end,
        timestamps,
    })
}

/// One detected visual scene boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneCut {
    pub index: usize,
    pub time: f64,
    pub title: String,
}

/// Detect visual scene cuts using ffmpeg's `select='gt(scene,threshold)',showinfo`
/// filter and return ordered scene start timestamps.
pub fn detect_scenes(
    player: &Player,
    file_override: Option<&str>,
    threshold: Option<f64>,
    max_scenes: Option<usize>,
) -> Result<Vec<SceneCut>> {
    let status = player.status();
    let file = match file_override.map(str::trim).filter(|f| !f.is_empty()) {
        Some(f) => f.to_string(),
        None => status.file.ok_or_else(|| anyhow!("nothing is playing"))?,
    };
    if super::disc::detect(&file).is_some() {
        bail!("cannot run scene detection directly on an optical disc");
    }

    let ffmpeg = find_ffmpeg()
        .ok_or_else(|| anyhow!("ffmpeg not found; cannot detect scenes"))?;

    let thresh = threshold.unwrap_or(0.3).clamp(0.05, 0.95);
    let limit = max_scenes.unwrap_or(20).clamp(1, 100);

    let filter = format!("select='gt(scene,{:.3})',showinfo", thresh);
    let mut cmd = std::process::Command::new(&ffmpeg);
    cmd.args([
        "-nostats",
        "-loglevel",
        "info",
        "-i",
        &file,
        "-vf",
        &filter,
        "-an",
        "-f",
        "null",
        "-",
    ]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }

    let output = cmd
        .output()
        .map_err(|e| anyhow!("failed to run ffmpeg for scene detection: {}", e))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw_times = parse_showinfo_timestamps(&stderr);

    let mut cuts: Vec<f64> = vec![0.0];
    for t in raw_times {
        if cuts.len() >= limit {
            break;
        }
        if t - *cuts.last().unwrap_or(&0.0) >= 1.5 {
            cuts.push((t * 100.0).round() / 100.0);
        }
    }

    Ok(cuts
        .into_iter()
        .enumerate()
        .map(|(i, time)| SceneCut {
            index: i,
            time,
            title: format!("Scene {}", i + 1),
        })
        .collect())
}

fn parse_showinfo_timestamps(stderr: &str) -> Vec<f64> {
    let mut times = Vec::new();
    for line in stderr.lines() {
        if !line.contains("showinfo") {
            continue;
        }
        if let Some(after) = line.split("pts_time:").nth(1) {
            let token = after
                .split(|c: char| c.is_whitespace() || c == ']')
                .next()
                .unwrap_or("");
            if let Ok(val) = token.parse::<f64>() {
                if val.is_finite() && val >= 0.0 {
                    times.push(val);
                }
            }
        }
    }
    times
}

fn probe_duration(ffmpeg: &PathBuf, file: &str) -> Option<f64> {
    let mut cmd = std::process::Command::new(ffmpeg);
    cmd.args(["-i", file]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let out = cmd.output().ok()?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    for line in stderr.lines() {
        if let Some(rest) = line.trim().strip_prefix("Duration:") {
            let dur_str = rest.split(',').next()?.trim();
            let parts: Vec<&str> = dur_str.split(':').collect();
            if parts.len() == 3 {
                let h: f64 = parts[0].parse().ok()?;
                let m: f64 = parts[1].parse().ok()?;
                let s: f64 = parts[2].parse().ok()?;
                return Some(h * 3600.0 + m * 60.0 + s);
            }
        }
    }
    None
}

fn temp_path(ext: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir()
        .join("unflick")
        .join(format!("frame-{}.{}", stamp, ext))
}

/// Standard base64. Written out rather than pulled in as a dependency —
/// it's twenty lines, and the two callers (frame capture and timeline
/// previews) both just need JPEG bytes in a JSON field.
pub fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);

    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(ALPHABET[(triple >> 18 & 0x3f) as usize] as char);
        out.push(ALPHABET[(triple >> 12 & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(triple >> 6 & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(triple & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{base64_encode, parse_showinfo_timestamps};

    #[test]
    fn base64_matches_rfc4648_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_handles_high_bytes() {
        assert_eq!(base64_encode(&[0xff, 0xd8, 0xff]), "/9j/");
        assert_eq!(base64_encode(&[0x00, 0x00, 0x00]), "AAAA");
    }

    #[test]
    fn parses_showinfo_pts_time_lines() {
        let log = "[Parsed_showinfo_1 @ 0x1] n:   0 pts:  12000 pts_time:4.250000 pos: 123\n\
                   [Parsed_showinfo_1 @ 0x1] n:   1 pts:  36000 pts_time:12.5 pos: 456\n";
        let times = parse_showinfo_timestamps(log);
        assert_eq!(times, vec![4.25, 12.5]);
    }
}
