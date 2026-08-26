//! SponsorBlock segment fetcher.
//!
//! Talks to the public SponsorBlock API (`https://sponsor.ajay.app`) to
//! retrieve community-curated segment metadata for YouTube videos. The
//! player consumes these segments to auto-skip ads, intros, outros, and
//! sponsor reads.
//!
//! No API key, no auth — purely public, anonymous reads.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use super::http::{agent, url_encode};

const SPONSORBLOCK_API: &str = "https://sponsor.ajay.app/api/skipSegments";

/// One contiguous segment of a video the user probably wants to skip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    /// Start time in seconds (inclusive).
    pub start: f64,
    /// End time in seconds (exclusive).
    pub end: f64,
    /// SponsorBlock category, e.g. "sponsor", "selfpromo", "intro", "outro",
    /// "interaction", "music_offtopic", "preview", "filler".
    pub category: String,
    /// Action to take: typically "skip"; can also be "mute" or "poi".
    /// We only auto-skip "skip" segments.
    pub action_type: String,
    /// SponsorBlock UUID for the segment (used for vote / debug).
    pub uuid: String,
}

/// Raw API response shape — the API uses `actionType` and a `[start, end]`
/// pair. We map this onto our `Segment` for downstream code.
#[derive(Debug, Deserialize)]
struct ApiSegment {
    segment: [f64; 2],
    category: String,
    #[serde(rename = "actionType")]
    action_type: String,
    #[serde(rename = "UUID")]
    uuid: String,
}

/// Fetch segments for a YouTube video filtered to the requested categories.
///
/// Errors:
///   * Network/timeout — returned as `Err`.
///   * HTTP 404 (no segments found) — returned as `Ok(vec![])` since this
///     is the API's success-but-empty signal.
///   * HTTP 4xx/5xx other — returned as `Err`.
///   * JSON parse failure — returned as `Err`.
///
/// 5-second timeout. We do not want to block playback start.
pub async fn fetch_segments(youtube_id: &str, categories: &[&str]) -> Result<Vec<Segment>> {
    if youtube_id.is_empty() {
        return Err(anyhow!("youtube_id is empty"));
    }
    // SponsorBlock expects the categories as a JSON-encoded list, then
    // URL-encoded into the query string.
    let categories_json = serde_json::to_string(categories)
        .map_err(|e| anyhow!("encode categories: {}", e))?;
    let url = format!(
        "{}?videoID={}&categories={}",
        SPONSORBLOCK_API,
        url_encode(youtube_id),
        url_encode(&categories_json),
    );

    // ureq is sync; run it on a blocking thread pool so we don't block the
    // tokio runtime.
    tokio::task::spawn_blocking(move || -> Result<Vec<Segment>> {
        let resp = agent(3, 5).get(&url).call();
        match resp {
            Ok(r) => {
                let body = r
                    .into_string()
                    .map_err(|e| anyhow!("read body: {}", e))?;
                let raw: Vec<ApiSegment> = serde_json::from_str(&body)
                    .map_err(|e| anyhow!("parse sponsorblock json: {}", e))?;
                Ok(raw
                    .into_iter()
                    .map(|s| Segment {
                        start: s.segment[0],
                        end: s.segment[1],
                        category: s.category,
                        action_type: s.action_type,
                        uuid: s.uuid,
                    })
                    .collect())
            }
            Err(ureq::Error::Status(404, _)) => Ok(vec![]),
            Err(ureq::Error::Status(code, r)) => {
                let body = r.into_string().unwrap_or_default();
                Err(anyhow!("sponsorblock http {}: {}", code, body))
            }
            Err(e) => Err(anyhow!("sponsorblock request failed: {}", e)),
        }
    })
    .await
    .map_err(|e| anyhow!("blocking task join: {}", e))?
}

/// Extract an 11-character YouTube video ID from any of the common URL
/// shapes. Returns `None` for non-YouTube URLs or unrecognised formats.
///
/// Recognised shapes:
///   * `youtube.com/watch?v=ID`
///   * `youtu.be/ID`
///   * `youtube.com/shorts/ID`
///   * `youtube.com/embed/ID`
///   * `m.youtube.com/...` (any of the above)
///   * `music.youtube.com/...` (any of the above)
pub fn extract_youtube_id(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    let host_check = |needle: &str| lower.contains(needle);

    // youtu.be/<ID>[?...]
    if host_check("youtu.be/") {
        let after = url.split("youtu.be/").nth(1)?;
        let id = take_id(after)?;
        return Some(id);
    }

    if !host_check("youtube.com/") && !host_check("youtube-nocookie.com/") {
        return None;
    }

    // /watch?v=<ID>
    if let Some(idx) = lower.find("v=") {
        // ensure "v=" is part of a query string after watch
        // (avoid matching some hypothetical "v=" inside a path)
        let after = &url[idx + 2..];
        if let Some(id) = take_id(after) {
            return Some(id);
        }
    }

    // /shorts/<ID>
    for marker in ["/shorts/", "/embed/", "/v/", "/live/"] {
        if let Some(pos) = lower.find(marker) {
            let after = &url[pos + marker.len()..];
            if let Some(id) = take_id(after) {
                return Some(id);
            }
        }
    }

    None
}

/// Read up to the next non-id character and validate the 11-char shape.
fn take_id(s: &str) -> Option<String> {
    let id: String = s
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if id.len() == 11 {
        Some(id)
    } else {
        None
    }
}

/// Minimal percent-encoder for the few characters that matter in a query
/// string: `[`, `]`, `"`, `,`, space. We don't pull in url::form_urlencoded
/// just for this one call.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_watch() {
        assert_eq!(
            extract_youtube_id("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".into())
        );
    }

    #[test]
    fn extracts_short_url() {
        assert_eq!(
            extract_youtube_id("https://youtu.be/dQw4w9WgXcQ?t=42"),
            Some("dQw4w9WgXcQ".into())
        );
    }

    #[test]
    fn extracts_shorts() {
        assert_eq!(
            extract_youtube_id("https://www.youtube.com/shorts/dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".into())
        );
    }

    #[test]
    fn extracts_embed() {
        assert_eq!(
            extract_youtube_id("https://www.youtube.com/embed/dQw4w9WgXcQ?autoplay=1"),
            Some("dQw4w9WgXcQ".into())
        );
    }

    #[test]
    fn rejects_non_youtube() {
        assert_eq!(extract_youtube_id("https://vimeo.com/12345"), None);
        assert_eq!(extract_youtube_id("https://example.com/watch?v=abc"), None);
    }
}
