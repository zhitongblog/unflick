//! Transcript access: turn whatever subtitles the current file has into a
//! searchable list of timed cues.
//!
//! This is what separates unflick's MCP surface from a script that pokes a
//! player from the outside. An external wrapper can send `seek 120`; it
//! can't answer "where does he explain the refund policy?", because that
//! needs the subtitle track the player already has open.
//!
//! Sources are tried in the order a user would expect:
//!
//!   1. the subtitle track currently selected, if it came from a file
//!      (this is the whisper-generated track, right after `subtitle
//!      generate`)
//!   2. any other external track that's loaded
//!   3. a sidecar `.srt` / `.vtt` sitting next to the video
//!   4. an embedded text track, extracted once with ffmpeg and cached
//!
//! Image-based subtitles (PGS, VobSub) carry no text and are skipped —
//! there's nothing to search without OCR.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

use super::player::Player;

/// One timed subtitle line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cue {
    pub index: usize,
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// A loaded transcript plus where it came from, so callers can tell the
/// user whether they're searching a whisper draft or the real subtitles.
#[derive(Debug, Clone, Serialize)]
pub struct Transcript {
    /// Absolute path of the subtitle file backing this transcript.
    pub source: String,
    /// How the source was found: "selected" | "external" | "sidecar" | "embedded".
    pub origin: String,
    pub cues: Vec<Cue>,
}

/// Subtitle containers we can read as text.
const TEXT_SUB_EXTENSIONS: &[&str] = &["srt", "vtt"];

// ─── Parsing ──────────────────────────────────────────────────────────────

/// Parse SRT or WebVTT. The two differ only in the decimal separator and a
/// header line, so one parser covers both rather than shipping two that
/// drift apart.
pub fn parse(content: &str) -> Vec<Cue> {
    let mut cues = Vec::new();

    // Strip a UTF-8 BOM and the WEBVTT header if present.
    let content = content.trim_start_matches('\u{feff}');

    for block in content.split("\n\n").flat_map(|b| b.split("\r\n\r\n")) {
        let block = block.trim();
        if block.is_empty() || block.starts_with("WEBVTT") {
            continue;
        }

        let mut lines = block.lines();
        let mut timing_line = match lines.next() {
            Some(l) => l.trim(),
            None => continue,
        };

        // SRT blocks lead with a sequence number; VTT blocks may lead with
        // an optional cue identifier. Either way, if the first line has no
        // arrow the timing is on the next one.
        if !timing_line.contains("-->") {
            timing_line = match lines.next() {
                Some(l) => l.trim(),
                None => continue,
            };
        }

        let Some((start, end)) = parse_timing(timing_line) else {
            continue;
        };

        let text = lines
            .map(|l| strip_tags(l.trim()))
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ");

        if text.is_empty() {
            continue;
        }

        cues.push(Cue {
            index: cues.len(),
            start,
            end,
            text,
        });
    }

    cues
}

/// `00:01:02,500 --> 00:01:05,000` (SRT) or `00:01:02.500 --> 00:01:05.000`
/// (VTT, possibly with trailing cue settings).
fn parse_timing(line: &str) -> Option<(f64, f64)> {
    let (left, right) = line.split_once("-->")?;
    // VTT allows `align:start position:50%` after the end timestamp.
    let right = right.trim().split_whitespace().next()?;
    Some((parse_timestamp(left.trim())?, parse_timestamp(right)?))
}

fn parse_timestamp(raw: &str) -> Option<f64> {
    let normalised = raw.replace(',', ".");
    let mut parts = normalised.split(':').collect::<Vec<_>>();
    // VTT permits `MM:SS.mmm` with no hour field.
    if parts.len() == 2 {
        parts.insert(0, "0");
    }
    if parts.len() != 3 {
        return None;
    }
    let h: f64 = parts[0].trim().parse().ok()?;
    let m: f64 = parts[1].trim().parse().ok()?;
    let s: f64 = parts[2].trim().parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + s)
}

/// Drop the inline markup subtitles carry — `<i>`, `{\an8}` and friends —
/// so a search for "refund" isn't defeated by an italic tag in the middle
/// of the word.
fn strip_tags(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut depth_angle = 0usize;
    let mut depth_brace = 0usize;
    for ch in line.chars() {
        match ch {
            '<' => depth_angle += 1,
            '>' => depth_angle = depth_angle.saturating_sub(1),
            '{' => depth_brace += 1,
            '}' => depth_brace = depth_brace.saturating_sub(1),
            _ if depth_angle == 0 && depth_brace == 0 => out.push(ch),
            _ => {}
        }
    }
    out.trim().to_string()
}

// ─── Source resolution ────────────────────────────────────────────────────

/// Load a transcript for whatever is playing right now.
pub fn load_current(player: &Player) -> Result<Transcript> {
    let video = player
        .status()
        .file
        .ok_or_else(|| anyhow!("nothing is playing"))?;

    if let Some((path, origin)) = external_track(player) {
        return read_transcript(&path, origin);
    }
    if let Some(path) = sidecar_for(&video) {
        return read_transcript(&path.to_string_lossy(), "sidecar");
    }
    if let Some(path) = extract_embedded(player, &video)? {
        return read_transcript(&path.to_string_lossy(), "embedded");
    }

    bail!(
        "no readable subtitles for this file. Load a subtitle file, or run \
         `unflick subtitle generate` to transcribe it first."
    )
}

fn read_transcript(path: &str, origin: &str) -> Result<Transcript> {
    let raw = read_text_lossy(Path::new(path))?;
    let cues = parse(&raw);
    if cues.is_empty() {
        bail!("subtitle file has no readable cues: {}", path);
    }
    Ok(Transcript {
        source: path.to_string(),
        origin: origin.to_string(),
        cues,
    })
}

/// Subtitle files are frequently not UTF-8 (legacy CJK encodings especially).
/// Rather than fail, fall back to a lossy decode — a few mangled characters
/// beat refusing to search the file at all.
fn read_text_lossy(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)
        .map_err(|e| anyhow!("failed to read {}: {}", path.display(), e))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// The selected external subtitle track, else any external one.
fn external_track(player: &Player) -> Option<(String, &'static str)> {
    let tracks = player.subtitle_list();
    if let Some(t) = tracks.iter().find(|t| t.selected) {
        if let Some(file) = t.external_file.as_ref().filter(|f| is_text_sub(f)) {
            return Some((file.clone(), "selected"));
        }
    }
    tracks
        .iter()
        .find_map(|t| t.external_file.as_ref().filter(|f| is_text_sub(f)))
        .map(|f| (f.clone(), "external"))
}

fn is_text_sub(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| TEXT_SUB_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// A `.srt` / `.vtt` sharing the video's stem. Also checks the stem with
/// a language suffix stripped (`movie.en.srt`), which is how most
/// downloaded subtitles are named.
fn sidecar_for(video: &str) -> Option<PathBuf> {
    let path = Path::new(video);
    let dir = path.parent()?;
    let stem = path.file_stem()?.to_string_lossy().into_owned();

    for ext in TEXT_SUB_EXTENSIONS {
        let direct = dir.join(format!("{}.{}", stem, ext));
        if direct.exists() {
            return Some(direct);
        }
    }

    // `movie.<anything>.srt` — take the first match so a lone `movie.en.srt`
    // is found without guessing at language codes.
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&format!("{}.", stem)) && is_text_sub(&name) {
            return Some(entry.path());
        }
    }
    None
}

/// Pull an embedded text subtitle track out with ffmpeg, once, into the
/// cache directory. Returns `None` when the file has no text track.
fn extract_embedded(player: &Player, video: &str) -> Result<Option<PathBuf>> {
    // Image-based tracks (`hdmv_pgs_subtitle`, `dvd_subtitle`) can't be
    // converted to text, so only count tracks ffmpeg can write as SRT.
    let tracks = player.subtitle_list();
    let embedded: Vec<_> = tracks.iter().filter(|t| t.external_file.is_none()).collect();
    if embedded.is_empty() {
        return Ok(None);
    }
    // Prefer the selected track, else the first one.
    let track = embedded.iter().find(|t| t.selected).unwrap_or(&embedded[0]);

    // mpv's `sid` is 1-based across subtitle tracks in file order, which
    // maps onto ffmpeg's `0:s:<n>` with n = sid - 1.
    let stream_index = (track.id - 1).max(0);

    let out = cache_path_for(video, stream_index);
    if out.exists() {
        return Ok(Some(out));
    }
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow!("failed to create transcript cache dir: {}", e))?;
    }

    let ffmpeg = super::player::find_ffmpeg()
        .ok_or_else(|| anyhow!("ffmpeg not found; cannot read embedded subtitles"))?;

    let mut cmd = std::process::Command::new(&ffmpeg);
    cmd.args([
        "-y",
        "-loglevel",
        "error",
        "-i",
        video,
        "-map",
        &format!("0:s:{}", stream_index),
        "-f",
        "srt",
    ]);
    cmd.arg(&out);
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
        // A PGS track lands here. Report it as "no text", not as a crash.
        bail!(
            "could not extract embedded subtitles (image-based tracks have no text): {}",
            stderr.chars().take(200).collect::<String>()
        );
    }
    Ok(Some(out))
}

fn cache_path_for(video: &str, stream_index: i64) -> PathBuf {
    // Hash the path so arbitrarily long / non-ASCII filenames can't produce
    // an invalid cache filename.
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in video.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    dirs_next::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("unflick")
        .join("transcripts")
        .join(format!("{:016x}-{}.srt", hash, stream_index))
}

// ─── Search ───────────────────────────────────────────────────────────────

/// Case-insensitive substring search over the cue text.
///
/// Deliberately not regex: this is aimed at "find where they say X", and a
/// stray `(` in a natural-language query shouldn't turn into a syntax error.
pub fn search<'a>(cues: &'a [Cue], query: &str, limit: usize) -> Vec<&'a Cue> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    cues.iter()
        .filter(|c| c.text.to_lowercase().contains(&needle))
        .take(limit)
        .collect()
}

// ─── Chapter derivation ───────────────────────────────────────────────────

/// Propose chapter breaks from the transcript's silences.
///
/// The heuristic: the longest pauses between cues are the most likely topic
/// boundaries. It picks the `target - 1` biggest gaps, subject to a minimum
/// spacing so a burst of pauses can't produce five chapters in one minute.
///
/// This is a baseline, not topic modelling. An agent that has read the
/// transcript will usually do better — that's what `chapters_set` is for.
pub fn derive_chapters(cues: &[Cue], target: usize, duration: f64) -> Vec<(f64, String)> {
    if cues.is_empty() || target < 2 {
        return Vec::new();
    }

    let total = if duration > 0.0 {
        duration
    } else {
        cues.last().map(|c| c.end).unwrap_or(0.0)
    };
    if total <= 0.0 {
        return Vec::new();
    }

    // No chapter shorter than this. Scaled to the runtime so it behaves on
    // both a 3-minute clip and a 3-hour film.
    let min_spacing = (total / target as f64) * 0.4;

    let mut gaps: Vec<(f64, usize)> = cues
        .windows(2)
        .enumerate()
        .map(|(i, w)| (w[1].start - w[0].end, i + 1))
        .collect();
    gaps.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut boundaries: Vec<usize> = Vec::new();
    for (_, cue_index) in gaps {
        if boundaries.len() + 1 >= target {
            break;
        }
        let start = cues[cue_index].start;
        let too_close = boundaries
            .iter()
            .any(|&b| (cues[b].start - start).abs() < min_spacing)
            || start < min_spacing
            || total - start < min_spacing;
        if !too_close {
            boundaries.push(cue_index);
        }
    }
    boundaries.sort_unstable();

    let mut chapters = vec![(0.0, title_from(&cues[0].text))];
    for b in boundaries {
        chapters.push((cues[b].start, title_from(&cues[b].text)));
    }
    chapters
}

/// First handful of words of a cue, used as a chapter title.
fn title_from(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().take(7).collect();
    let mut title = words.join(" ");
    if title.chars().count() > 60 {
        title = title.chars().take(57).collect::<String>() + "…";
    }
    if title.is_empty() {
        title = "Chapter".to_string();
    }
    title
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_srt_with_markup_and_multiline_text() {
        let srt = "1\n00:00:01,000 --> 00:00:04,500\n<i>Hello</i> there\nsecond line\n\n\
                   2\n00:00:05,000 --> 00:00:06,000\n{\\an8}Second cue\n";
        let cues = parse(srt);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start, 1.0);
        assert_eq!(cues[0].end, 4.5);
        assert_eq!(cues[0].text, "Hello there second line");
        assert_eq!(cues[1].text, "Second cue");
    }

    #[test]
    fn parses_vtt_without_hours_or_index() {
        let vtt = "WEBVTT\n\n00:01.000 --> 00:03.000 align:start\nNo hour field\n";
        let cues = parse(vtt);
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].start, 1.0);
        assert_eq!(cues[0].text, "No hour field");
    }

    #[test]
    fn search_is_case_insensitive_and_limited() {
        let cues = parse(
            "1\n00:00:01,000 --> 00:00:02,000\nThe REFUND policy\n\n\
             2\n00:00:03,000 --> 00:00:04,000\nrefund again\n\n\
             3\n00:00:05,000 --> 00:00:06,000\nunrelated\n",
        );
        assert_eq!(search(&cues, "refund", 10).len(), 2);
        assert_eq!(search(&cues, "refund", 1).len(), 1);
        assert!(search(&cues, "  ", 10).is_empty());
    }

    #[test]
    fn derived_chapters_start_at_zero_and_respect_spacing() {
        let mut srt = String::new();
        for i in 0..20 {
            // A long pause every fifth cue.
            let start = i as f64 * 10.0 + if i % 5 == 0 { 5.0 } else { 0.0 };
            srt.push_str(&format!(
                "{}\n00:00:{:02},000 --> 00:00:{:02},000\nline {}\n\n",
                i + 1,
                start as u32,
                start as u32 + 1,
                i
            ));
        }
        let cues = parse(&srt);
        let chapters = derive_chapters(&cues, 4, 210.0);
        assert!(chapters.len() >= 2 && chapters.len() <= 4);
        assert_eq!(chapters[0].0, 0.0);
        for pair in chapters.windows(2) {
            assert!(pair[1].0 > pair[0].0, "chapters must be ordered");
        }
    }
}
