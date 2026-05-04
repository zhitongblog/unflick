//! Post-play hooks for streaming URLs.
//!
//! After a URL has been resolved by yt-dlp and handed to mpv, there are two
//! background tasks we want to fire and forget:
//!
//!   1. SponsorBlock fetch — pull community-curated skip segments, attach
//!      them to the player. The auto-skip polling task picks them up.
//!   2. yt-dlp subtitle download — write external subtitle files to a
//!      temp dir and load each one via mpv `sub-add`.
//!
//! Neither should block playback start. They `tokio::spawn` and log on
//! failure.
//!
//! This module is the wire-up point that the URL-aware play handler in
//! `cli/commands.rs` (P0 territory) and `gui/commands.rs` (P0+P1) will
//! call. The function is async-but-fast: it spawns and returns.

use std::sync::Arc;

use serde_json::Value;

use super::player::Player;
use super::sponsorblock::{self, Segment};

/// Settings snapshot consumed by the post-play hooks. Pulled from the
/// settings JSON via `read_settings_snapshot()` so callers don't have to
/// hand-pluck the keys.
#[derive(Debug, Clone)]
pub struct UrlPostPlaySettings {
    pub sponsorblock_enabled: bool,
    pub sponsorblock_categories: Vec<String>,
    pub auto_download_subtitles: bool,
    pub subtitle_languages: Vec<String>,
}

impl Default for UrlPostPlaySettings {
    fn default() -> Self {
        Self {
            sponsorblock_enabled: true,
            sponsorblock_categories: vec![
                "sponsor".into(),
                "selfpromo".into(),
                "intro".into(),
                "outro".into(),
                "interaction".into(),
            ],
            auto_download_subtitles: true,
            subtitle_languages: vec!["en".into(), "zh-CN".into()],
        }
    }
}

/// Read the post-play settings from the persisted settings.json. Falls back
/// to defaults for any missing key. Never panics.
pub fn read_settings_snapshot() -> UrlPostPlaySettings {
    let mut out = UrlPostPlaySettings::default();
    let all = match super::settings::read_all() {
        Ok(v) => v,
        Err(_) => return out,
    };
    if let Some(v) = all.get("sponsorblock_enabled").and_then(Value::as_bool) {
        out.sponsorblock_enabled = v;
    }
    if let Some(arr) = all.get("sponsorblock_categories").and_then(Value::as_array) {
        let parsed: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        if !parsed.is_empty() {
            out.sponsorblock_categories = parsed;
        }
    }
    if let Some(v) = all
        .get("auto_download_subtitles")
        .and_then(Value::as_bool)
    {
        out.auto_download_subtitles = v;
    }
    if let Some(arr) = all.get("subtitle_languages").and_then(Value::as_array) {
        let parsed: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        if !parsed.is_empty() {
            out.subtitle_languages = parsed;
        }
    }
    out
}

/// Fire-and-forget post-play hooks for a streaming URL.
///
/// Contract: returns immediately after spawning background tasks. The
/// caller does *not* await actual fetch/download completion. Failures
/// are logged to stderr; nothing here propagates errors back to the
/// playback start path.
///
/// Caller responsibilities:
///   * Resolve the URL to a stream URL via yt-dlp (or equivalent).
///   * Hand the stream URL to mpv via `Player::play`.
///   * Then call this with the **original page URL** (not the stream URL)
///     and a clone of the settings snapshot.
///
/// Inside:
///   * If the URL is a YouTube link AND `sponsorblock_enabled` is true,
///     spawns a fetch and `enable_sponsorblock`s the result.
///   * If `auto_download_subtitles` is true, spawns a yt-dlp call and
///     loads each downloaded `.srt` via mpv `sub-add`.
pub fn after_play_url_hooks(
    player: Arc<Player>,
    url: String,
    yt_dlp_path: Option<std::path::PathBuf>,
    settings: UrlPostPlaySettings,
) {
    // ── SponsorBlock ────────────────────────────────────────────────
    if settings.sponsorblock_enabled {
        if let Some(youtube_id) = sponsorblock::extract_youtube_id(&url) {
            let player_for_sb = Arc::clone(&player);
            let categories = settings.sponsorblock_categories.clone();
            tokio::spawn(async move {
                let cats: Vec<&str> = categories.iter().map(String::as_str).collect();
                match sponsorblock::fetch_segments(&youtube_id, &cats).await {
                    Ok(segments) => {
                        eprintln!(
                            "[sponsorblock] fetched {} segment(s) for {}",
                            segments.len(),
                            youtube_id
                        );
                        player_for_sb.enable_sponsorblock(segments);
                    }
                    Err(e) => {
                        eprintln!("[sponsorblock] fetch failed for {}: {}", youtube_id, e);
                    }
                }
            });
        }
    }

    // ── yt-dlp subtitle download ────────────────────────────────────
    if settings.auto_download_subtitles {
        let langs = settings.subtitle_languages.clone();
        if !langs.is_empty() {
            let player_for_subs = Arc::clone(&player);
            let url_for_subs = url.clone();
            let yt_dlp_for_subs = yt_dlp_path;
            tokio::spawn(async move {
                let yt_dlp = match yt_dlp_for_subs {
                    Some(p) => p,
                    None => {
                        eprintln!("[auto-subs] no yt-dlp path provided; skipping");
                        return;
                    }
                };
                if let Err(e) = download_and_attach_subtitles(
                    &player_for_subs,
                    &yt_dlp,
                    &url_for_subs,
                    &langs,
                )
                .await
                {
                    eprintln!("[auto-subs] failed: {}", e);
                }
            });
        }
    }
}

/// Run yt-dlp to fetch subtitles into a unique temp dir, then load each
/// resulting `.srt` via mpv's `sub-add`. yt-dlp is sync so we run it on
/// the blocking pool.
async fn download_and_attach_subtitles(
    player: &Arc<Player>,
    yt_dlp: &std::path::Path,
    url: &str,
    languages: &[String],
) -> anyhow::Result<()> {
    let lang_csv = languages.join(",");
    // Unique temp dir per invocation so concurrent calls don't collide.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let temp_dir = std::env::temp_dir().join(format!("unflick-subs-{}", stamp));
    std::fs::create_dir_all(&temp_dir)?;

    let yt_dlp_owned = yt_dlp.to_path_buf();
    let url_owned = url.to_string();
    let temp_dir_owned = temp_dir.clone();

    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let mut cmd = std::process::Command::new(&yt_dlp_owned);
        cmd.args([
            "--skip-download",
            "--write-auto-sub",
            "--write-sub",
            "--sub-langs",
            &lang_csv,
            "--convert-subs",
            "srt",
            "--output",
            &format!(
                "{}/%(id)s.%(ext)s",
                temp_dir_owned.to_string_lossy()
            ),
            &url_owned,
        ]);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let output = cmd.output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "yt-dlp exited {}: {}",
                output.status,
                stderr.chars().take(400).collect::<String>()
            );
        }
        Ok(())
    })
    .await
    .map_err(|e| anyhow::anyhow!("blocking task join: {}", e))??;

    // Scan for any .srt files yt-dlp wrote and attach them.
    let mut attached = 0usize;
    if let Ok(entries) = std::fs::read_dir(&temp_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("srt") {
                if let Some(p) = path.to_str() {
                    match player.subtitle_load(p) {
                        Ok(()) => {
                            attached += 1;
                            eprintln!("[auto-subs] attached {}", p);
                        }
                        Err(e) => {
                            eprintln!("[auto-subs] sub-add failed for {}: {}", p, e);
                        }
                    }
                }
            }
        }
    }
    if attached == 0 {
        eprintln!(
            "[auto-subs] yt-dlp succeeded but no .srt files were written for {:?}",
            languages
        );
    }
    Ok(())
}

/// Just the segment-fetch half of the post-play work. Useful for the CLI
/// `sponsor list` command which only wants segments, not subtitle downloads.
pub async fn fetch_segments_for_url(
    url: &str,
    categories: &[String],
) -> anyhow::Result<Vec<Segment>> {
    let id = sponsorblock::extract_youtube_id(url)
        .ok_or_else(|| anyhow::anyhow!("not a recognised YouTube URL: {}", url))?;
    let cats: Vec<&str> = categories.iter().map(String::as_str).collect();
    sponsorblock::fetch_segments(&id, &cats).await
}

/// Spawn the SponsorBlock auto-skip polling task. Polls `playback-time` on
/// the given player every 250 ms and seeks past any active sponsor segment.
///
/// Safe to call once at startup. The task lives for the duration of the
/// tokio runtime; it holds an `Arc<Player>`, so the player is kept alive
/// for as long as the task runs.
pub fn spawn_sponsor_skip_task(player: Arc<Player>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
        // We poll regardless of playback state; mpv returns an error for
        // time-pos when nothing is loaded — that just means "nothing to
        // do" so we silently skip.
        loop {
            interval.tick().await;
            // Read the current playback position. If it fails (no file
            // loaded, paused without a position yet, etc.) skip this tick.
            let time = match player
                .mpv_handle()
                .get_property_f64("playback-time")
                .or_else(|_| player.mpv_handle().get_property_f64("time-pos"))
            {
                Ok(t) => t,
                Err(_) => continue,
            };
            if let Some(end) = player.check_sponsor_skip(time) {
                eprintln!(
                    "[sponsorblock] auto-skip: {:.2}s -> {:.2}s",
                    time, end
                );
                let _ = player.seek(end);
            }
        }
    });
}
