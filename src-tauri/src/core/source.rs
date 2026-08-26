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
    let how = match scheme {
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
    };

    let kind = if scheme == "nfs" { "NFS" } else { "SMB" };
    Some(format!("{} URLs are not supported — {}.", kind, how))
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
    fn unc_paths_are_recognised_as_shares() {
        assert!(is_unc_path(r"\\server\share\film.mkv"));
        assert!(is_unc_path(r"\\192.168.1.10\media\film.mkv"));
        assert!(is_unc_path(r"\\?\UNC\server\share\film.mkv"));
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
    fn unsupported_message_falls_back_to_listing_protocols() {
        let supported = vec!["file".to_string(), "https".to_string()];
        let msg = unsupported_message("gopher://h/f", "gopher", &supported);
        assert!(msg.contains("no gopher:// support"), "{}", msg);
        assert!(msg.contains("file, https"), "{}", msg);
    }
}
