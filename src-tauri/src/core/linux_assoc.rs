//! Linux file-association registration.
//!
//! The .deb postinst already runs `update-desktop-database` so that
//! /usr/share/applications/unflick.desktop is indexed and "Open With…"
//! lists unflick as a video player. But that only registers it as
//! *available*; on most distros the per-user default is still mpv,
//! VLC, or whatever the desktop environment shipped.
//!
//! At every launch we run `xdg-mime default unflick.desktop <type>`
//! for the common video and audio MIME types. xdg-mime is part of
//! xdg-utils which is a hard dependency of every modern desktop, so
//! we don't probe — just call it and ignore the failure if it's
//! missing (e.g. server install with no DE). Idempotent.

use std::process::Command;

const VIDEO_MIME_TYPES: &[&str] = &[
    "video/mp4",
    "video/x-matroska",
    "video/x-msvideo",
    "video/quicktime",
    "video/x-ms-wmv",
    "video/x-flv",
    "video/webm",
    "video/x-m4v",
    "video/mp2t",
    "video/mpeg",
    "video/3gpp",
    "video/ogg",
];

const AUDIO_MIME_TYPES: &[&str] = &[
    "audio/mpeg",
    "audio/flac",
    "audio/wav",
    "audio/ogg",
    "audio/mp4",
    "audio/aac",
    "audio/x-ms-wma",
];

/// Register unflick as the default app for video and audio MIME types.
/// Returns the count of types successfully registered. Best-effort —
/// missing xdg-mime / unflick.desktop is treated as success-with-zero.
pub fn register_default_program() -> Result<usize, String> {
    // Probe — don't error if xdg-mime isn't installed (server / minimal
    // install). The user can still launch unflick manually.
    let probe = Command::new("xdg-mime")
        .arg("--version")
        .output();
    if probe.is_err() {
        return Ok(0);
    }

    let mut ok = 0usize;
    for mime in VIDEO_MIME_TYPES.iter().chain(AUDIO_MIME_TYPES.iter()) {
        let status = Command::new("xdg-mime")
            .args(["default", "unflick.desktop", mime])
            .status();
        match status {
            Ok(s) if s.success() => ok += 1,
            Ok(s) => {
                eprintln!("[unflick-assoc] xdg-mime default {mime} -> exit {s}");
            }
            Err(e) => {
                eprintln!("[unflick-assoc] xdg-mime default {mime} -> error {e}");
            }
        }
    }
    Ok(ok)
}
