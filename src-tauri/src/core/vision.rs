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
    use super::base64_encode;

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
}
