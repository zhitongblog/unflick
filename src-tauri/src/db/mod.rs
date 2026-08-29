use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaEntry {
    pub id: i64,
    pub path: String,
    pub title: String,
    pub duration: Option<f64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub file_size: Option<i64>,
    pub added_at: String,
    pub last_played: Option<String>,
    pub play_count: i64,
}

/// A named position inside a file.
///
/// `path` is whatever the player is holding — a local path or a URL — so a
/// bookmark on a stream survives the resolved CDN address changing between
/// sessions, the same way resume points do.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: i64,
    pub path: String,
    pub position: f64,
    /// `None` means unnamed; every surface shows the timestamp instead.
    pub name: Option<String>,
    pub created_at: String,
}

/// Environment override for where the library database lives.
///
/// Exists so the integration tests get their own database instead of
/// scribbling resume points and scanned media into the one the user is
/// actually using. Also gives anyone running a second isolated instance a
/// way to keep its history separate.
pub const DATA_DIR_ENV: &str = "UNFLICK_DATA_DIR";

/// Directory holding unflick's persistent data.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var(DATA_DIR_ENV)
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return PathBuf::from(dir);
    }
    let mut path = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("unflick");
    path
}

/// What was playing when unflick last saw the player.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Session {
    pub path: String,
    pub position: f64,
    /// 0 when unknown — live streams, or a file still loading.
    pub duration: f64,
    pub updated_at: String,
}

/// Below this many seconds in, there's nothing worth resuming to.
const MIN_RESUME_SECS: f64 = 1.0;

/// How close to the end still counts as "finished". Generous enough to
/// cover a user who stopped during the credits.
const END_TOLERANCE_SECS: f64 = 5.0;

/// Fraction of the runtime past which a file counts as finished, so short
/// clips aren't declared unfinished just because 5 seconds is most of them.
const END_TOLERANCE_RATIO: f64 = 0.98;

/// Whether playback got close enough to the end to treat the file as watched.
pub fn is_finished(position: f64, duration: f64) -> bool {
    if duration <= 0.0 {
        return false;
    }
    position >= duration - END_TOLERANCE_SECS || position / duration >= END_TOLERANCE_RATIO
}

/// Read a `media` row selected in the canonical column order.
///
/// Three queries wanted the same twelve-field mapping; keeping one copy
/// means a schema change can't leave one of them reading the wrong column.
fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<MediaEntry> {
    Ok(MediaEntry {
        id: row.get(0)?,
        path: row.get(1)?,
        title: row.get(2)?,
        duration: row.get(3)?,
        width: row.get(4)?,
        height: row.get(5)?,
        video_codec: row.get(6)?,
        audio_codec: row.get(7)?,
        file_size: row.get(8)?,
        added_at: row.get(9)?,
        last_played: row.get(10)?,
        play_count: row.get(11)?,
    })
}

/// How close two bookmarks have to be, in seconds, before a new one is
/// treated as a correction of the old rather than a second place.
const BOOKMARK_MERGE_SECS: f64 = 1.0;

fn row_to_bookmark(row: &rusqlite::Row<'_>) -> rusqlite::Result<Bookmark> {
    Ok(Bookmark {
        id: row.get(0)?,
        path: row.get(1)?,
        position: row.get(2)?,
        name: row.get(3)?,
        created_at: row.get(4)?,
    })
}

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open() -> Result<Self> {
        let db_path = Self::db_path();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&db_path)?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.init_tables()?;
        Ok(db)
    }

    fn db_path() -> PathBuf {
        let mut path = data_dir();
        path.push("library.db");
        path
    }

    fn init_tables(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS media (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL,
                duration REAL,
                width INTEGER,
                height INTEGER,
                video_codec TEXT,
                audio_codec TEXT,
                file_size INTEGER,
                added_at TEXT NOT NULL DEFAULT (datetime('now')),
                last_played TEXT,
                play_count INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_media_path ON media(path);
            CREATE INDEX IF NOT EXISTS idx_media_title ON media(title);

            CREATE TABLE IF NOT EXISTS playback_position (
                path TEXT PRIMARY KEY,
                position REAL NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS bookmark (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL,
                position REAL NOT NULL,
                name TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_bookmark_path ON bookmark(path);

            -- What was on screen when we last looked. One row, because
            -- there is one player; the CHECK is what keeps it that way
            -- rather than a convention nobody enforces.
            CREATE TABLE IF NOT EXISTS session (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                path TEXT NOT NULL,
                position REAL NOT NULL,
                duration REAL NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
        ",
        )?;
        Ok(())
    }

    pub fn upsert_media(&self, entry: &MediaEntry) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO media (path, title, duration, width, height, video_codec, audio_codec, file_size)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(path) DO UPDATE SET
                title=?2, duration=?3, width=?4, height=?5, video_codec=?6, audio_codec=?7, file_size=?8",
            params![
                entry.path,
                entry.title,
                entry.duration,
                entry.width,
                entry.height,
                entry.video_codec,
                entry.audio_codec,
                entry.file_size
            ],
        )?;
        Ok(())
    }

    pub fn search(&self, query: &str) -> Result<Vec<MediaEntry>> {
        let conn = self.conn.lock().unwrap();
        let pattern = format!("%{}%", query);
        let mut stmt = conn.prepare(
            "SELECT id, path, title, duration, width, height, video_codec, audio_codec, file_size, added_at, last_played, play_count
             FROM media WHERE title LIKE ?1 OR path LIKE ?1 ORDER BY title",
        )?;
        let entries = stmt
            .query_map(params![pattern], |row| {
                row_to_entry(row)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(entries)
    }

    pub fn clear_all(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute("DELETE FROM media", [])?;
        Ok(n)
    }

    pub fn list_all(&self) -> Result<Vec<MediaEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, path, title, duration, width, height, video_codec, audio_codec, file_size, added_at, last_played, play_count
             FROM media ORDER BY title",
        )?;
        let entries = stmt
            .query_map([], |row| {
                row_to_entry(row)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(entries)
    }

    /// Note that `path` was played, creating a row for it if the library
    /// has never seen it.
    ///
    /// This used to be a bare `UPDATE`, which meant a file opened by
    /// drag-and-drop or Open File — i.e. most of what anyone actually
    /// watches — matched no row and was silently not recorded. Only
    /// library-scanned files ever got a history, which made "recently
    /// played" a list of things you mostly hadn't played.
    ///
    /// The synthesised row carries just a title derived from the filename;
    /// a later library scan fills in duration and codecs via `upsert_media`,
    /// which leaves `last_played` and `play_count` alone.
    pub fn record_play(&self, path: &str) -> Result<()> {
        let title = std::path::Path::new(path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string());

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO media (path, title, last_played, play_count)
             VALUES (?1, ?2, datetime('now'), 1)
             ON CONFLICT(path) DO UPDATE SET
                last_played = datetime('now'),
                play_count = play_count + 1",
            params![path, title],
        )?;
        Ok(())
    }

    /// Most recently played files, newest first. Entries that have never
    /// been played are excluded — this is a history, not the library.
    pub fn recent(&self, limit: usize) -> Result<Vec<MediaEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, path, title, duration, width, height, video_codec, audio_codec,
                    file_size, added_at, last_played, play_count
             FROM media
             WHERE last_played IS NOT NULL
             ORDER BY last_played DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], row_to_entry)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Forget the play history without discarding scanned metadata.
    pub fn clear_recent(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE media SET last_played = NULL, play_count = 0 WHERE last_played IS NOT NULL",
            [],
        )?;
        Ok(n)
    }

    pub fn save_position(&self, path: &str, position: f64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO playback_position (path, position) VALUES (?1, ?2)
             ON CONFLICT(path) DO UPDATE SET position=?2, updated_at=datetime('now')",
            params![path, position],
        )?;
        Ok(())
    }

    /// Record where to resume `path`, or forget it if the file is done.
    ///
    /// This is the single policy for resume points, shared by GUI, CLI and
    /// MCP. Two things are deliberately *not* remembered:
    ///
    ///   * the first second — there's nothing meaningful to resume to.
    ///   * the tail end — a position saved at EOF means the next play
    ///     resumes on the last frame and lands straight back on EOF, so
    ///     re-opening a film you finished looks like a player that won't
    ///     play. A finished file starts over, so any stale point is
    ///     cleared rather than updated.
    ///
    /// `duration` of 0 means unknown (live streams, still-loading files);
    /// the tail check is skipped there rather than guessed at.
    pub fn remember_position(&self, path: &str, position: f64, duration: f64) -> Result<()> {
        if position <= MIN_RESUME_SECS {
            return Ok(());
        }
        if is_finished(position, duration) {
            return self.clear_position(path);
        }
        self.save_position(path, position)
    }

    pub fn get_position(&self, path: &str) -> Result<Option<f64>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT position FROM playback_position WHERE path = ?1")?;
        let result = stmt.query_row(params![path], |row| row.get(0)).ok();
        Ok(result)
    }

    pub fn clear_position(&self, path: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM playback_position WHERE path = ?1", params![path])?;
        Ok(())
    }

    // ─── Session ──────────────────────────────────────────────────────────

    /// Remember what is on screen, so a later launch can offer it back.
    ///
    /// Separate from `playback_position` on purpose. That table answers
    /// "if this file is opened again, where does it start" — one row per
    /// file, and a row survives forever. This answers "what was the user
    /// watching", which is one thing at a time and stops being true the
    /// moment they stop watching it.
    pub fn set_session(&self, path: &str, position: f64, duration: f64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO session (id, path, position, duration) VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET
                 path=?1, position=?2, duration=?3, updated_at=datetime('now')",
            params![path, position, duration],
        )?;
        Ok(())
    }

    pub fn get_session(&self) -> Result<Option<Session>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT path, position, duration, updated_at FROM session WHERE id = 1")?;
        let row = stmt
            .query_row([], |row| {
                Ok(Session {
                    path: row.get(0)?,
                    position: row.get(1)?,
                    duration: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            })
            .ok();
        Ok(row)
    }

    pub fn clear_session(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM session WHERE id = 1", [])?;
        Ok(())
    }

    pub fn remove(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM media WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ─── Bookmarks ────────────────────────────────────────────────────────

    /// Save `position` in `path`, or update the bookmark already sitting
    /// there.
    ///
    /// The merge window exists because the natural way to make a bookmark is
    /// a keypress, and a keypress gets repeated — by a held key, by a user
    /// unsure it registered. Two entries a third of a second apart are not
    /// two places in the film. Naming a spot that already has a bookmark
    /// therefore renames it rather than stacking a second one on top; an
    /// `add` with no name leaves an existing name alone, so re-pressing the
    /// key can't silently strip a label off.
    pub fn add_bookmark(&self, path: &str, position: f64, name: Option<&str>) -> Result<Bookmark> {
        let position = position.max(0.0);
        let conn = self.conn.lock().unwrap();

        let existing: Option<i64> = conn
            .query_row(
                "SELECT id FROM bookmark
                 WHERE path = ?1 AND abs(position - ?2) <= ?3
                 ORDER BY abs(position - ?2) LIMIT 1",
                params![path, position, BOOKMARK_MERGE_SECS],
                |row| row.get(0),
            )
            .ok();

        let id = match existing {
            Some(id) => {
                conn.execute(
                    "UPDATE bookmark SET position = ?2, name = COALESCE(?3, name) WHERE id = ?1",
                    params![id, position, name],
                )?;
                id
            }
            None => {
                conn.execute(
                    "INSERT INTO bookmark (path, position, name) VALUES (?1, ?2, ?3)",
                    params![path, position, name],
                )?;
                conn.last_insert_rowid()
            }
        };

        Self::read_bookmark(&conn, id)?
            .ok_or_else(|| anyhow::anyhow!("bookmark {} vanished after writing it", id))
    }

    /// Bookmarks for one file, or for every file when `path` is `None`.
    ///
    /// Ordered by position within a file so the list reads like the timeline,
    /// not like the order they happened to be made in.
    pub fn list_bookmarks(&self, path: Option<&str>) -> Result<Vec<Bookmark>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, path, position, name, created_at FROM bookmark
             WHERE ?1 IS NULL OR path = ?1
             ORDER BY path, position",
        )?;
        let rows = stmt.query_map(params![path], row_to_bookmark)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn get_bookmark(&self, id: i64) -> Result<Option<Bookmark>> {
        let conn = self.conn.lock().unwrap();
        Self::read_bookmark(&conn, id)
    }

    /// Rename a bookmark. `None` drops the name, so a mistyped label can be
    /// taken back off rather than only overwritten.
    pub fn rename_bookmark(&self, id: i64, name: Option<&str>) -> Result<Bookmark> {
        let conn = self.conn.lock().unwrap();
        let changed = conn.execute(
            "UPDATE bookmark SET name = ?2 WHERE id = ?1",
            params![id, name],
        )?;
        if changed == 0 {
            anyhow::bail!("no bookmark with id {}", id);
        }
        Self::read_bookmark(&conn, id)?
            .ok_or_else(|| anyhow::anyhow!("bookmark {} vanished after renaming it", id))
    }

    /// Delete one bookmark. `false` means there was nothing with that id —
    /// the caller reports that rather than claiming a deletion that didn't
    /// happen.
    pub fn remove_bookmark(&self, id: i64) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute("DELETE FROM bookmark WHERE id = ?1", params![id])? > 0)
    }

    /// Delete every bookmark for one file, or all of them when `path` is
    /// `None`. Returns how many went.
    pub fn clear_bookmarks(&self, path: Option<&str>) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute(
            "DELETE FROM bookmark WHERE ?1 IS NULL OR path = ?1",
            params![path],
        )?)
    }

    fn read_bookmark(conn: &Connection, id: i64) -> Result<Option<Bookmark>> {
        let mut stmt = conn.prepare(
            "SELECT id, path, position, name, created_at FROM bookmark WHERE id = ?1",
        )?;
        Ok(stmt.query_row(params![id], row_to_bookmark).ok())
    }
}
