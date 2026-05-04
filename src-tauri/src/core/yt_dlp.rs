//! Shared yt-dlp invocation helper.
//!
//! Used by GUI (`extract_stream_url` Tauri command), CLI (`unflick play <url>`),
//! and MCP (`play` tool with an http(s) URL) so all three interfaces resolve
//! streaming URLs through the same code path.
//!
//! Responsibilities:
//! - Locate the yt-dlp executable (user-data > exe-relative > PATH)
//! - Spawn yt-dlp with a sensible format fallback chain
//! - Enforce a hard 60s timeout (kills the child on expiry)
//! - Allow callers to cancel an in-flight extraction by setting a global
//!   AtomicBool flag the wait loop observes
//! - Categorize stderr into a small set of kinds the UI/CLI can render
//!   into human-friendly messages
//!
//! The helper deliberately does **not** depend on Tauri (`AppHandle` and
//! friends) so the daemon process — which never holds an `AppHandle` — can
//! call it for CLI/MCP url playback.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

/// Hard upper bound on how long yt-dlp may run before we kill it.
const EXTRACT_TIMEOUT: Duration = Duration::from_secs(60);

/// yt-dlp format selector chain. yt-dlp evaluates `/`-separated options in
/// order and falls back to the next when the previous yields no match — so a
/// single `-f` value covers MP4 progressive, MP4 split-track, generic best,
/// and worst-as-last-resort. No client-side retry needed.
pub const FORMAT_CHAIN: &str =
    "best[ext=mp4][height<=1080]/bestvideo[ext=mp4]+bestaudio[ext=m4a]/best/worst";

/// Cooperative cancel flag. `cancel()` flips it to true; the in-flight
/// extraction polls it via `tokio::select!` and kills the child. The flag
/// is cleared at the start of every new extraction so a stale cancel from
/// a previous attempt never aborts a fresh one.
static CANCEL_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Categorized error reason for a failed extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Network,
    LoginRequired,
    GeoBlocked,
    Private,
    UnsupportedSite,
    Timeout,
    Cancelled,
    Unknown,
}

impl ErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::Network => "network",
            ErrorKind::LoginRequired => "login_required",
            ErrorKind::GeoBlocked => "geo_blocked",
            ErrorKind::Private => "private",
            ErrorKind::UnsupportedSite => "unsupported_site",
            ErrorKind::Timeout => "timeout",
            ErrorKind::Cancelled => "cancelled",
            ErrorKind::Unknown => "unknown",
        }
    }

    /// Short, user-facing message suitable for CLI output. The GUI layer
    /// can do its own i18n on top of `as_str()` if it wants nicer copy.
    pub fn human_message(self) -> &'static str {
        match self {
            ErrorKind::Network => "network error reaching the site (check your connection or proxy)",
            ErrorKind::LoginRequired => "this video requires login (try setting cookies-from-browser in settings)",
            ErrorKind::GeoBlocked => "this video is not available in your region",
            ErrorKind::Private => "this video is private",
            ErrorKind::UnsupportedSite => "this site is not supported by yt-dlp",
            ErrorKind::Timeout => "extraction timed out after 60s",
            ErrorKind::Cancelled => "extraction was cancelled",
            ErrorKind::Unknown => "yt-dlp failed (see error_message for details)",
        }
    }
}

/// Result of a stream-URL extraction. `stream_url` is empty on failure;
/// `error_kind`/`error_message` are present iff `stream_url` is empty.
/// Shape is shared between GUI, CLI, and MCP.
#[derive(Debug, Clone, Serialize)]
pub struct ExtractResult {
    pub stream_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

impl ExtractResult {
    pub fn ok(url: impl Into<String>) -> Self {
        Self { stream_url: url.into(), error_kind: None, error_message: None }
    }

    pub fn err(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            stream_url: String::new(),
            error_kind: Some(kind.as_str().to_string()),
            error_message: Some(message.into()),
        }
    }

    pub fn is_ok(&self) -> bool {
        !self.stream_url.is_empty()
    }
}

/// User-data path for the auto-updated yt-dlp copy (matches the GUI path).
fn yt_dlp_user_path() -> Option<PathBuf> {
    let dir = dirs_next::data_local_dir()?.join("unflick").join("bin");
    let _ = std::fs::create_dir_all(&dir);
    let name = if cfg!(target_os = "windows") { "yt-dlp.exe" } else { "yt-dlp" };
    Some(dir.join(name))
}

/// Locate yt-dlp without an `AppHandle`. Search order:
/// 1. User-data auto-updated copy (newest version)
/// 2. Sibling of the running executable (bundled by the installer)
/// 3. PATH
///
/// The GUI layer has its own `find_yt_dlp(&AppHandle)` that prefers the
/// Tauri resource directory; both eventually converge on the same
/// `~/AppData/Local/unflick/bin/yt-dlp.exe` once auto-update has run.
pub fn find_yt_dlp() -> Option<PathBuf> {
    if let Some(p) = yt_dlp_user_path() {
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for sub in ["yt-dlp", ""] {
                for name in ["yt-dlp.exe", "yt-dlp"] {
                    let p = if sub.is_empty() {
                        dir.join(name)
                    } else {
                        dir.join(sub).join(name)
                    };
                    if p.exists() {
                        return Some(p);
                    }
                }
            }
        }
    }
    let candidate = if cfg!(target_os = "windows") { "yt-dlp.exe" } else { "yt-dlp" };
    which::which(candidate).ok()
}

/// Categorize yt-dlp stderr into one of the well-known error kinds.
fn classify(stderr: &str) -> ErrorKind {
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("sign in to confirm")
        || lower.contains("login required")
        || lower.contains("members-only")
        || lower.contains("requires authentication")
    {
        return ErrorKind::LoginRequired;
    }
    if lower.contains("not available in your country")
        || lower.contains("geo-restricted")
        || lower.contains("geo restricted")
        || lower.contains("not available from your location")
    {
        return ErrorKind::GeoBlocked;
    }
    if lower.contains("private video") || lower.contains("video is private") {
        return ErrorKind::Private;
    }
    if lower.contains("unsupported url") {
        return ErrorKind::UnsupportedSite;
    }
    if lower.contains("connection")
        || lower.contains("failed to resolve")
        || lower.contains("timed out")
        || lower.contains("name or service not known")
        || lower.contains("network is unreachable")
    {
        return ErrorKind::Network;
    }
    ErrorKind::Unknown
}

/// Spawn yt-dlp and resolve a single direct stream URL. Honors the global
/// cancel flag and the 60s timeout.
///
/// `proxy` may be `None` (no proxy), `Some("")` (treated as no proxy), or a
/// proxy URL like `http://127.0.0.1:7890`.
///
/// `quality` may be `None` (use `FORMAT_CHAIN` default), `Some("audio_only")`
/// (audio-only, swaps `-f` for `-x --audio-format m4a`), or a `"<N>p"`
/// height cap like `"1080p"` (height-bounded format selector).
///
/// `cookies_browser` may be `None` (no `--cookies-from-browser`) or a
/// browser name like `"firefox"`. The browser must be closed during the
/// call (yt-dlp can't read a locked cookie DB).
pub async fn extract_stream_url(
    yt_dlp: &Path,
    url: &str,
    proxy: Option<&str>,
    quality: Option<&str>,
    cookies_browser: Option<&str>,
) -> ExtractResult {
    // Reset cancel from any previous extraction.
    CANCEL_REQUESTED.store(false, Ordering::SeqCst);

    let mut cmd = Command::new(yt_dlp);
    cmd.arg("--get-url")
        .arg("--no-playlist")
        .arg("--no-warnings");

    // Quality routing: audio_only takes the `-x` extraction path (no `-f`),
    // numeric heights use a height-capped fallback chain, otherwise we keep
    // the default FORMAT_CHAIN.
    let mut audio_only = false;
    let mut height_cap: Option<u32> = None;
    if let Some(q) = quality.map(str::trim).filter(|s| !s.is_empty()) {
        if q.eq_ignore_ascii_case("audio_only") {
            audio_only = true;
        } else if let Some(n) = q.strip_suffix('p').and_then(|n| n.parse::<u32>().ok()) {
            height_cap = Some(n);
        }
    }
    if audio_only {
        cmd.arg("-x").arg("--audio-format").arg("m4a");
    } else if let Some(n) = height_cap {
        // Mirror the FORMAT_CHAIN priority but with a height ceiling. yt-dlp
        // evaluates the slash-separated entries in order and picks the first
        // that resolves, so a missing tier degrades gracefully.
        let capped = format!(
            "best[ext=mp4][height<={n}]/bestvideo[ext=mp4][height<={n}]+bestaudio[ext=m4a]/best[height<={n}]/worst",
            n = n
        );
        cmd.arg("-f").arg(capped);
    } else {
        cmd.arg("-f").arg(FORMAT_CHAIN);
    }
    if let Some(b) = cookies_browser.map(str::trim).filter(|s| !s.is_empty()) {
        cmd.arg("--cookies-from-browser").arg(b);
    }
    if let Some(p) = proxy.map(str::trim).filter(|s| !s.is_empty()) {
        cmd.arg("--proxy").arg(p);
    }
    cmd.arg(url);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    // Suppress console window on Windows.
    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return ExtractResult::err(ErrorKind::Unknown, format!("yt-dlp failed to spawn: {}", e))
        }
    };

    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();

    // Drain stdout/stderr concurrently with the wait so the pipes never
    // block the child. Tasks complete once the child closes its end.
    let stdout_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        if let Some(p) = stdout_pipe.as_mut() {
            let _ = p.read_to_end(&mut buf).await;
        }
        buf
    });
    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        if let Some(p) = stderr_pipe.as_mut() {
            let _ = p.read_to_end(&mut buf).await;
        }
        buf
    });

    // Wait for the child while polling the cancel flag every 100ms and
    // capping the whole thing at 60s.
    let deadline = tokio::time::Instant::now() + EXTRACT_TIMEOUT;
    let mut termination: Option<ErrorKind> = None;
    let status = loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            termination = Some(ErrorKind::Timeout);
            break None;
        }
        let tick = std::cmp::min(deadline - now, Duration::from_millis(100));
        tokio::select! {
            res = child.wait() => break Some(res),
            _ = tokio::time::sleep(tick) => {
                if CANCEL_REQUESTED.load(Ordering::SeqCst) {
                    termination = Some(ErrorKind::Cancelled);
                    break None;
                }
            }
        }
    };

    if termination.is_some() {
        let _ = child.start_kill();
        let _ = child.wait().await;
    }

    // Collect output regardless of how the wait ended. The reader tasks
    // already had their pipes — closing the child causes them to finish.
    let stdout_buf = stdout_task.await.unwrap_or_default();
    let stderr_buf = stderr_task.await.unwrap_or_default();

    if let Some(kind) = termination {
        return ExtractResult::err(kind, kind.human_message());
    }
    let status = match status {
        Some(Ok(s)) => s,
        Some(Err(io_err)) => {
            return ExtractResult::err(
                ErrorKind::Unknown,
                format!("yt-dlp wait failed: {}", io_err),
            );
        }
        None => unreachable!("status only None when termination is Some"),
    };

    if !status.success() {
        let stderr = String::from_utf8_lossy(&stderr_buf).trim().to_string();
        let kind = if stderr.is_empty() { ErrorKind::Unknown } else { classify(&stderr) };
        let message = if stderr.is_empty() {
            kind.human_message().to_string()
        } else {
            stderr
        };
        return ExtractResult::err(kind, message);
    }

    let stdout = String::from_utf8_lossy(&stdout_buf);
    match stdout.lines().find(|l| !l.trim().is_empty()) {
        Some(line) => ExtractResult::ok(line.trim().to_string()),
        None => ExtractResult::err(ErrorKind::Unknown, "yt-dlp returned no URL"),
    }
}

/// Cancel a currently-running extraction. The in-flight wait loop polls
/// `CANCEL_REQUESTED` every 100ms and kills the child when set. Returns
/// `true` to indicate the request was registered (whether or not an
/// extraction is actually in flight — the next one would otherwise pick
/// up the stale flag, but `extract_stream_url` resets it on entry).
pub fn cancel() -> bool {
    CANCEL_REQUESTED.store(true, Ordering::SeqCst);
    true
}

/// True if an `http://` or `https://` URL (used by CLI/MCP to decide
/// whether to route through extraction).
pub fn is_http_url(s: &str) -> bool {
    let lower = s.trim_start().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}
