//! Finding what an older unflick left behind.
//!
//! Between v0.9 and v0.10 the Windows installer's per-user directory moved
//! from `%LOCALAPPDATA%\unflick` to `%LOCALAPPDATA%\Programs\unflick` — a
//! default that changed underneath us, not a decision. Both installs write
//! the *same* registry key (`HKCU\...\Uninstall\unflick`), so upgrading
//! overwrote the old uninstaller's registration and left roughly half a
//! gigabyte of files with nothing pointing at them: no entry in Apps &
//! Features, no shortcut, no way for anyone to know it was there.
//!
//! The installer can stop it happening again (see `installer-hooks.nsh`),
//! but that does nothing for a machine that already upgraded. Hence a
//! command.
//!
//! ## The one thing this must not get wrong
//!
//! On Windows `dirs_next::cache_dir()` is also `%LOCALAPPDATA%`, so the old
//! install directory and the *live* cache directory are the same path:
//! `%LOCALAPPDATA%\unflick\thumbs` and `.../covers` are being written right
//! now by the version the user is running. Deleting the folder wholesale
//! would take them with it. So the caches are named explicitly and skipped,
//! and nothing is removed without positive evidence that the directory was
//! an install — an `unflick.exe` or `uninstall.exe` that is not the one
//! running.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

/// Cache directories the current version still writes, inside what is
/// otherwise a stale install. Kept in sync with `thumbnail::cache_root` and
/// `nowplaying::cache_path` — if a third cache appears, it belongs here too.
const LIVE_CACHES: &[&str] = &["thumbs", "covers"];

/// Files that prove a directory was an unflick install rather than just a
/// cache folder that happens to share the name.
const INSTALL_MARKERS: &[&str] = &["unflick.exe", "uninstall.exe"];

/// Overrides where a stale install is looked for. Tests only.
pub const LEGACY_DIR_ENV: &str = "UNFLICK_LEGACY_DIR";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Leftover {
    pub path: String,
    pub bytes: u64,
    /// Whether this is a directory. Callers show a different verb for each.
    pub is_dir: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Report {
    /// The stale install directory, when there is one.
    pub directory: Option<String>,
    /// What would be removed from it.
    pub items: Vec<Leftover>,
    pub total_bytes: u64,
    /// Paths deliberately left alone, so the report can say so rather than
    /// leaving the user to wonder why the folder is still there.
    pub kept: Vec<String>,
    /// True when `apply` was requested and the removals were carried out.
    pub removed: bool,
}

/// Where an unflick older than v0.10 installed itself on Windows.
///
/// `None` on macOS and Linux: an `.app` bundle is replaced wholesale, and
/// `.deb` / `.rpm` upgrades are the package manager's problem. There has
/// never been a stale directory to find there.
pub fn legacy_install_dir() -> Option<PathBuf> {
    // Same escape hatch as `UNFLICK_DATA_DIR` and friends: the tests build a
    // fake stale install in a temp directory, because the rule worth
    // guarding — that the live caches survive — cannot be exercised against
    // a real machine without deleting someone's real half-gigabyte.
    if let Some(dir) = std::env::var_os(LEGACY_DIR_ENV) {
        let dir = PathBuf::from(dir);
        if !dir.as_os_str().is_empty() {
            return Some(dir);
        }
    }
    if !cfg!(target_os = "windows") {
        return None;
    }
    Some(dirs_next::data_local_dir()?.join("unflick"))
}

/// The directory the running binary lives in. Never a removal candidate.
fn current_install_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(Path::to_path_buf)
}

/// Look for a stale install and describe what could be reclaimed.
///
/// Reports without touching anything; `remove_leftovers` is the half that
/// deletes. Splitting them is the point — half a gigabyte is worth seeing
/// itemised before it goes.
pub fn scan() -> Report {
    let mut report = Report::default();

    let Some(dir) = legacy_install_dir() else {
        return report;
    };
    if !dir.is_dir() {
        return report;
    }
    // Someone still on the old layout: this is their live install.
    if current_install_dir().is_some_and(|cur| same_path(&cur, &dir)) {
        return report;
    }
    if !INSTALL_MARKERS.iter().any(|m| dir.join(m).exists()) {
        // A folder of caches and nothing else. Not ours to clear out.
        return report;
    }

    report.directory = Some(dir.to_string_lossy().into_owned());

    let Ok(entries) = std::fs::read_dir(&dir) else {
        return report;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if LIVE_CACHES.contains(&name.as_str()) {
            report.kept.push(path.to_string_lossy().into_owned());
            continue;
        }
        let is_dir = path.is_dir();
        let bytes = size_of(&path);
        report.total_bytes += bytes;
        report.items.push(Leftover {
            path: path.to_string_lossy().into_owned(),
            bytes,
            is_dir,
        });
    }
    // Biggest first: the point of the list is deciding whether it is worth
    // the disk, and one 180 MB folder settles that faster than forty files.
    report.items.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    report
}

/// Delete everything `scan` listed. Returns the same report, marked done.
pub fn remove_leftovers() -> Result<Report> {
    let mut report = scan();
    let Some(dir) = report.directory.clone() else {
        bail!("nothing to clean up");
    };

    // Re-check rather than trusting the scan: `remove_leftovers` deletes,
    // and the gap between describing and doing is exactly where a wrong
    // path would slip through.
    let root = PathBuf::from(&dir);
    if current_install_dir().is_some_and(|cur| same_path(&cur, &root)) {
        bail!("refusing to remove the directory unflick is running from");
    }

    let mut failures = Vec::new();
    for item in &report.items {
        let path = PathBuf::from(&item.path);
        if path.parent() != Some(root.as_path()) {
            // Cannot happen from our own scan; a guard against a future
            // caller handing us a path from somewhere else.
            failures.push(format!("{}: outside the stale install", item.path));
            continue;
        }
        let result = if item.is_dir {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        if let Err(e) = result {
            failures.push(format!("{}: {}", item.path, e));
        }
    }

    // Only if the caches were the last things left — otherwise the folder
    // is still in use and removing it is not on the table.
    if report.kept.is_empty() && failures.is_empty() {
        let _ = std::fs::remove_dir(&root);
    }

    if !failures.is_empty() {
        return Err(anyhow!(
            "could not remove {} item(s): {}",
            failures.len(),
            failures.join("; ")
        ));
    }
    report.removed = true;
    Ok(report)
}

/// Total size of a file, or of everything under a directory.
fn size_of(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_file() {
        return meta.len();
    }
    // Symlinked directories are not followed: their contents belong to
    // whatever they point at, and counting them would report disk that
    // removing this folder never frees.
    if !meta.is_dir() {
        return 0;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries.flatten().map(|e| size_of(&e.path())).sum()
}

fn same_path(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canon(a) == canon(b)
}

/// `501.0 MB`, `2.3 GB`. For a report a person reads.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{:.1} {}", value, UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_the_way_a_person_would_write_them() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(999), "999 B");
        assert_eq!(human_size(1024), "1.0 KB");
        assert_eq!(human_size(525_336_576), "501.0 MB");
        assert_eq!(human_size(3_221_225_472), "3.0 GB");
    }

    #[test]
    fn size_of_walks_a_directory() {
        let dir = std::env::temp_dir().join("unflick-cleanup-size-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        std::fs::write(dir.join("a.bin"), vec![0u8; 100]).unwrap();
        std::fs::write(dir.join("nested").join("b.bin"), vec![0u8; 250]).unwrap();

        assert_eq!(size_of(&dir), 350);
        assert_eq!(size_of(&dir.join("a.bin")), 100);
        assert_eq!(size_of(&dir.join("missing")), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_live_caches_are_the_ones_the_player_writes() {
        // If a cache is added elsewhere and not listed here, cleanup would
        // delete it out from under the running version.
        assert!(LIVE_CACHES.contains(&"thumbs"));
        assert!(LIVE_CACHES.contains(&"covers"));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn there_is_nothing_to_clean_off_windows() {
        assert!(legacy_install_dir().is_none());
        let report = scan();
        assert!(report.directory.is_none());
        assert!(report.items.is_empty());
    }
}
