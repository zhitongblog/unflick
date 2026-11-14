use std::sync::Mutex;

use super::types::{PlaylistEntry, RepeatMode};

/// Manages an ordered playlist of media files.
/// Thread-safe: all state is behind Mutex.
///
/// `entries` is the canonical list and its indices are what the UI, CLI and
/// MCP address. Traversal order is separate: `order` holds a permutation of
/// entry indices, which is the identity when shuffle is off. Keeping the two
/// apart means toggling shuffle never renumbers anything the user is
/// looking at.
pub struct Playlist {
    entries: Mutex<Vec<String>>,
    current: Mutex<Option<usize>>,
    repeat: Mutex<RepeatMode>,
    shuffle: Mutex<bool>,
    order: Mutex<Vec<usize>>,
}

impl Playlist {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            current: Mutex::new(None),
            repeat: Mutex::new(RepeatMode::Off),
            shuffle: Mutex::new(false),
            order: Mutex::new(Vec::new()),
        }
    }

    /// Add a file to the end of the playlist.
    pub fn add(&self, path: &str) {
        let mut entries = self.entries.lock().unwrap();
        entries.push(path.to_string());
        let new_index = entries.len() - 1;

        // Newly added entries join the traversal order at the end even in
        // shuffle mode — dropping a file onto a shuffled playlist should
        // still play it, not silently never reach it.
        self.order.lock().unwrap().push(new_index);

        if entries.len() == 1 {
            let mut current = self.current.lock().unwrap();
            if current.is_none() {
                *current = Some(0);
            }
        }
    }

    /// Remove entry at the given index.
    pub fn remove(&self, index: usize) -> Result<(), String> {
        let mut entries = self.entries.lock().unwrap();
        if index >= entries.len() {
            return Err(format!("index {} out of range (playlist has {} entries)", index, entries.len()));
        }
        entries.remove(index);

        // Drop the removed index from the traversal order and shift every
        // later index down one to match the entries vector.
        {
            let mut order = self.order.lock().unwrap();
            order.retain(|&i| i != index);
            for i in order.iter_mut() {
                if *i > index {
                    *i -= 1;
                }
            }
        }

        let mut current = self.current.lock().unwrap();
        if let Some(cur) = *current {
            if entries.is_empty() {
                *current = None;
            } else if index == cur {
                // Current track was removed; clamp to valid range
                *current = Some(cur.min(entries.len() - 1));
            } else if index < cur {
                // An earlier entry was removed; shift current back
                *current = Some(cur - 1);
            }
        }
        Ok(())
    }

    /// List all entries with their index and current flag.
    pub fn list(&self) -> Vec<PlaylistEntry> {
        let entries = self.entries.lock().unwrap();
        let current = self.current.lock().unwrap();
        entries
            .iter()
            .enumerate()
            .map(|(i, path)| {
                let title = std::path::Path::new(path)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.clone());
                PlaylistEntry {
                    index: i,
                    path: path.clone(),
                    title,
                    current: *current == Some(i),
                }
            })
            .collect()
    }

    /// Advance to the next track. Returns its path, or None if at the end
    /// with repeat off.
    ///
    /// This is the *manual* skip (next button, `unflick playlist next`), so
    /// repeat-one does not apply — pressing next while looping one track
    /// should still move on. Only `RepeatMode::All` wraps.
    pub fn next(&self) -> Option<String> {
        self.step(1)
    }

    /// Go to the previous track. Returns its path, or None if at the
    /// beginning with repeat off.
    pub fn prev(&self) -> Option<String> {
        self.step(-1)
    }

    /// Shared traversal for next/prev. Walks `order`, not `entries`, so
    /// shuffle is honoured in both directions.
    fn step(&self, delta: isize) -> Option<String> {
        let entries = self.entries.lock().unwrap();
        if entries.is_empty() {
            return None;
        }
        let order = self.order.lock().unwrap();
        let mut current = self.current.lock().unwrap();
        let repeat = *self.repeat.lock().unwrap();

        let cur_entry = match *current {
            Some(c) => c,
            None => {
                // Nothing playing yet: start at the head of the order.
                let first = *order.first()?;
                *current = Some(first);
                return entries.get(first).cloned();
            }
        };

        let pos = order.iter().position(|&i| i == cur_entry)?;
        let len = order.len() as isize;
        let raw = pos as isize + delta;

        let next_pos = if raw < 0 || raw >= len {
            if repeat == RepeatMode::All {
                raw.rem_euclid(len) as usize
            } else {
                return None;
            }
        } else {
            raw as usize
        };

        let next_entry = order[next_pos];
        *current = Some(next_entry);
        entries.get(next_entry).cloned()
    }

    /// What to play when the current file hits EOF. Unlike `next`, this
    /// honours repeat-one by returning the current path again.
    pub fn next_on_eof(&self) -> Option<String> {
        if *self.repeat.lock().unwrap() == RepeatMode::One {
            return self.current().map(|(_, path)| path);
        }
        self.next()
    }

    /// Clear all entries.
    pub fn clear(&self) {
        let mut entries = self.entries.lock().unwrap();
        let mut current = self.current.lock().unwrap();
        entries.clear();
        self.order.lock().unwrap().clear();
        *current = None;
    }

    /// Get the current track index and path.
    pub fn current(&self) -> Option<(usize, String)> {
        let entries = self.entries.lock().unwrap();
        let current = self.current.lock().unwrap();
        current.and_then(|idx| entries.get(idx).map(|p| (idx, p.clone())))
    }

    /// Jump to a specific index.
    pub fn set_current(&self, index: usize) -> Result<String, String> {
        let entries = self.entries.lock().unwrap();
        if index >= entries.len() {
            return Err(format!("index {} out of range (playlist has {} entries)", index, entries.len()));
        }
        let mut current = self.current.lock().unwrap();
        *current = Some(index);
        Ok(entries[index].clone())
    }

    // ─── Repeat / shuffle ─────────────────────────────────────────────────

    pub fn repeat_mode(&self) -> RepeatMode {
        *self.repeat.lock().unwrap()
    }

    pub fn set_repeat_mode(&self, mode: RepeatMode) {
        *self.repeat.lock().unwrap() = mode;
    }

    pub fn shuffle_enabled(&self) -> bool {
        *self.shuffle.lock().unwrap()
    }

    /// Turn shuffle on or off and rebuild the traversal order.
    ///
    /// Enabling shuffle keeps whatever is playing at the head of the new
    /// order, so the current track isn't cut off the moment the button is
    /// pressed. Disabling restores plain ascending order.
    pub fn set_shuffle(&self, enabled: bool) {
        *self.shuffle.lock().unwrap() = enabled;
        self.rebuild_order();
    }

    fn rebuild_order(&self) {
        // Snapshot the small fields through short-lived guards before taking
        // `order`. Everywhere else locks in entries → order → current →
        // repeat order; grabbing them nested here in a different order is
        // how you get an intermittent deadlock between `set_shuffle` and a
        // concurrent `next()`.
        let shuffle = *self.shuffle.lock().unwrap();
        let current = *self.current.lock().unwrap();
        let entry_count = self.entries.lock().unwrap().len();

        let mut order = self.order.lock().unwrap();
        *order = (0..entry_count).collect();
        if !shuffle {
            return;
        }

        // Fisher-Yates over a xorshift stream. A full RNG crate would be
        // dead weight for one shuffle button; this is not security-relevant
        // and only needs to look unordered to a human.
        let mut state = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x2545F4914F6CDD1D)
            | 1;
        let mut next_rand = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };

        let len = order.len();
        for i in (1..len).rev() {
            let j = (next_rand() % (i as u64 + 1)) as usize;
            order.swap(i, j);
        }

        // Float the current entry to the front so it stays the reference
        // point for the next `step` call.
        if let Some(cur) = current {
            if let Some(pos) = order.iter().position(|&i| i == cur) {
                order.swap(0, pos);
            }
        }
    }

    // ─── Import / Export (.m3u / .m3u8 / .pls) ────────────────────────────

    /// Save the current playlist to a `.m3u`, `.m3u8`, or `.pls` file.
    /// Returns the number of entries written.
    pub fn save_to_file(&self, path: &str) -> Result<usize, String> {
        let list = self.list();
        let target = std::path::Path::new(path);
        if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create {}: {}", parent.display(), e))?;
        }

        let is_pls = target
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("pls"))
            .unwrap_or(false);

        let mut out = String::new();
        if is_pls {
            out.push_str("[playlist]\n");
            for (i, entry) in list.iter().enumerate() {
                let idx = i + 1;
                out.push_str(&format!("File{}={}\n", idx, entry.path));
                out.push_str(&format!("Title{}={}\n", idx, entry.title));
            }
            out.push_str(&format!("NumberOfEntries={}\nVersion=2\n", list.len()));
        } else {
            out.push_str("#EXTM3U\n");
            for entry in &list {
                out.push_str(&format!("#EXTINF:-1,{}\n{}\n", entry.title, entry.path));
            }
        }

        std::fs::write(target, out)
            .map_err(|e| format!("failed to write {}: {}", target.display(), e))?;
        Ok(list.len())
    }

    /// Load entries from a `.m3u`, `.m3u8`, or `.pls` file into the playlist.
    /// Replaces existing entries unless `append` is true.
    pub fn load_from_file(&self, path: &str, append: bool) -> Result<Vec<PlaylistEntry>, String> {
        let source = std::path::Path::new(path);
        let bytes = std::fs::read(source)
            .map_err(|e| format!("failed to read {}: {}", source.display(), e))?;
        let raw = String::from_utf8_lossy(&bytes);

        if raw.contains("#EXT-X-TARGETDURATION") || raw.contains("#EXT-X-STREAM-INF") {
            return Err(
                "this .m3u8 is an HLS media stream, not a playlist file — play it with `unflick play`"
                    .to_string(),
            );
        }

        let base_dir = source.parent();
        let is_pls = source
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("pls"))
            .unwrap_or(false)
            || raw.trim_start().starts_with("[playlist]");

        let mut parsed_paths = Vec::new();
        if is_pls {
            for line in raw.lines() {
                let trimmed = line.trim();
                if let Some((k, v)) = trimmed.split_once('=') {
                    let k_lower = k.trim().to_ascii_lowercase();
                    if k_lower.starts_with("file")
                        && k_lower[4..].chars().all(|c| c.is_ascii_digit())
                    {
                        let item = v.trim();
                        if !item.is_empty() {
                            parsed_paths.push(resolve_playlist_entry(item, base_dir));
                        }
                    }
                }
            }
        } else {
            for line in raw.lines() {
                let trimmed = line.trim().trim_start_matches('\u{feff}');
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                parsed_paths.push(resolve_playlist_entry(trimmed, base_dir));
            }
        }

        if !append {
            self.clear();
        }
        for p in parsed_paths {
            self.add(&p);
        }
        Ok(self.list())
    }
}

fn resolve_playlist_entry(raw: &str, base_dir: Option<&std::path::Path>) -> String {
    if super::source::scheme_of(raw).is_some() || super::source::is_unc_path(raw) {
        return raw.to_string();
    }
    let p = std::path::Path::new(raw);
    if p.is_absolute() {
        return raw.to_string();
    }
    if let Some(dir) = base_dir.filter(|d| !d.as_os_str().is_empty()) {
        return dir.join(p).to_string_lossy().into_owned();
    }
    raw.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_loads_m3u_and_pls_playlists() {
        let dir = std::env::temp_dir().join(format!("unflick-pl-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let pl = Playlist::new();
        pl.add("https://example.com/a.mp4");
        pl.add("sub/b.mkv");

        let m3u = dir.join("list.m3u");
        assert_eq!(pl.save_to_file(&m3u.to_string_lossy()).unwrap(), 2);

        let loaded = Playlist::new();
        let entries = loaded.load_from_file(&m3u.to_string_lossy(), false).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "https://example.com/a.mp4");
        assert!(entries[1].path.ends_with("sub/b.mkv"));

        let pls = dir.join("list.pls");
        assert_eq!(pl.save_to_file(&pls.to_string_lossy()).unwrap(), 2);
        let loaded_pls = Playlist::new();
        let entries_pls = loaded_pls.load_from_file(&pls.to_string_lossy(), false).unwrap();
        assert_eq!(entries_pls.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
