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
}
