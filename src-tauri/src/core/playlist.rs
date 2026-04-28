use std::sync::Mutex;

use super::types::PlaylistEntry;

/// Manages an ordered playlist of media files.
/// Thread-safe: all state is behind Mutex.
pub struct Playlist {
    entries: Mutex<Vec<String>>,
    current: Mutex<Option<usize>>,
}

impl Playlist {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            current: Mutex::new(None),
        }
    }

    /// Add a file to the end of the playlist.
    pub fn add(&self, path: &str) {
        let mut entries = self.entries.lock().unwrap();
        entries.push(path.to_string());
        // If this is the first entry and nothing is current, set it as current
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

    /// Advance to the next track. Returns its path, or None if at end.
    pub fn next(&self) -> Option<String> {
        let entries = self.entries.lock().unwrap();
        let mut current = self.current.lock().unwrap();

        if entries.is_empty() {
            return None;
        }

        let next_idx = match *current {
            Some(cur) => {
                if cur + 1 < entries.len() {
                    cur + 1
                } else {
                    return None; // at end
                }
            }
            None => 0,
        };

        *current = Some(next_idx);
        Some(entries[next_idx].clone())
    }

    /// Go to the previous track. Returns its path, or None if at beginning.
    pub fn prev(&self) -> Option<String> {
        let entries = self.entries.lock().unwrap();
        let mut current = self.current.lock().unwrap();

        if entries.is_empty() {
            return None;
        }

        let prev_idx = match *current {
            Some(cur) => {
                if cur > 0 {
                    cur - 1
                } else {
                    return None; // at beginning
                }
            }
            None => 0,
        };

        *current = Some(prev_idx);
        Some(entries[prev_idx].clone())
    }

    /// Clear all entries.
    pub fn clear(&self) {
        let mut entries = self.entries.lock().unwrap();
        let mut current = self.current.lock().unwrap();
        entries.clear();
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
}
