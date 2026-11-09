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
/// duration, top keywords, and timeline segments with representative lines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSummary {
    pub source: String,
    pub origin: String,
    pub cue_count: usize,
    pub word_count: usize,
    pub duration: f64,
    pub speaking_seconds: f64,
    pub keywords: Vec<String>,
    pub segments: Vec<TranscriptSegment>,
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
}
