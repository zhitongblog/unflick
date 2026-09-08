//! Auto-subtitles for local files.
//!
//! The streaming half of this lives in [`super::url_post_play`]: for a URL,
//! yt-dlp already knows where the subtitles are and fetches them alongside
//! the stream. A file on disk has no such channel — the only thing that can
//! find subtitles for it is a lookup service, so the same
//! `auto_download_subtitles` setting has to mean OpenSubtitles here.
//!
//! What it will not do:
//!   * Spend a download on a file that already has subtitles. mpv loads
//!     embedded tracks and same-named sidecars by itself, so the common
//!     case costs nothing and asks nothing.
//!   * Run without an API key. OpenSubtitles downloads count against a
//!     personal daily allowance and unflick does not ship a shared key,
//!     so with no key configured this is silently inert — the settings
//!     panel is where the user is told about that, not stderr.
//!
//! Like the URL hooks, this returns as soon as it has spawned: nothing here
//! is allowed to stand between pressing play and seeing a picture.

use std::sync::Arc;
use std::time::Duration;

use super::opensubtitles::{self, SearchRequest};
use super::player::Player;

/// How long to let mpv settle before reading its track list. `play` already
/// waits for the file to load, but track-list population trails the load
/// event slightly and a premature read would report "no subtitles" for a
/// file that has them — and spend a download saying so.
const TRACK_SETTLE: Duration = Duration::from_millis(400);

/// Fire-and-forget subtitle lookup for a local file. Safe to call on every
/// play; it decides for itself whether there is anything to do.
///
/// `path` is the file as the user named it. Callers pass the original path
/// rather than anything mpv resolved, because the filename is what the
/// search query is derived from.
pub fn after_play_file_hooks(player: Arc<Player>, path: String) {
    let settings = super::url_post_play::read_settings_snapshot();
    if !worth_looking_up(
        &path,
        settings.auto_download_subtitles,
        opensubtitles::is_configured(),
    ) {
        return;
    }

    let languages = settings.subtitle_languages.join(",");
    // An OS thread with its own runtime, for the same reason the URL hooks
    // use one: this is called from Tauri command handlers and from the
    // control server's synchronous dispatch, neither of which guarantees a
    // tokio reactor on the calling thread.
    std::thread::spawn(move || {
        std::thread::sleep(TRACK_SETTLE);

        // Something is already there — an embedded track, or the sidecar a
        // previous run of this very function wrote. Either way, done.
        if !player.subtitle_list().is_empty() {
            return;
        }

        // Guard against the file having moved on while we slept: by now the
        // user may have skipped to the next thing, and attaching subtitles
        // for the previous file would be worse than attaching none.
        if !still_playing(&player, &path) {
            return;
        }

        let req = SearchRequest {
            query: None,
            file: Some(path.clone()),
            languages: Some(languages),
            hash: true,
        };
        let fallback = subtitle_cache_dir();
        match opensubtitles::run_auto(&req, &fallback) {
            Ok((dl, best, _)) => {
                // Re-check the file: the download is a network round trip
                // and the user may have moved on during it.
                if !still_playing(&player, &path) {
                    return;
                }
                match player.subtitle_load(&dl.path) {
                    Ok(()) => eprintln!(
                        "[auto-subs] attached {} ({}, {})",
                        dl.path,
                        best.language,
                        if best.moviehash_match {
                            "exact file match"
                        } else {
                            "title match"
                        }
                    ),
                    Err(e) => eprintln!("[auto-subs] sub-add failed for {}: {}", dl.path, e),
                }
            }
            Err(e) => eprintln!("[auto-subs] no subtitle downloaded for {}: {}", path, e),
        }
    });
}

/// Whether this play is worth a lookup at all, decided before a thread is
/// spawned for it.
///
///   * URLs go through the yt-dlp path in `url_post_play`; running both
///     would spend an OpenSubtitles download on what yt-dlp already fetched.
///   * Without a key there is nothing to ask — see the module docs.
///   * A path that is not a file is a disc, a device, or a share that went
///     away; there is no hash to compute and no name worth searching.
fn worth_looking_up(path: &str, enabled: bool, key_configured: bool) -> bool {
    enabled
        && key_configured
        && !super::yt_dlp::is_http_url(path)
        && std::path::Path::new(path).is_file()
}

/// Whether `path` is still what the player has loaded.
///
/// Compared with separators normalised: mpv echoes back the path it was
/// given, and the same file reaches us as `D:\x\y.mkv` from a drag-drop
/// and `D:/x/y.mkv` from the command line. A mismatch here would silently
/// switch the feature off for one of the two.
fn still_playing(player: &Player, path: &str) -> bool {
    player
        .status()
        .file
        .as_deref()
        .is_some_and(|current| normalize_path(current) == normalize_path(path))
}

fn normalize_path(s: &str) -> String {
    s.replace('\\', "/")
}

/// Where a download goes when the video's own directory can't take one —
/// a read-only mount, an optical disc, a share. Mirrors the GUI command's
/// choice so a file downloaded either way lands in the same place.
fn subtitle_cache_dir() -> std::path::PathBuf {
    dirs_next::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("unflick")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_real_file() -> std::path::PathBuf {
        let path = std::env::temp_dir().join("unflick-auto-subs-test.mkv");
        std::fs::write(&path, b"not really a matroska file").unwrap();
        path
    }

    #[test]
    fn looks_up_a_local_file_when_switched_on_and_keyed() {
        let file = a_real_file();
        assert!(worth_looking_up(&file.to_string_lossy(), true, true));
    }

    #[test]
    fn stays_out_of_the_way_when_switched_off() {
        let file = a_real_file();
        assert!(!worth_looking_up(&file.to_string_lossy(), false, true));
    }

    #[test]
    fn stays_out_of_the_way_without_a_key() {
        // The common case: the setting defaults on, and OpenSubtitles has
        // not been set up. Nothing should be attempted, and nothing should
        // be reported as failing.
        let file = a_real_file();
        assert!(!worth_looking_up(&file.to_string_lossy(), true, false));
    }

    #[test]
    fn leaves_streaming_urls_to_yt_dlp() {
        assert!(!worth_looking_up(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            true,
            true
        ));
    }

    #[test]
    fn skips_what_is_not_a_file() {
        assert!(!worth_looking_up("dvd://3", true, true));
        assert!(!worth_looking_up(
            &std::env::temp_dir()
                .join("unflick-no-such-file.mkv")
                .to_string_lossy(),
            true,
            true
        ));
    }

    #[test]
    fn path_separators_do_not_decide_whether_a_file_is_still_playing() {
        // mpv echoes back the path it was given, so the same file arrives
        // as a backslash path from a drag-drop and a forward-slash one from
        // the command line. Both have to compare equal or the lookup is
        // silently switched off for one of them.
        assert_eq!(
            normalize_path(r"D:\media\film.mkv"),
            normalize_path("D:/media/film.mkv")
        );
        assert_ne!(
            normalize_path(r"D:\media\film.mkv"),
            normalize_path(r"D:\media\other.mkv")
        );
    }
}
