//! What kind of thing a caller pointed us at, and what to say when mpv
//! cannot open it.
//!
//! The reason this exists is network shares. `smb://server/media/film.mkv` is
//! what people type — it is what VLC accepts and what a file manager shows in
//! its address bar — but our bundled mpv has no SMB protocol at all, and
//! neither does a stock ffmpeg. Handed one, mpv fails with nothing useful,
//! and the honest answer ("mount the share, then play the mounted path") is
//! not something anyone guesses.
//!
//! Mounted shares themselves need no help: a UNC path on Windows, `/Volumes`
//! on macOS, `/mnt` on Linux are all ordinary file paths by the time mpv sees
//! them.

use crate::db::SourceKey;

/// What to remember `input` under, and where to open it.
///
/// For everything with a path of its own — a file, a URL, a `.iso` — the key
/// *is* the path, byte for byte and deliberately unnormalised: every resume
/// point and bookmark already in the database was written under the exact
/// string the user typed, and canonicalising here would orphan the lot.
///
/// The exception is a mounted disc, whose path is the drive it happens to be
/// in. `disc::identity` reads the disc's own index and returns something
/// that means *that disc*, so the next one into the same drive is not
/// offered its bookmarks.
///
/// Costs one `read_dir` and one small file read for a disc, and nothing at
/// all for a URL — which is why the scheme check comes first.
pub fn key_of(input: &str) -> SourceKey {
    if scheme_of(input).is_some() {
        return SourceKey::path(input);
    }
    match crate::core::disc::identity(input) {
        Some(id) => SourceKey {
            key: id.key,
            path: input.to_string(),
            label: id.label,
        },
        None => SourceKey::path(input),
    }
}

/// The same, but free when `input` is what the player already has open.
///
/// This is what keeps the five-second autosave tick and every status-driven
/// bookmark call off the optical drive: probing a disc twelve times a minute
/// would keep it spun up for as long as the film is on.
pub fn key_of_playing(player: &super::player::Player, input: &str) -> SourceKey {
    match player.current_source() {
        Some(src) if src.path == input => src,
        _ => key_of(input),
    }
}

/// The URL scheme of `input`, lowercased, or `None` if it is a plain path.
///
/// Requires `://` so a Windows drive letter never reads as a scheme, and
/// requires two or more characters for the same reason — `d://foo` is far
/// more likely to be a mangled path than a protocol nobody has heard of.
pub fn scheme_of(input: &str) -> Option<String> {
    let (scheme, _) = input.split_once("://")?;
    if scheme.len() < 2 {
        return None;
    }
    let mut chars = scheme.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
    let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    (first_ok && rest_ok).then(|| scheme.to_ascii_lowercase())
}

/// Advice for a network scheme this build cannot open, or `None` when we have
/// nothing better to say than "unsupported".
///
/// Deliberately concrete about the platform in hand: "mount the share" is
/// true everywhere and actionable nowhere.
pub fn mount_hint(scheme: &str) -> Option<String> {
    let how = mount_how(scheme)?;
    Some(format!("{} URLs are not supported — {}.", share_kind(scheme), how))
}

/// The platform-specific "mount it, then play the mounted path" clause on its
/// own, for the messages that need it without the "not supported" in front.
pub fn mount_how(scheme: &str) -> Option<&'static str> {
    Some(match scheme {
        "smb" | "cifs" => {
            if cfg!(target_os = "windows") {
                r"map or open the share in Explorer, then play the UNC path (\\server\share\file.mkv)"
            } else if cfg!(target_os = "macos") {
                "connect to the server in Finder, then play the path under /Volumes"
            } else {
                "mount the share (mount -t cifs), then play the mounted path"
            }
        }
        "nfs" => {
            if cfg!(target_os = "windows") {
                "mount the export with Client for NFS, then play the mapped drive path"
            } else if cfg!(target_os = "macos") {
                "mount the export in Finder or with mount_nfs, then play the path under /Volumes"
            } else {
                "mount the export (mount -t nfs), then play the mounted path"
            }
        }
        _ => return None,
    })
}

fn share_kind(scheme: &str) -> &'static str {
    if scheme == "nfs" { "NFS" } else { "SMB" }
}

/// The message for a share URL that this build's mpv *claims* to speak, but
/// could not open.
///
/// Some libmpv builds list `smb://` among their protocols — Ubuntu 24.04's
/// does — so the refusal in [`unsupported_message`] never fires and the
/// attempt goes through to mpv. Letting it try costs nothing (it fails in
/// milliseconds) and keeps a build whose ffmpeg really has libsmbclient able
/// to play a share directly. But the listing is not proof: on Ubuntu the
/// ffmpeg underneath has no SMB at all and answers "Protocol not found" even
/// for an open guest share on localhost (seen 2026-09-24). mpv reports all of
/// it as "could not open", which cannot tell that apart from a server that is
/// down or a share that wants a password — so the message names the
/// possibilities rather than picking one, and ends with the advice every
/// other build hands out up front.
///
/// `None` for anything that is not a share, so other failures keep their
/// own wording.
pub fn share_open_failed_message(input: &str, scheme: &str, error: &str) -> Option<String> {
    let how = mount_how(scheme)?;
    Some(format!(
        "{}. This build's mpv lists {}:// but could not open it — its ffmpeg may \
         lack {} support, the server may be unreachable, or the share may need a \
         login. To play it, {}.",
        error.trim_end_matches('.'),
        scheme,
        share_kind(scheme),
        how
    ))
}

/// A path written the way Windows writes a share, on a machine that is not
/// Windows — `\\server\share\film.mkv`, or the `//server/share/film.mkv`
/// people type out of the same habit.
///
/// Only useful off Windows, and only for a path that is not there: on macOS a
/// leading `//` is just a root-relative path, so `//Volumes/media/film.mkv`
/// exists and plays and must never be second-guessed. What this catches is
/// the case that otherwise ends in mpv's bare "could not open": someone typed
/// the Windows spelling of a share on a Mac, where the answer is the same one
/// `smb://` already gets — mount it first.
///
/// Returns the host, so the message can name what it recognised rather than
/// telling someone about SMB when they mistyped a local path.
pub fn windows_share_host(path: &str) -> Option<String> {
    if cfg!(target_os = "windows") {
        return None;
    }
    let rest = path
        .strip_prefix(r"\\")
        .or_else(|| path.strip_prefix("//"))?;
    // A share is host + something on it. A bare `//host` names no file, and
    // three slashes is a typo of a local path, not a share.
    let mut parts = rest.splitn(2, |c| c == '/' || c == '\\');
    let host = parts.next()?;
    let on_it = parts.next()?;
    if host.is_empty() || on_it.is_empty() || host.contains(' ') {
        return None;
    }
    Some(host.to_string())
}

/// Whether `path` names a Windows share — a UNC path, reached over the
/// network however local it looks.
///
/// Windows only, and deliberately so. A share mounted on macOS or Linux is an
/// ordinary path by the time it reaches us (`/Volumes/media`, `/mnt/nas`), and
/// nothing in the string separates it from a local disk — `/mnt/usb` is not a
/// network mount. Guessing from the prefix would slow local playback for
/// everyone who keeps their films under `/mnt`. Those platforms keep mpv's
/// default; this covers the one case a string can settle on its own.
pub fn is_unc_path(path: &str) -> bool {
    // `\\?\C:\...` and `\\.\PhysicalDrive0` share the prefix but are local
    // device paths. Only the UNC form of the extended prefix is a share.
    if let Some(rest) = path.strip_prefix(r"\\?\").or_else(|| path.strip_prefix(r"\\.\")) {
        return rest.len() > 4 && rest[..4].eq_ignore_ascii_case(r"UNC\");
    }
    path.starts_with(r"\\") && path.len() > 2
}

/// The message for a source whose scheme mpv has no protocol for.
pub fn unsupported_message(input: &str, scheme: &str, supported: &[String]) -> String {
    let mut msg = format!("cannot open {}: no {}:// support in this build", input, scheme);
    if let Some(hint) = mount_hint(scheme) {
        msg.push_str(". ");
        msg.push_str(&hint);
    } else if !supported.is_empty() {
        msg.push_str(". Supported: ");
        msg.push_str(&supported.join(", "));
    }
    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_paths_are_not_schemes() {
        assert_eq!(scheme_of(r"D:\media\film.mkv"), None);
        assert_eq!(scheme_of(r"\\server\share\film.mkv"), None);
        assert_eq!(scheme_of("C:/media/film.mkv"), None);
        // A drive letter with a doubled separator is still a path, not a
        // protocol — nothing is served over `d://`.
        assert_eq!(scheme_of("d://media/film.mkv"), None);
    }

    #[test]
    fn unix_paths_are_not_schemes() {
        assert_eq!(scheme_of("/Volumes/media/film.mkv"), None);
        assert_eq!(scheme_of("./film.mkv"), None);
        assert_eq!(scheme_of("film.mkv"), None);
    }

    #[test]
    fn schemes_are_recognised_and_lowercased() {
        assert_eq!(scheme_of("smb://server/share/f.mkv").as_deref(), Some("smb"));
        assert_eq!(scheme_of("SMB://server/share/f.mkv").as_deref(), Some("smb"));
        assert_eq!(scheme_of("https://example.com/f.mp4").as_deref(), Some("https"));
        assert_eq!(scheme_of("webdavs://h/f.mkv").as_deref(), Some("webdavs"));
    }

    #[test]
    fn share_protocols_get_a_platform_specific_hint() {
        for scheme in ["smb", "cifs"] {
            let hint = mount_hint(scheme).expect("share protocols carry a hint");
            assert!(hint.starts_with("SMB URLs are not supported"), "{}", hint);
        }
        // NFS is not mounted the way SMB is on any of the three platforms, so
        // handing it the SMB advice would send people somewhere useless.
        let nfs = mount_hint("nfs").expect("nfs carries a hint");
        assert!(nfs.starts_with("NFS URLs are not supported"), "{}", nfs);
        assert!(nfs.to_lowercase().contains("export"), "{}", nfs);

        assert_eq!(mount_hint("https"), None);
    }

    #[test]
    fn a_share_that_was_tried_and_failed_still_says_how_to_mount_it() {
        let msg = share_open_failed_message(
            "smb://nas/media/f.mkv",
            "smb",
            "could not open smb://nas/media/f.mkv",
        )
        .expect("smb carries advice");
        // mpv's own words stay first, for the bug report …
        assert!(msg.starts_with("could not open smb://nas/media/f.mkv. "), "{}", msg);
        // … and the advice is the same clause the up-front refusal uses.
        assert!(msg.contains(mount_how("smb").unwrap()), "{}", msg);
        assert!(msg.contains("SMB"), "{}", msg);
        assert!(!msg.contains("not supported"), "this build does support it: {}", msg);

        let nfs = share_open_failed_message("nfs://h/e/f.mkv", "nfs", "could not open x")
            .expect("nfs carries advice");
        assert!(nfs.contains(mount_how("nfs").unwrap()), "{}", nfs);

        // Anything that is not a share keeps its own error untouched.
        assert_eq!(share_open_failed_message("https://h/f.mp4", "https", "nope"), None);
    }

    #[test]
    fn unc_paths_are_recognised_as_shares() {
        assert!(is_unc_path(r"\\server\share\film.mkv"));
        assert!(is_unc_path(r"\\192.168.1.10\media\film.mkv"));
        assert!(is_unc_path(r"\\?\UNC\server\share\film.mkv"));
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn the_windows_spelling_of_a_share_is_recognised_off_windows() {
        assert_eq!(windows_share_host(r"\\server\share\film.mkv").as_deref(), Some("server"));
        assert_eq!(windows_share_host("//server/share/film.mkv").as_deref(), Some("server"));
        assert_eq!(windows_share_host("//192.168.1.10/media/film.mkv").as_deref(), Some("192.168.1.10"));
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn an_ordinary_path_is_never_mistaken_for_a_share() {
        // The shapes that matter: a real absolute path, a root-relative one
        // that macOS resolves happily, and a host with nothing on it.
        assert_eq!(windows_share_host("/Volumes/media/film.mkv"), None);
        assert_eq!(windows_share_host("///Volumes/media/film.mkv"), None);
        assert_eq!(windows_share_host("//server"), None);
        assert_eq!(windows_share_host("//server/"), None);
        assert_eq!(windows_share_host("film.mkv"), None);
    }

    #[test]
    fn local_paths_are_not_shares() {
        assert!(!is_unc_path(r"D:\media\film.mkv"));
        assert!(!is_unc_path("/mnt/nas/film.mkv"), "no string says this is a mount");
        assert!(!is_unc_path("/Volumes/media/film.mkv"));
        // Extended-length and device prefixes look like UNC but are local.
        assert!(!is_unc_path(r"\\?\D:\media\film.mkv"));
        assert!(!is_unc_path(r"\\.\PhysicalDrive0"));
    }

    #[test]
    fn an_ordinary_path_is_its_own_key_byte_for_byte() {
        // Every row written before discs had identities was keyed by the
        // exact string the user typed. Normalising here — a trailing slash,
        // a case fold, a canonicalise — would orphan all of them.
        for path in [
            r"D:\films\something.mkv",
            "/home/alex/film.mp4",
            "./relative.mkv",
            "/tmp/an image.iso",
        ] {
            let src = key_of(path);
            assert_eq!(src.key, path);
            assert_eq!(src.path, path);
            assert_eq!(src.label, None);
            assert!(!src.is_disc());
        }
    }

    #[test]
    fn a_url_is_never_stat_ed() {
        // Nothing here should reach the filesystem for something that is
        // plainly not on it — including mpv's own disc syntax, which is
        // taken at its word everywhere else too.
        for url in ["https://example.com/f.mp4", "dvd://3", "smb://h/share/f.mkv"] {
            assert_eq!(key_of(url), crate::db::SourceKey::path(url));
        }
    }

    #[test]
    fn a_mounted_disc_is_keyed_by_the_disc_and_opened_by_the_path() {
        let dir = std::env::temp_dir().join("unflick-source-key-disc");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("VIDEO_TS")).unwrap();
        std::fs::write(dir.join("VIDEO_TS").join("VIDEO_TS.IFO"), b"a video manager")
            .unwrap();

        let path = dir.to_string_lossy().into_owned();
        let src = key_of(&path);
        assert!(src.is_disc(), "expected a disc key, got {}", src.key);
        assert_eq!(src.path, path, "mpv still needs somewhere to open");
        assert_eq!(src.label.as_deref(), Some("unflick-source-key-disc"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unsupported_message_falls_back_to_listing_protocols() {
        let supported = vec!["file".to_string(), "https".to_string()];
        let msg = unsupported_message("gopher://h/f", "gopher", &supported);
        assert!(msg.contains("no gopher:// support"), "{}", msg);
        assert!(msg.contains("file, https"), "{}", msg);
    }
}
