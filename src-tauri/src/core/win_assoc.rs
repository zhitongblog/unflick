//! Windows Default Programs registration.
//!
//! Tauri's NSIS bundler writes a basic `HKCR\.<ext>\OpenWithProgIDs\unflick`
//! per file-association entry, which is enough to make unflick show up in
//! the right-click "Open with" menu but *not* enough to appear in
//! Settings → Default apps. For that, Windows wants a full
//! `RegisteredApplications` block with a `Capabilities` sub-key listing
//! the supported extensions. Without this:
//!   - "Set unflick as the default app" never lists unflick
//!   - `ms-settings:defaultapps?registeredAppUser=unflick` opens the
//!     generic page instead of jumping to our app
//!
//! We do the registration in HKCU on every app launch. HKCU avoids the
//! admin prompt UAC requires for HKLM writes, the registration is per-
//! user, and the writes are idempotent. The (un)installer's HKLM entries
//! still cover system-wide visibility for the same user once they
//! re-launch the app.
//!
//! All writes target the current user — Windows' "User Choice" hash
//! protection means apps still can't *force* themselves to be default
//! (only the user, via Settings, can confirm), but the user will now
//! actually find unflick in that list.

#![cfg(target_os = "windows")]

use anyhow::{anyhow, Result};
use winreg::enums::*;
use winreg::RegKey;

const CAPABILITIES_PATH: &str = r"Software\unflick\Capabilities";
const PROGID_VIDEO: &str = "unflick.Video";
const PROGID_AUDIO: &str = "unflick.Audio";

const VIDEO_EXTS: &[&str] = &[
    ".mp4", ".mkv", ".avi", ".mov", ".wmv", ".flv", ".webm", ".m4v", ".ts", ".mpg", ".mpeg",
    ".3gp", ".ogv",
];

const AUDIO_EXTS: &[&str] = &[".mp3", ".flac", ".wav", ".ogg", ".m4a", ".aac", ".opus"];

/// Register unflick as a Default Programs candidate for the current user.
/// Idempotent and best-effort: if any individual key write fails we log
/// and keep going so a partially-registered app is at least *somewhat*
/// discoverable.
pub fn register_default_program() -> Result<()> {
    let exe = std::env::current_exe().map_err(|e| anyhow!("current_exe: {e}"))?;
    let exe_str = exe.to_string_lossy().into_owned();

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    // 1) Capabilities key — name + description shown in Settings.
    let (cap, _) = hkcu
        .create_subkey(CAPABILITIES_PATH)
        .map_err(|e| anyhow!("create Capabilities: {e}"))?;
    cap.set_value("ApplicationName", &"unflick")
        .map_err(|e| anyhow!("ApplicationName: {e}"))?;
    cap.set_value(
        "ApplicationDescription",
        &"A modern, beautiful, AI-ready video player",
    )
    .map_err(|e| anyhow!("ApplicationDescription: {e}"))?;

    // 2) FileAssociations sub-key — maps extensions to our ProgIDs.
    let (fa, _) = hkcu
        .create_subkey(format!(r"{}\FileAssociations", CAPABILITIES_PATH))
        .map_err(|e| anyhow!("create FileAssociations: {e}"))?;
    for ext in VIDEO_EXTS {
        let _ = fa.set_value(*ext, &PROGID_VIDEO);
    }
    for ext in AUDIO_EXTS {
        let _ = fa.set_value(*ext, &PROGID_AUDIO);
    }

    // 3) ProgIDs with shell\open\command entries — this is what
    // Windows actually invokes when the user double-clicks a file.
    register_progid(&hkcu, PROGID_VIDEO, "unflick Video", &exe_str)?;
    register_progid(&hkcu, PROGID_AUDIO, "unflick Audio", &exe_str)?;

    // 4) RegisteredApplications entry — pointer that makes unflick
    // appear in Settings → Default apps. Value is the path to our
    // Capabilities key (relative to HKEY_CURRENT_USER).
    let (ra, _) = hkcu
        .create_subkey(r"Software\RegisteredApplications")
        .map_err(|e| anyhow!("create RegisteredApplications: {e}"))?;
    ra.set_value("unflick", &CAPABILITIES_PATH)
        .map_err(|e| anyhow!("RegisteredApplications.unflick: {e}"))?;

    Ok(())
}

fn register_progid(hkcu: &RegKey, progid: &str, friendly: &str, exe: &str) -> Result<()> {
    let base = format!(r"Software\Classes\{progid}");
    let (root, _) = hkcu
        .create_subkey(&base)
        .map_err(|e| anyhow!("create progid {progid}: {e}"))?;
    root.set_value("", &friendly)
        .map_err(|e| anyhow!("progid friendly: {e}"))?;

    // DefaultIcon (optional but makes the file-explorer entries match).
    if let Ok((di, _)) = hkcu.create_subkey(format!(r"{base}\DefaultIcon")) {
        let _ = di.set_value("", &format!("\"{exe}\",0"));
    }

    let (cmd, _) = hkcu
        .create_subkey(format!(r"{base}\shell\open\command"))
        .map_err(|e| anyhow!("create command: {e}"))?;
    cmd.set_value("", &format!("\"{exe}\" \"%1\""))
        .map_err(|e| anyhow!("command value: {e}"))?;

    Ok(())
}
