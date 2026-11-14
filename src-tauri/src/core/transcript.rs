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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

/// Aggregated speaking statistics for one identified speaker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeakerStat {
    pub name: String,
    pub cue_count: usize,
    pub word_count: usize,
    pub speaking_seconds: f64,
    pub first_appearance: f64,
}

/// A loaded transcript plus where it came from, so callers can tell the
/// user whether they're searching a whisper draft or the real subtitles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transcript {
    /// Absolute path of the subtitle file backing this transcript.
    pub source: String,
    /// How the source was found: "selected" | "external" | "sidecar" | "embedded".
    pub origin: String,
    pub cues: Vec<Cue>,
}

/// One matching subtitle cue from a library-wide transcript search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryTranscriptMatch {
    pub media_id: i64,
    pub path: String,
    pub title: String,
    pub source: String,
    pub origin: String,
    pub cue: Cue,
}

/// One structural segment of a transcript summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub index: usize,
    pub start: f64,
    pub end: f64,
    pub title: String,
    pub cue_count: usize,
    pub preview: String,
}

/// Structured overview of a transcript: total cues, word count, speaking
/// duration, top keywords, speaker breakdown, and timeline segments with representative lines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSummary {
    pub source: String,
    pub origin: String,
    pub cue_count: usize,
    pub word_count: usize,
    pub duration: f64,
    pub speaking_seconds: f64,
    pub keywords: Vec<String>,
    pub speakers: Vec<SpeakerStat>,
    pub segments: Vec<TranscriptSegment>,
}

/// Subtitle containers we can read as text.
const TEXT_SUB_EXTENSIONS: &[&str] = &["srt", "vtt", "ass", "ssa"];

// ─── Parsing ──────────────────────────────────────────────────────────────

/// Parse SRT, WebVTT, or ASS/SSA into timed cues (preserving speaker tags
/// when present in ASS `Name`, WebVTT `<v Name>`, `whisper --tinydiarize`
/// `[SPEAKER_TURN]`, or inline `[Speaker 1]:` / `ALICE:` prefixes).
pub fn parse(content: &str) -> Vec<Cue> {
    let content = content.trim_start_matches('\u{feff}');

    if is_ass_content(content) {
        return parse_ass(content);
    }

    let mut cues = Vec::new();
    let mut tdrz_speaker_idx: Option<usize> = None;

    for block in content.split("\n\n").flat_map(|b| b.split("\r\n\r\n")) {
        let block = block.trim();
        if block.is_empty() || block.starts_with("WEBVTT") || block.starts_with("NOTE") {
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

        let raw_lines: Vec<&str> = lines.map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
        if raw_lines.is_empty() {
            continue;
        }

        // Check for whisper.cpp `--tinydiarize` (`[SPEAKER_TURN]`) marker.
        let joined_raw = raw_lines.join(" ");
        if joined_raw.contains("[SPEAKER_TURN]") {
            tdrz_speaker_idx = Some(match tdrz_speaker_idx {
                None => 1,
                Some(1) => 2,
                Some(_) => 1,
            });
        }

        // Check for WebVTT `<v Speaker Name>` voice span before stripping HTML tags.
        let vtt_speaker = raw_lines.iter().find_map(|l| extract_vtt_voice(l));

        let cleaned = raw_lines
            .iter()
            .map(|l| {
                let without_tdrz = l.replace("[SPEAKER_TURN]", "");
                strip_tags(without_tdrz.trim())
            })
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ");

        if cleaned.is_empty() {
            continue;
        }

        let (inline_speaker, text) = extract_inline_speaker(&cleaned);
        if text.is_empty() {
            continue;
        }

        let speaker = vtt_speaker
            .or(inline_speaker)
            .or_else(|| tdrz_speaker_idx.map(|n| format!("Speaker {}", n)));

        cues.push(Cue {
            index: cues.len(),
            start,
            end,
            text,
            speaker,
        });
    }

    cues
}

fn is_ass_content(content: &str) -> bool {
    content.contains("[Events]")
        || content.contains("[Script Info]")
        || content
            .lines()
            .any(|l| l.trim_start().starts_with("Dialogue:"))
}

/// Parse SubStation Alpha (`.ass` / `.ssa`) `[Events]` `Dialogue:` lines.
fn parse_ass(content: &str) -> Vec<Cue> {
    let mut cues = Vec::new();
    let mut start_col = 1usize;
    let mut end_col = 2usize;
    let mut name_col = Some(4usize);
    let mut text_col = 9usize;
    let mut field_count = 10usize;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if let Some(rest) = line.strip_prefix("Format:") {
            let cols: Vec<String> = rest
                .split(',')
                .map(|c| c.trim().to_ascii_lowercase())
                .collect();
            if cols.len() >= 3 {
                field_count = cols.len();
                if let Some(i) = cols.iter().position(|c| c == "start") {
                    start_col = i;
                }
                if let Some(i) = cols.iter().position(|c| c == "end") {
                    end_col = i;
                }
                name_col = cols.iter().position(|c| c == "name" || c == "actor");
                if let Some(i) = cols.iter().position(|c| c == "text") {
                    text_col = i;
                } else {
                    text_col = cols.len() - 1;
                }
            }
            continue;
        }

        let Some(rest) = line.strip_prefix("Dialogue:") else {
            continue;
        };

        let parts: Vec<&str> = rest.splitn(field_count, ',').collect();
        if parts.len() <= text_col || parts.len() <= start_col.max(end_col) {
            continue;
        }

        let Some(start) = parse_timestamp(parts[start_col].trim()) else {
            continue;
        };
        let Some(end) = parse_timestamp(parts[end_col].trim()) else {
            continue;
        };

        let raw_text = parts[text_col].trim();
        // Skip vector drawing commands (`{\p1}...`).
        if raw_text.contains(r"{\p1") || raw_text.contains(r"{\p2") {
            continue;
        }

        let normalized_breaks = raw_text
            .replace(r"\N", " ")
            .replace(r"\n", " ")
            .replace(r"\h", " ");
        let cleaned = strip_tags(&normalized_breaks);
        if cleaned.is_empty() {
            continue;
        }

        let ass_name = name_col
            .and_then(|idx| parts.get(idx))
            .map(|s| s.trim())
            .filter(|s| {
                !s.is_empty()
                    && !s.eq_ignore_ascii_case("default")
                    && !s.eq_ignore_ascii_case("ntp")
                    && !s.eq_ignore_ascii_case("comment")
            })
            .map(|s| s.to_string());

        let (inline_speaker, final_text) = extract_inline_speaker(&cleaned);
        if final_text.is_empty() {
            continue;
        }

        cues.push(Cue {
            index: cues.len(),
            start,
            end,
            text: final_text,
            speaker: ass_name.or(inline_speaker),
        });
    }

    cues
}

/// Extract speaker name from a WebVTT `<v ...>` tag, e.g. `<v Alice>Hi` or
/// `<v.loud Bob Smith>Hello`.
fn extract_vtt_voice(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("<v")?;
    let close = rest.find('>')?;
    let inside = &rest[..close];
    // `<v.class1.class2 Speaker Name>` — classes are attached to `<v` before
    // the first space.
    let space_idx = inside.find(char::is_whitespace)?;
    let speaker = inside[space_idx..].trim();
    (!speaker.is_empty()).then(|| speaker.to_string())
}

const NON_SPEAKER_LABELS: &[&str] = &[
    "http", "https", "note", "warning", "error", "tip", "time", "date",
    "location", "chapter", "part", "scene", "step", "version", "rule",
    "example", "source", "status", "result", "title", "todo", "music",
    "sfx", "sound", "audio", "video",
];

/// Extract an inline speaker prefix such as `[Speaker 1]: Hello`,
/// `(ALICE) Hello`, `SPEAKER_00: Hello`, or `Alice: Hello`.
fn extract_inline_speaker(line: &str) -> (Option<String>, String) {
    let trimmed = line.trim();
    let without_dash = trimmed
        .strip_prefix('-')
        .map(|s| s.trim_start())
        .unwrap_or(trimmed);

    // 1. Bracketed speaker: `[Speaker 1]: Hello` or `[Alice] Hello`
    if let Some(rest) = without_dash.strip_prefix('[') {
        if let Some((inside, after)) = rest.split_once(']') {
            let label = inside.trim();
            let after_trimmed = after
                .trim_start()
                .strip_prefix(':')
                .or_else(|| after.trim_start().strip_prefix('：'))
                .map(|s| s.trim_start());
            if let Some(body) = after_trimmed {
                if is_plausible_speaker_label(label) && !body.is_empty() {
                    return (Some(label.to_string()), body.to_string());
                }
            }
        }
    }

    // 2. Colon-separated prefix: `Speaker 1: Hello`, `ALICE: Hello`, `说话人1：你好`
    if let Some((prefix, after)) = without_dash
        .split_once(':')
        .or_else(|| without_dash.split_once('：'))
    {
        let label = prefix.trim();
        let body = after.trim();
        if !body.is_empty() && is_plausible_speaker_label(label) {
            return (Some(label.to_string()), body.to_string());
        }
    }

    (None, trimmed.to_string())
}

fn is_plausible_speaker_label(label: &str) -> bool {
    let char_count = label.chars().count();
    if char_count == 0 || char_count > 24 {
        return false;
    }
    if label.split_whitespace().count() > 3 {
        return false;
    }
    let lower = label.to_lowercase();
    if NON_SPEAKER_LABELS.contains(&lower.as_str()) {
        return false;
    }
    if lower.starts_with("speaker")
        || lower.starts_with("说话人")
        || lower.starts_with("角色")
        || lower.starts_with("host")
        || lower.starts_with("guest")
        || lower.starts_with("narrator")
    {
        return true;
    }
    // Avoid mistaking numeric timestamps (`12:30`) or ratios for speakers.
    if label.chars().all(|c| c.is_ascii_digit() || c.is_whitespace()) {
        return false;
    }
    let first = label.chars().next().unwrap();
    let valid_start = first.is_ascii_uppercase() || !first.is_ascii();
    let valid_chars = label
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.'));
    valid_start && valid_chars
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

/// Load a transcript for an arbitrary file on disk (sidecar `.srt`/`.vtt`,
/// cached extracted embedded subtitle, or a direct `.srt`/`.vtt` path)
/// without needing the file to be currently open in mpv.
pub fn load_for_file(video: &str) -> Result<Transcript> {
    if is_text_sub(video) && Path::new(video).is_file() {
        return read_transcript(video, "file");
    }
    if let Some(path) = sidecar_for(video) {
        return read_transcript(&path.to_string_lossy(), "sidecar");
    }
    for idx in 0..4 {
        let cached = cache_path_for(video, idx);
        if cached.is_file() {
            return read_transcript(&cached.to_string_lossy(), "embedded");
        }
    }
    bail!(
        "no readable subtitles found for {}. Place a .srt/.vtt sidecar next to it \
         or run `unflick subtitle generate` first.",
        video
    )
}

/// Load the transcript for `file` if supplied (using the live player when
/// `file` matches what is currently playing), or for whatever is playing.
pub fn load_or_current(player: &Player, file: Option<&str>) -> Result<Transcript> {
    match file.map(str::trim).filter(|f| !f.is_empty()) {
        Some(f) => {
            if player.status().file.as_deref() == Some(f) {
                load_current(player)
            } else {
                load_for_file(f)
            }
        }
        None => load_current(player),
    }
}

fn read_transcript(path: &str, origin: &str) -> Result<Transcript> {
    let raw = read_text_lossy(Path::new(path))?;
    let mut cues = parse(&raw);
    if cues.is_empty() {
        bail!("subtitle file has no readable cues: {}", path);
    }
    apply_speaker_overrides(path, &mut cues);
    Ok(Transcript {
        source: path.to_string(),
        origin: origin.to_string(),
        cues,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SpeakerOverrides {
    #[serde(default)]
    cue_speakers: std::collections::HashMap<usize, String>,
    #[serde(default)]
    renames: std::collections::HashMap<String, String>,
}

fn speaker_override_path_for(source_path: &str) -> PathBuf {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in source_path.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let base = std::env::var_os("UNFLICK_DATA_DIR")
        .map(PathBuf::from)
        .or_else(dirs_next::cache_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("unflick")
        .join("transcripts");
    base.join(format!("{:016x}-speakers.json", hash))
}

fn load_speaker_overrides(source_path: &str) -> SpeakerOverrides {
    let p = speaker_override_path_for(source_path);
    std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_speaker_overrides(source_path: &str, overrides: &SpeakerOverrides) -> Result<()> {
    let p = speaker_override_path_for(source_path);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(overrides)?;
    std::fs::write(&p, json)?;
    Ok(())
}

fn apply_speaker_overrides(source_path: &str, cues: &mut [Cue]) {
    let overrides = load_speaker_overrides(source_path);
    if overrides.cue_speakers.is_empty() && overrides.renames.is_empty() {
        return;
    }
    for cue in cues.iter_mut() {
        if cue.speaker.is_none() {
            if let Some(assigned) = overrides.cue_speakers.get(&cue.index) {
                cue.speaker = Some(assigned.clone());
            }
        }
        if let Some(ref cur) = cue.speaker {
            if let Some(renamed) = overrides.renames.get(cur) {
                cue.speaker = Some(renamed.clone());
            }
        }
    }
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
    search_filtered(cues, query, None, limit)
}

/// Search over cue text with an optional case-insensitive speaker filter.
pub fn search_filtered<'a>(
    cues: &'a [Cue],
    query: &str,
    speaker: Option<&str>,
    limit: usize,
) -> Vec<&'a Cue> {
    let needle = query.trim().to_lowercase();
    let spk_filter = speaker.map(str::trim).filter(|s| !s.is_empty()).map(|s| s.to_lowercase());
    if needle.is_empty() && spk_filter.is_none() {
        return Vec::new();
    }
    cues.iter()
        .filter(|c| {
            if let Some(ref wanted) = spk_filter {
                let matches_spk = c
                    .speaker
                    .as_ref()
                    .map(|s| s.to_lowercase().contains(wanted))
                    .unwrap_or(false);
                if !matches_spk {
                    return false;
                }
            }
            needle.is_empty() || c.text.to_lowercase().contains(&needle)
        })
        .take(limit)
        .collect()
}

/// Filter cues by an optional speaker name (case-insensitive substring match).
pub fn filter_by_speaker(cues: &[Cue], speaker: Option<&str>) -> Vec<Cue> {
    let Some(wanted) = speaker.map(str::trim).filter(|s| !s.is_empty()) else {
        return cues.to_vec();
    };
    let wanted_lower = wanted.to_lowercase();
    cues.iter()
        .filter(|c| {
            c.speaker
                .as_ref()
                .map(|s| s.to_lowercase().contains(&wanted_lower))
                .unwrap_or(false)
        })
        .cloned()
        .collect()
}

/// Compute per-speaker statistics ordered by first appearance.
pub fn speaker_stats(cues: &[Cue]) -> Vec<SpeakerStat> {
    let mut order: Vec<String> = Vec::new();
    let mut map: std::collections::HashMap<String, SpeakerStat> = std::collections::HashMap::new();
    for cue in cues {
        let Some(ref name) = cue.speaker else {
            continue;
        };
        let entry = map.entry(name.clone()).or_insert_with(|| {
            order.push(name.clone());
            SpeakerStat {
                name: name.clone(),
                cue_count: 0,
                word_count: 0,
                speaking_seconds: 0.0,
                first_appearance: cue.start,
            }
        });
        entry.cue_count += 1;
        entry.word_count += count_words(&cue.text);
        entry.speaking_seconds += (cue.end - cue.start).max(0.0);
    }
    order
        .into_iter()
        .filter_map(|name| map.remove(&name))
        .collect()
}

/// Lightweight speaker diarization combining acoustic voice features (F0 pitch
/// & zero-crossing rate from 8kHz mono PCM when `media_path` is available)
/// with conversational turn heuristics (dialogue dashes, question-response
/// boundaries, and inter-cue pauses).
pub fn diarize(
    transcript: &mut Transcript,
    media_path: Option<&str>,
    target_speakers: Option<usize>,
) -> Result<Vec<SpeakerStat>> {
    if transcript.cues.is_empty() {
        return Ok(Vec::new());
    }
    let k = target_speakers.unwrap_or(2).clamp(2, 6);

    // Try extracting 8kHz mono PCM acoustic features if a media file is available.
    let acoustic = media_path
        .filter(|p| !is_text_sub(p) && Path::new(p).is_file())
        .and_then(|p| extract_cue_acoustic_features(p, &transcript.cues));

    let labels = if let Some(features) = acoustic.filter(|f| has_acoustic_variance(f)) {
        cluster_acoustic_turns(&transcript.cues, &features, k)
    } else {
        assign_heuristic_turns(&transcript.cues, k)
    };

    let mut overrides = load_speaker_overrides(&transcript.source);
    overrides.cue_speakers.clear();
    for (cue, spk_idx) in transcript.cues.iter_mut().zip(labels.into_iter()) {
        let base_name = format!("Speaker {}", spk_idx + 1);
        let final_name = overrides
            .renames
            .get(&base_name)
            .cloned()
            .unwrap_or(base_name.clone());
        overrides.cue_speakers.insert(cue.index, final_name.clone());
        cue.speaker = Some(final_name);
    }
    let _ = save_speaker_overrides(&transcript.source, &overrides);
    Ok(speaker_stats(&transcript.cues))
}

/// Rename a speaker across the transcript and persist the mapping.
pub fn rename_speaker(transcript: &mut Transcript, from: &str, to: &str) -> Result<usize> {
    let from_trimmed = from.trim();
    let to_trimmed = to.trim();
    if from_trimmed.is_empty() || to_trimmed.is_empty() {
        bail!("both `from` and `to` speaker names are required");
    }
    let from_lower = from_trimmed.to_lowercase();
    let mut updated = 0usize;
    let mut matched_exact = from_trimmed.to_string();

    for cue in transcript.cues.iter_mut() {
        if let Some(ref cur) = cue.speaker {
            if cur.to_lowercase() == from_lower {
                matched_exact = cur.clone();
                cue.speaker = Some(to_trimmed.to_string());
                updated += 1;
            }
        }
    }

    if updated == 0 {
        bail!(
            "speaker {:?} not found in transcript (run `unflick transcript speakers --detect` first)",
            from_trimmed
        );
    }

    let mut overrides = load_speaker_overrides(&transcript.source);
    for val in overrides.cue_speakers.values_mut() {
        if val.to_lowercase() == from_lower {
            *val = to_trimmed.to_string();
        }
    }
    for val in overrides.renames.values_mut() {
        if val.to_lowercase() == from_lower {
            *val = to_trimmed.to_string();
        }
    }
    overrides.renames.insert(matched_exact, to_trimmed.to_string());
    let _ = save_speaker_overrides(&transcript.source, &overrides);
    Ok(updated)
}

/// Format a transcript (optionally filtered by speaker) as `txt`, `srt`, `vtt`, or `json`.
pub fn export(
    transcript: &Transcript,
    format: &str,
    speaker: Option<&str>,
) -> Result<String> {
    let cues = filter_by_speaker(&transcript.cues, speaker);
    match format.trim().to_ascii_lowercase().as_str() {
        "txt" | "text" => {
            let mut out = String::new();
            for c in &cues {
                let ts = format_hms(c.start);
                if let Some(ref spk) = c.speaker {
                    out.push_str(&format!("[{}] {}: {}\n", ts, spk, c.text));
                } else {
                    out.push_str(&format!("[{}] {}\n", ts, c.text));
                }
            }
            Ok(out)
        }
        "srt" => {
            let mut out = String::new();
            for (i, c) in cues.iter().enumerate() {
                let start = format_srt_ts(c.start);
                let end = format_srt_ts(c.end);
                if let Some(ref spk) = c.speaker {
                    out.push_str(&format!("{}\n{} --> {}\n[{}]: {}\n\n", i + 1, start, end, spk, c.text));
                } else {
                    out.push_str(&format!("{}\n{} --> {}\n{}\n\n", i + 1, start, end, c.text));
                }
            }
            Ok(out)
        }
        "vtt" | "webvtt" => {
            let mut out = String::from("WEBVTT\n\n");
            for c in &cues {
                let start = format_srt_ts(c.start).replace(',', ".");
                let end = format_srt_ts(c.end).replace(',', ".");
                if let Some(ref spk) = c.speaker {
                    out.push_str(&format!("{} --> {}\n<v {}>{}\n\n", start, end, spk, c.text));
                } else {
                    out.push_str(&format!("{} --> {}\n{}\n\n", start, end, c.text));
                }
            }
            Ok(out)
        }
        "json" => {
            let exported = serde_json::json!({
                "source": transcript.source,
                "origin": transcript.origin,
                "speakers": speaker_stats(&cues),
                "cues": cues,
            });
            Ok(serde_json::to_string_pretty(&exported)?)
        }
        other => bail!(
            "unsupported export format {:?} (expected txt | srt | vtt | json)",
            other
        ),
    }
}

fn format_hms(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{:02}:{:02}:{:02}", h, m, s)
    } else {
        format!("{:02}:{:02}", m, s)
    }
}

fn format_srt_ts(seconds: f64) -> String {
    let total_ms = (seconds.max(0.0) * 1000.0).round() as u64;
    let h = total_ms / 3_600_000;
    let m = (total_ms % 3_600_000) / 60_000;
    let s = (total_ms % 60_000) / 1000;
    let ms = total_ms % 1000;
    format!("{:02}:{:02}:{:02},{:03}", h, m, s, ms)
}

#[derive(Clone, Copy)]
struct CueAcoustic {
    zcr: f64,
    tilt: f64,
}

fn extract_cue_acoustic_features(media_path: &str, cues: &[Cue]) -> Option<Vec<CueAcoustic>> {
    let ffmpeg = super::player::find_ffmpeg()?;
    let max_end = cues.last()?.end.min(1800.0);
    if max_end <= 0.0 {
        return None;
    }

    let mut cmd = std::process::Command::new(&ffmpeg);
    cmd.args([
        "-loglevel",
        "error",
        "-t",
        &format!("{:.1}", max_end + 1.0),
        "-i",
        media_path,
        "-vn",
        "-ac",
        "1",
        "-ar",
        "8000",
        "-f",
        "s16le",
        "-",
    ]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let output = cmd.output().ok()?;
    if !output.status.success() || output.stdout.len() < 1600 {
        return None;
    }
    let samples: Vec<i16> = output
        .stdout
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();

    let mut out = Vec::with_capacity(cues.len());
    for c in cues {
        let s_idx = ((c.start * 8000.0) as usize).min(samples.len());
        let e_idx = ((c.end * 8000.0) as usize).min(samples.len());
        if e_idx <= s_idx + 80 {
            out.push(CueAcoustic { zcr: 0.0, tilt: 0.0 });
            continue;
        }
        let slice = &samples[s_idx..e_idx];
        let mut crossings = 0usize;
        let mut energy = 0.0f64;
        let mut diff_energy = 0.0f64;
        for w in slice.windows(2) {
            let a = w[0] as f64;
            let b = w[1] as f64;
            if (a >= 0.0 && b < 0.0) || (a < 0.0 && b >= 0.0) {
                crossings += 1;
            }
            energy += a * a;
            let d = b - a;
            diff_energy += d * d;
        }
        let zcr = crossings as f64 / slice.len() as f64;
        let tilt = if energy > 1.0 {
            (diff_energy / energy).sqrt()
        } else {
            0.0
        };
        out.push(CueAcoustic { zcr, tilt });
    }
    Some(out)
}

fn has_acoustic_variance(features: &[CueAcoustic]) -> bool {
    if features.len() < 2 {
        return false;
    }
    let mean_zcr = features.iter().map(|f| f.zcr).sum::<f64>() / features.len() as f64;
    let var_zcr = features
        .iter()
        .map(|f| (f.zcr - mean_zcr).powi(2))
        .sum::<f64>()
        / features.len() as f64;
    var_zcr.sqrt() > 0.015
}

fn cluster_acoustic_turns(cues: &[Cue], features: &[CueAcoustic], k: usize) -> Vec<usize> {
    let n = cues.len();
    let k = k.min(n).max(1);
    let mut sorted_idx: Vec<usize> = (0..n).collect();
    sorted_idx.sort_by(|&a, &b| {
        features[a]
            .zcr
            .partial_cmp(&features[b].zcr)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut centroids: Vec<(f64, f64)> = (0..k)
        .map(|i| {
            let pick = sorted_idx[(i * n + n / (2 * k)).min(n - 1)];
            (features[pick].zcr, features[pick].tilt)
        })
        .collect();

    let mut assignments = vec![0usize; n];
    for _ in 0..10 {
        for i in 0..n {
            let mut best = 0;
            let mut best_dist = f64::MAX;
            for (c_idx, &(cz, ct)) in centroids.iter().enumerate() {
                let dz = features[i].zcr - cz;
                let dt = (features[i].tilt - ct) * 0.25;
                let dist = dz * dz + dt * dt;
                if dist < best_dist {
                    best_dist = dist;
                    best = c_idx;
                }
            }
            assignments[i] = best;
        }
        for (c_idx, centroid) in centroids.iter_mut().enumerate() {
            let mut sum_z = 0.0;
            let mut sum_t = 0.0;
            let mut cnt = 0usize;
            for i in 0..n {
                if assignments[i] == c_idx {
                    sum_z += features[i].zcr;
                    sum_t += features[i].tilt;
                    cnt += 1;
                }
            }
            if cnt > 0 {
                *centroid = (sum_z / cnt as f64, sum_t / cnt as f64);
            }
        }
    }

    normalize_speaker_order(&assignments, k)
}

fn assign_heuristic_turns(cues: &[Cue], k: usize) -> Vec<usize> {
    let n = cues.len();
    if n == 0 {
        return Vec::new();
    }
    let k = k.min(n).max(1);
    if k == 1 || n == 1 {
        return vec![0; n];
    }

    let mut gaps: Vec<f64> = cues
        .windows(2)
        .map(|w| (w[1].start - w[0].end).max(0.0))
        .collect();
    gaps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median_gap = gaps.get(gaps.len() / 2).copied().unwrap_or(0.5);
    let pause_threshold = (median_gap * 1.25).max(0.6);

    let mut assignments = Vec::with_capacity(n);
    let mut current = 0usize;
    assignments.push(current);

    for i in 1..n {
        let gap = (cues[i].start - cues[i - 1].end).max(0.0);
        let prev_question = cues[i - 1].text.trim_end().ends_with('?')
            || cues[i - 1].text.trim_end().ends_with('？');
        let starts_with_dash = cues[i].text.trim_start().starts_with('-');
        if starts_with_dash || prev_question || gap >= pause_threshold {
            current = (current + 1) % k;
        }
        assignments.push(current);
    }

    // Ensure at least two speakers appear if pauses were completely uniform.
    if assignments.iter().all(|&a| a == 0) && n >= 2 {
        for (i, a) in assignments.iter_mut().enumerate() {
            *a = i % k;
        }
    }

    normalize_speaker_order(&assignments, k)
}

fn normalize_speaker_order(raw: &[usize], k: usize) -> Vec<usize> {
    let mut map: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    let mut next_id = 0usize;
    raw.iter()
        .map(|&r| {
            *map.entry(r).or_insert_with(|| {
                let id = next_id.min(k.saturating_sub(1));
                next_id += 1;
                id
            })
        })
        .collect()
}

/// Search across every media entry in the library that has a readable
/// sidecar or cached subtitle file.
pub fn search_library(
    entries: &[crate::db::MediaEntry],
    query: &str,
    limit: usize,
) -> Vec<LibraryTranscriptMatch> {
    let needle = query.trim();
    if needle.is_empty() || limit == 0 {
        return Vec::new();
    }

    let mut out = Vec::new();
    for entry in entries {
        if out.len() >= limit {
            break;
        }
        let Ok(transcript) = load_for_file(&entry.path) else {
            continue;
        };
        let remaining = limit - out.len();
        for cue in search(&transcript.cues, needle, remaining) {
            out.push(LibraryTranscriptMatch {
                media_id: entry.id,
                path: entry.path.clone(),
                title: entry.title.clone(),
                source: transcript.source.clone(),
                origin: transcript.origin.clone(),
                cue: cue.clone(),
            });
        }
    }
    out
}

// ─── Structured summary ───────────────────────────────────────────────────

const STOPWORDS: &[&str] = &[
    "the", "and", "that", "this", "with", "from", "have", "for", "not", "are",
    "was", "were", "but", "you", "what", "they", "your", "will", "can", "all",
    "there", "their", "been", "would", "could", "should", "about", "into",
    "just", "like", "more", "when", "where", "who", "how", "why", "out", "one",
    "our", "his", "her", "its", "has", "had", "don", "didn", "doesn", "isn",
    "aren", "won", "can't", "don't", "it's", "that's", "we're", "you're",
    "they're", "some", "very", "then", "than", "them", "these", "those",
    "here", "only", "also", "back", "well", "even", "now", "way", "because",
];

fn count_words(text: &str) -> usize {
    let ascii_words = text
        .split(|c: char| c.is_whitespace() || c.is_ascii_punctuation())
        .filter(|w| !w.is_empty() && w.chars().any(|ch| ch.is_ascii_alphanumeric()))
        .count();
    let cjk_chars = text
        .chars()
        .filter(|&c| {
            matches!(
                c as u32,
                0x4E00..=0x9FFF | 0x3040..=0x30FF | 0xAC00..=0xD7AF
            )
        })
        .count();
    ascii_words + cjk_chars
}

fn extract_keywords(cues: &[Cue], max_keywords: usize) -> Vec<String> {
    use std::collections::HashMap;
    let mut counts: HashMap<String, usize> = HashMap::new();
    for cue in cues {
        for raw in cue.text.split(|c: char| !c.is_alphanumeric() && c != '\'') {
            let word = raw.trim_matches('\'').to_lowercase();
            let char_len = word.chars().count();
            if char_len < 3 {
                continue;
            }
            if word.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            if STOPWORDS.contains(&word.as_str()) {
                continue;
            }
            *counts.entry(word).or_insert(0) += 1;
        }
    }
    let mut pairs: Vec<(String, usize)> = counts.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    pairs.into_iter().take(max_keywords).map(|(w, _)| w).collect()
}

/// Build a structured summary of a transcript with word counts, speaking
/// duration, top keywords, and timeline segments.
pub fn summarize(transcript: &Transcript, segment_count: usize, duration: f64) -> TranscriptSummary {
    let cue_count = transcript.cues.len();
    let word_count: usize = transcript.cues.iter().map(|c| count_words(&c.text)).sum();
    let speaking_seconds: f64 = transcript
        .cues
        .iter()
        .map(|c| (c.end - c.start).max(0.0))
        .sum();
    let total_duration = if duration > 0.0 {
        duration
    } else {
        transcript.cues.last().map(|c| c.end).unwrap_or(0.0)
    };
    let keywords = extract_keywords(&transcript.cues, 8);

    let target = segment_count.clamp(1, 20);
    let mut raw_chapters = if target >= 2 {
        derive_chapters(&transcript.cues, target, total_duration)
    } else {
        Vec::new()
    };
    if raw_chapters.is_empty() && !transcript.cues.is_empty() {
        raw_chapters.push((0.0, title_from(&transcript.cues[0].text)));
    }

    let mut segments = Vec::with_capacity(raw_chapters.len());
    for (idx, (start, title)) in raw_chapters.iter().enumerate() {
        let end = raw_chapters
            .get(idx + 1)
            .map(|(next_start, _)| *next_start)
            .unwrap_or(total_duration.max(*start));
        let seg_cues: Vec<&Cue> = transcript
            .cues
            .iter()
            .filter(|c| {
                c.start >= *start
                    && (idx + 1 == raw_chapters.len() || c.start < end)
            })
            .collect();
        let preview_raw = seg_cues
            .iter()
            .take(3)
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let preview = if preview_raw.chars().count() > 160 {
            preview_raw.chars().take(157).collect::<String>() + "…"
        } else {
            preview_raw
        };
        segments.push(TranscriptSegment {
            index: idx,
            start: *start,
            end,
            title: title.clone(),
            cue_count: seg_cues.len(),
            preview,
        });
    }

    TranscriptSummary {
        source: transcript.source.clone(),
        origin: transcript.origin.clone(),
        cue_count,
        word_count,
        duration: total_duration,
        speaking_seconds,
        keywords,
        speakers: speaker_stats(&transcript.cues),
        segments,
    }
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

    #[test]
    fn summarize_extracts_keywords_stats_and_segments() {
        let srt = "1\n00:00:01,000 --> 00:00:05,000\nWelcome to the quantum computing lecture\n\n\
                   2\n00:00:06,000 --> 00:00:10,000\nQuantum bits superposition and quantum entanglement\n\n\
                   3\n00:01:00,000 --> 00:01:04,000\nError correction in superconducting circuits\n";
        let transcript = Transcript {
            source: "lecture.srt".into(),
            origin: "sidecar".into(),
            cues: parse(srt),
        };
        let summary = summarize(&transcript, 2, 70.0);
        assert_eq!(summary.cue_count, 3);
        assert!(summary.word_count >= 16);
        assert!((summary.speaking_seconds - 12.0).abs() < 1e-6);
        assert_eq!(summary.duration, 70.0);
        assert_eq!(summary.keywords.first().map(|s| s.as_str()), Some("quantum"));
        assert!(!summary.segments.is_empty());
        assert_eq!(summary.segments[0].start, 0.0);
    }

    #[test]
    fn search_library_scans_sidecar_subtitles_across_entries() {
        let dir = std::env::temp_dir().join(format!(
            "unflick-lib-transcript-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let vid_a = dir.join("alpha.mp4");
        let srt_a = dir.join("alpha.srt");
        let vid_b = dir.join("beta.mp4");
        let srt_b = dir.join("beta.en.srt");
        std::fs::write(&vid_a, b"fake").unwrap();
        std::fs::write(
            &srt_a,
            "1\n00:00:02,000 --> 00:00:04,000\nThe secret passphrase is avocado\n",
        )
        .unwrap();
        std::fs::write(&vid_b, b"fake").unwrap();
        std::fs::write(
            &srt_b,
            "1\n00:00:12,500 --> 00:00:15,000\nRemember the avocado toast recipe\n",
        )
        .unwrap();

        let entries = vec![
            crate::db::MediaEntry {
                id: 1,
                key: vid_a.to_string_lossy().into_owned(),
                path: vid_a.to_string_lossy().into_owned(),
                title: "Alpha".into(),
                duration: Some(30.0),
                width: None,
                height: None,
                video_codec: None,
                audio_codec: None,
                file_size: None,
                added_at: String::new(),
                last_played: None,
                play_count: 0,
            },
            crate::db::MediaEntry {
                id: 2,
                key: vid_b.to_string_lossy().into_owned(),
                path: vid_b.to_string_lossy().into_owned(),
                title: "Beta".into(),
                duration: Some(60.0),
                width: None,
                height: None,
                video_codec: None,
                audio_codec: None,
                file_size: None,
                added_at: String::new(),
                last_played: None,
                play_count: 0,
            },
        ];

        let hits = search_library(&entries, "avocado", 10);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].title, "Alpha");
        assert_eq!(hits[0].cue.start, 2.0);
        assert_eq!(hits[1].title, "Beta");
        assert_eq!(hits[1].cue.start, 12.5);

        let limited = search_library(&entries, "avocado", 1);
        assert_eq!(limited.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_ass_dialogue_with_actors_and_strips_overrides() {
        let ass = "[Script Info]\nTitle: Demo\n\n[Events]\n\
                   Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
                   Dialogue: 0,0:00:01.00,0:00:03.50,Default,Alice,0,0,0,,{\\an8\\pos(100,200)}Hello, world!\\NHow are you?\n\
                   Dialogue: 0,0:00:04.00,0:00:06.00,Default,Bob,0,0,0,,I am doing great.\n\
                   Dialogue: 0,0:00:07.00,0:00:08.00,Default,,0,0,0,,{\\p1}m 0 0 l 100 100{\\p0}\n";
        let cues = parse(ass);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].speaker.as_deref(), Some("Alice"));
        assert_eq!(cues[0].text, "Hello, world! How are you?");
        assert_eq!(cues[0].start, 1.0);
        assert_eq!(cues[0].end, 3.5);
        assert_eq!(cues[1].speaker.as_deref(), Some("Bob"));
        assert_eq!(cues[1].text, "I am doing great.");
    }

    #[test]
    fn parses_vtt_voice_tags_and_inline_speakers_and_tinydiarize() {
        let vtt = "WEBVTT\n\n\
                   00:01.000 --> 00:03.000\n<v.loud Host>Welcome back everyone</v>\n\n\
                   00:04.000 --> 00:06.000\n[Guest 1]: Thanks for having me\n\n\
                   00:07.000 --> 00:09.000\n[SPEAKER_TURN] Let us dive in\n";
        let cues = parse(vtt);
        assert_eq!(cues.len(), 3);
        assert_eq!(cues[0].speaker.as_deref(), Some("Host"));
        assert_eq!(cues[0].text, "Welcome back everyone");
        assert_eq!(cues[1].speaker.as_deref(), Some("Guest 1"));
        assert_eq!(cues[1].text, "Thanks for having me");
        assert_eq!(cues[2].speaker.as_deref(), Some("Speaker 1"));
        assert_eq!(cues[2].text, "Let us dive in");
    }

    #[test]
    fn diarize_rename_filter_and_export_roundtrip() {
        let srt = "1\n00:00:01,000 --> 00:00:03,000\nWhat is the refund policy?\n\n\
                   2\n00:00:05,000 --> 00:00:08,000\n- You can return any item within thirty days.\n\n\
                   3\n00:00:12,000 --> 00:00:15,000\nDoes that include digital downloads?\n\n\
                   4\n00:00:17,000 --> 00:00:20,000\n- Yes, if it was not downloaded yet.\n";
        let mut transcript = Transcript {
            source: format!(
                "{}/unflick-diarize-test-{}.srt",
                std::env::temp_dir().to_string_lossy(),
                std::process::id()
            ),
            origin: "sidecar".into(),
            cues: parse(srt),
        };
        let stats = diarize(&mut transcript, None, Some(2)).unwrap();
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].name, "Speaker 1");
        assert_eq!(stats[1].name, "Speaker 2");

        let count = rename_speaker(&mut transcript, "Speaker 1", "Customer").unwrap();
        assert_eq!(count, 2);
        assert_eq!(transcript.cues[0].speaker.as_deref(), Some("Customer"));

        let hits = search_filtered(&transcript.cues, "refund", Some("Customer"), 10);
        assert_eq!(hits.len(), 1);
        let miss = search_filtered(&transcript.cues, "refund", Some("Speaker 2"), 10);
        assert!(miss.is_empty());

        let exported_txt = export(&transcript, "txt", Some("Customer")).unwrap();
        assert!(exported_txt.contains("Customer: What is the refund policy?"));
        assert!(!exported_txt.contains("thirty days"));

        let _ = std::fs::remove_file(speaker_override_path_for(&transcript.source));
    }
}
