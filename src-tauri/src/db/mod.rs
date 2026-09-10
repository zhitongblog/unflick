use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

/// How a row is found again, and where to open it.
///
/// `key` identifies the thing across sessions; `path` is what to hand mpv
/// and what to show a person. For everything with a path of its own they are
/// the same string, byte for byte and deliberately unnormalised —
/// canonicalising here would orphan every row already written.
///
/// They differ for one case, which is the whole reason this type exists: a
/// mounted disc. Its path is the drive (`E:\`, `/Volumes/DVD_VIDEO`) and the
/// next disc into that drive answers to the same one, so before this
/// bookmarks and resume points left on one film were offered on the next.
/// `core::source::key_of` is what builds these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceKey {
    pub key: String,
    pub path: String,
    /// The disc's volume name, when there is one. Used as the history title
    /// so two discs from one drive do not both show up as `E:\`.
    pub label: Option<String>,
}

impl SourceKey {
    /// A source that is its own key: a file, a URL, a disc image.
    pub fn path(path: impl Into<String>) -> Self {
        let path = path.into();
        Self {
            key: path.clone(),
            path,
            label: None,
        }
    }

    /// Whether this key came from a disc's contents rather than its path.
    pub fn is_disc(&self) -> bool {
        self.key.starts_with(crate::core::disc::KEY_PREFIX)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaEntry {
    pub id: i64,
    /// What this row is matched by. Equal to `path` for everything except a
    /// mounted disc.
    pub key: String,
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
    /// The identity this bookmark belongs to. Equal to `path` for
    /// everything but a disc; `disc:…` for one.
    pub key: String,
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
    /// What to look the resume point up by. Equal to `path` except for a
    /// disc, where it says *which* disc was in the drive.
    pub key: String,
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

/// The `media` columns, in the one order `row_to_entry` reads.
const MEDIA_COLUMNS: &str = "id, key, path, title, duration, width, height, \
     video_codec, audio_codec, file_size, added_at, last_played, play_count";

/// Read a `media` row selected in the canonical column order.
///
/// Three queries wanted the same mapping; keeping one copy means a schema
/// change can't leave one of them reading the wrong column.
fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<MediaEntry> {
    Ok(MediaEntry {
        id: row.get(0)?,
        key: row.get(1)?,
        path: row.get(2)?,
        title: row.get(3)?,
        duration: row.get(4)?,
        width: row.get(5)?,
        height: row.get(6)?,
        video_codec: row.get(7)?,
        audio_codec: row.get(8)?,
        file_size: row.get(9)?,
        added_at: row.get(10)?,
        last_played: row.get(11)?,
        play_count: row.get(12)?,
    })
}

/// How close two bookmarks have to be, in seconds, before a new one is
/// treated as a correction of the old rather than a second place.
const BOOKMARK_MERGE_SECS: f64 = 1.0;

const BOOKMARK_COLUMNS: &str = "id, key, path, position, name, created_at";

fn row_to_bookmark(row: &rusqlite::Row<'_>) -> rusqlite::Result<Bookmark> {
    Ok(Bookmark {
        id: row.get(0)?,
        key: row.get(1)?,
        path: row.get(2)?,
        position: row.get(3)?,
        name: row.get(4)?,
        created_at: row.get(5)?,
    })
}

pub struct Database {
    conn: Mutex<Connection>,
}

/// Whether `table` already has `column`. This is what makes the migration
/// safe to run twice — it asks the database what it looks like rather than
/// trusting a version number that a half-finished upgrade could have lied
/// about.
fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut stmt = conn.prepare("SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2")?;
    Ok(stmt.exists(params![table, column])?)
}

/// Bumped whenever the shape below changes and `migrate` learns a step.
/// 0 is every database written before sources had an identity of their own.
const SCHEMA_VERSION: i64 = 1;

impl Database {
    pub fn open() -> Result<Self> {
        Self::open_at(&Self::db_path())
    }

    /// Open a database file by name.
    ///
    /// Public so a test can build one, seed it, and reopen it without going
    /// anywhere near `UNFLICK_DATA_DIR` — the migration below is the
    /// riskiest code in this file and had to be exercised against a real
    /// pre-migration database rather than reasoned about.
    pub fn open_at(db_path: &std::path::Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(db_path)?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.init_tables()?;
        db.migrate()?;
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
            -- `key` rather than `path` is what a row is found by. They hold
            -- the same string for a file, a URL or a disc image; for a
            -- mounted disc the path is the drive and the key is the disc.
            CREATE TABLE IF NOT EXISTS media (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                key TEXT NOT NULL UNIQUE,
                path TEXT NOT NULL,
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
                key TEXT PRIMARY KEY,
                path TEXT NOT NULL,
                position REAL NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS bookmark (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                key TEXT NOT NULL,
                path TEXT NOT NULL,
                position REAL NOT NULL,
                name TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            -- `idx_bookmark_key` is created by `migrate` rather than here:
            -- this batch also runs against a database that predates the
            -- column, where indexing it would fail before the migration
            -- that adds it ever got a chance to run.

            -- What was on screen when we last looked. One row, because
            -- there is one player; the CHECK is what keeps it that way
            -- rather than a convention nobody enforces.
            CREATE TABLE IF NOT EXISTS session (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                key TEXT NOT NULL,
                path TEXT NOT NULL,
                position REAL NOT NULL,
                duration REAL NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
        ",
        )?;
        Ok(())
    }

    /// Bring a database written before sources had identities up to date.
    ///
    /// Every existing row is backfilled `key = path`, which is precisely
    /// what it was already matched by, so nothing that worked stops working.
    ///
    /// What that means for the rows this change exists to fix — the ones
    /// written under a drive letter, where several discs' bookmarks are
    /// piled up under one `E:\` — is that they are *orphaned in place*.
    /// A key of `E:\` can never again be produced by a disc, since an
    /// identified disc is keyed `disc:…`, so those bookmarks stop being
    /// offered on the wrong film, which was the bug. They are not deleted
    /// and not merged into a synthetic "unknown disc": we cannot tell which
    /// disc each belonged to, a bookmark someone made by hand is not ours
    /// to throw away on a guess, and bucketing several discs' bookmarks
    /// under one invented identity would recreate the same collision under
    /// a new name. They stay reachable — `bookmark list --all` shows them,
    /// `bookmark list --key "E:\"` scopes to them, `bookmark clear --key`
    /// removes them — and a disc-scoped list says how many are there.
    ///
    /// All of it in one transaction guarded by `PRAGMA user_version`, so a
    /// crash halfway rolls back and a second open is a no-op.
    fn migrate(&self) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version >= SCHEMA_VERSION {
            return Ok(());
        }

        let tx = conn.transaction()?;

        // `media` and `playback_position` need their UNIQUE / PRIMARY KEY
        // moved off `path`, which SQLite will only do by rebuild. Row ids
        // are carried over explicitly so bookmarks and anything else
        // holding an id still point at the same row.
        if !has_column(&tx, "media", "key")? {
            tx.execute_batch(
                "
                ALTER TABLE media RENAME TO media_pre_key;
                CREATE TABLE media (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    key TEXT NOT NULL UNIQUE,
                    path TEXT NOT NULL,
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
                INSERT INTO media (id, key, path, title, duration, width, height,
                                   video_codec, audio_codec, file_size, added_at,
                                   last_played, play_count)
                    SELECT id, path, path, title, duration, width, height,
                           video_codec, audio_codec, file_size, added_at,
                           last_played, play_count
                    FROM media_pre_key;
                DROP TABLE media_pre_key;
                CREATE INDEX IF NOT EXISTS idx_media_path ON media(path);
                CREATE INDEX IF NOT EXISTS idx_media_title ON media(title);
                ",
            )?;
        }

        if !has_column(&tx, "playback_position", "key")? {
            tx.execute_batch(
                "
                ALTER TABLE playback_position RENAME TO playback_position_pre_key;
                CREATE TABLE playback_position (
                    key TEXT PRIMARY KEY,
                    path TEXT NOT NULL,
                    position REAL NOT NULL,
                    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                INSERT INTO playback_position (key, path, position, updated_at)
                    SELECT path, path, position, updated_at FROM playback_position_pre_key;
                DROP TABLE playback_position_pre_key;
                ",
            )?;
        }

        // These two only gain a column, so an ALTER does it. The DEFAULT is
        // required by SQLite for a NOT NULL column added to existing rows;
        // it is never the value anything writes.
        if !has_column(&tx, "bookmark", "key")? {
            tx.execute_batch(
                "
                ALTER TABLE bookmark ADD COLUMN key TEXT NOT NULL DEFAULT '';
                UPDATE bookmark SET key = path;
                ",
            )?;
        }
        if !has_column(&tx, "session", "key")? {
            tx.execute_batch(
                "
                ALTER TABLE session ADD COLUMN key TEXT NOT NULL DEFAULT '';
                UPDATE session SET key = path;
                ",
            )?;
        }

        // Bookmarks are looked up by key now, so the old index is dead
        // weight on every write.
        tx.execute_batch(
            "
            DROP INDEX IF EXISTS idx_bookmark_path;
            CREATE INDEX IF NOT EXISTS idx_bookmark_key ON bookmark(key);
            ",
        )?;

        tx.commit()?;
        // Outside the transaction: PRAGMA user_version is not transactional
        // in the way DDL is, and setting it last means a crash re-runs the
        // (idempotent, column-guarded) migration rather than skipping it.
        conn.execute_batch(&format!("PRAGMA user_version = {}", SCHEMA_VERSION))?;
        Ok(())
    }

    pub fn upsert_media(&self, entry: &MediaEntry) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO media (key, path, title, duration, width, height, video_codec, audio_codec, file_size)
             VALUES (?9, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(key) DO UPDATE SET
                path=?1, title=?2, duration=?3, width=?4, height=?5, video_codec=?6, audio_codec=?7, file_size=?8",
            params![
                entry.path,
                entry.title,
                entry.duration,
                entry.width,
                entry.height,
                entry.video_codec,
                entry.audio_codec,
                entry.file_size,
                entry.key
            ],
        )?;
        Ok(())
    }

    pub fn search(&self, query: &str) -> Result<Vec<MediaEntry>> {
        let conn = self.conn.lock().unwrap();
        let pattern = format!("%{}%", query);
        let mut stmt = conn.prepare(&format!(
            "SELECT {MEDIA_COLUMNS}
             FROM media WHERE title LIKE ?1 OR path LIKE ?1 ORDER BY title",
        ))?;
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
        let mut stmt = conn.prepare(&format!(
            "SELECT {MEDIA_COLUMNS} FROM media ORDER BY title",
        ))?;
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
    ///
    /// A disc's title is its volume label, because two discs out of one
    /// drive would otherwise both show up in `recent` as `E:\`. The path is
    /// refreshed on every play so a disc that moves from one drive to
    /// another still has somewhere to be opened from.
    pub fn record_play(&self, src: &SourceKey) -> Result<()> {
        let title = src.label.clone().unwrap_or_else(|| {
            std::path::Path::new(&src.path)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| src.path.clone())
        });

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO media (key, path, title, last_played, play_count)
             VALUES (?1, ?2, ?3, datetime('now'), 1)
             ON CONFLICT(key) DO UPDATE SET
                path = excluded.path,
                last_played = datetime('now'),
                play_count = play_count + 1",
            params![src.key, src.path, title],
        )?;
        Ok(())
    }

    /// Most recently played files, newest first. Entries that have never
    /// been played are excluded — this is a history, not the library.
    pub fn recent(&self, limit: usize) -> Result<Vec<MediaEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {MEDIA_COLUMNS}
             FROM media
             WHERE last_played IS NOT NULL
             ORDER BY last_played DESC
             LIMIT ?1",
        ))?;
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

    pub fn save_position(&self, src: &SourceKey, position: f64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO playback_position (key, path, position) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET
                path=excluded.path, position=?3, updated_at=datetime('now')",
            params![src.key, src.path, position],
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
    pub fn remember_position(&self, src: &SourceKey, position: f64, duration: f64) -> Result<()> {
        if position <= MIN_RESUME_SECS {
            return Ok(());
        }
        if is_finished(position, duration) {
            return self.clear_position(&src.key);
        }
        self.save_position(src, position)
    }

    pub fn get_position(&self, key: &str) -> Result<Option<f64>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT position FROM playback_position WHERE key = ?1")?;
        let result = stmt.query_row(params![key], |row| row.get(0)).ok();
        Ok(result)
    }

    pub fn clear_position(&self, key: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM playback_position WHERE key = ?1", params![key])?;
        Ok(())
    }

    /// The title `recent` would show for a key, when there is a row for it.
    /// Used to name a disc in the "wrong disc" refusals — "put THE MATRIX
    /// back in" beats "put disc:dvd:3f2a… back in".
    pub fn title_for_key(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT title FROM media WHERE key = ?1")?;
        Ok(stmt.query_row(params![key], |row| row.get(0)).ok())
    }

    // ─── Session ──────────────────────────────────────────────────────────

    /// Remember what is on screen, so a later launch can offer it back.
    ///
    /// Separate from `playback_position` on purpose. That table answers
    /// "if this file is opened again, where does it start" — one row per
    /// file, and a row survives forever. This answers "what was the user
    /// watching", which is one thing at a time and stops being true the
    /// moment they stop watching it.
    pub fn set_session(&self, src: &SourceKey, position: f64, duration: f64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO session (id, key, path, position, duration) VALUES (1, ?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
                 key=?1, path=?2, position=?3, duration=?4, updated_at=datetime('now')",
            params![src.key, src.path, position, duration],
        )?;
        Ok(())
    }

    pub fn get_session(&self) -> Result<Option<Session>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT key, path, position, duration, updated_at FROM session WHERE id = 1",
        )?;
        let row = stmt
            .query_row([], |row| {
                Ok(Session {
                    key: row.get(0)?,
                    path: row.get(1)?,
                    position: row.get(2)?,
                    duration: row.get(3)?,
                    updated_at: row.get(4)?,
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
    pub fn add_bookmark(
        &self,
        src: &SourceKey,
        position: f64,
        name: Option<&str>,
    ) -> Result<Bookmark> {
        let position = position.max(0.0);
        let conn = self.conn.lock().unwrap();

        // Matched on `key`: two discs sharing a drive letter share a path,
        // and merging across them would be the original bug wearing a hat.
        let existing: Option<i64> = conn
            .query_row(
                "SELECT id FROM bookmark
                 WHERE key = ?1 AND abs(position - ?2) <= ?3
                 ORDER BY abs(position - ?2) LIMIT 1",
                params![src.key, position, BOOKMARK_MERGE_SECS],
                |row| row.get(0),
            )
            .ok();

        let id = match existing {
            Some(id) => {
                conn.execute(
                    "UPDATE bookmark SET position = ?2, name = COALESCE(?3, name), path = ?4
                     WHERE id = ?1",
                    params![id, position, name, src.path],
                )?;
                id
            }
            None => {
                conn.execute(
                    "INSERT INTO bookmark (key, path, position, name) VALUES (?1, ?2, ?3, ?4)",
                    params![src.key, src.path, position, name],
                )?;
                conn.last_insert_rowid()
            }
        };

        Self::read_bookmark(&conn, id)?
            .ok_or_else(|| anyhow::anyhow!("bookmark {} vanished after writing it", id))
    }

    /// Bookmarks for one source, or for everything when `key` is `None`.
    ///
    /// Ordered by position within a source so the list reads like the
    /// timeline, not like the order they happened to be made in. Grouped by
    /// key rather than path so two discs sharing a drive letter come back as
    /// two runs instead of interleaved.
    pub fn list_bookmarks(&self, key: Option<&str>) -> Result<Vec<Bookmark>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {BOOKMARK_COLUMNS} FROM bookmark
             WHERE ?1 IS NULL OR key = ?1
             ORDER BY key, position",
        ))?;
        let rows = stmt.query_map(params![key], row_to_bookmark)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// How many bookmarks sit under one key.
    ///
    /// Exists for the orphan note: when a disc is asked about, this counts
    /// what is still filed under the drive it is in — bookmarks made before
    /// discs had an identity — so the answer can say they are there and how
    /// to reach them, rather than letting them quietly disappear.
    pub fn count_bookmarks(&self, key: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT count(*) FROM bookmark WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )?;
        Ok(n as usize)
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

    /// Delete every bookmark for one source, or all of them when `key` is
    /// `None`. Returns how many went.
    pub fn clear_bookmarks(&self, key: Option<&str>) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute(
            "DELETE FROM bookmark WHERE ?1 IS NULL OR key = ?1",
            params![key],
        )?)
    }

    fn read_bookmark(conn: &Connection, id: i64) -> Result<Option<Bookmark>> {
        let mut stmt =
            conn.prepare(&format!("SELECT {BOOKMARK_COLUMNS} FROM bookmark WHERE id = ?1"))?;
        Ok(stmt.query_row(params![id], row_to_bookmark).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway database file. Named per test rather than shared, since
    /// `cargo test` runs these concurrently.
    struct TempDb(PathBuf);

    impl TempDb {
        fn new(name: &str) -> Self {
            let p = std::env::temp_dir().join(format!("unflick-db-{}.sqlite", name));
            let _ = std::fs::remove_file(&p);
            Self(p)
        }
        fn open(&self) -> Database {
            Database::open_at(&self.0).expect("open database")
        }
    }

    impl Drop for TempDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn disc_key(which: &str) -> SourceKey {
        SourceKey {
            key: format!("disc:dvd:{}", which),
            path: r"E:\".to_string(),
            label: Some(format!("DISC {}", which)),
        }
    }

    /// The schema exactly as it was before sources had identities, with a
    /// row in every table filed under a drive letter.
    ///
    /// Written out by hand rather than checked in as a fixture, so the shape
    /// this migrates *from* is readable next to the migration itself.
    fn seed_pre_migration(path: &std::path::Path) {
        let conn = Connection::open(path).expect("create legacy database");
        conn.execute_batch(
            "
            CREATE TABLE media (
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
            CREATE INDEX idx_media_path ON media(path);
            CREATE INDEX idx_media_title ON media(title);
            CREATE TABLE playback_position (
                path TEXT PRIMARY KEY,
                position REAL NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE bookmark (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL,
                position REAL NOT NULL,
                name TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX idx_bookmark_path ON bookmark(path);
            CREATE TABLE session (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                path TEXT NOT NULL,
                position REAL NOT NULL,
                duration REAL NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            INSERT INTO media (id, path, title, last_played, play_count)
                VALUES (7, 'E:\\', 'E', datetime('now'), 3);
            INSERT INTO media (id, path, title) VALUES (8, '/films/ordinary.mkv', 'ordinary');
            INSERT INTO playback_position (path, position) VALUES ('E:\\', 640.0);
            INSERT INTO playback_position (path, position) VALUES ('/films/ordinary.mkv', 12.0);
            INSERT INTO bookmark (id, path, position, name)
                VALUES (1, 'E:\\', 300.0, 'the good bit');
            INSERT INTO bookmark (id, path, position, name)
                VALUES (2, '/films/ordinary.mkv', 5.0, NULL);
            INSERT INTO session (id, path, position, duration) VALUES (1, 'E:\\', 640.0, 5400.0);
            ",
        )
        .expect("seed legacy rows");
    }

    #[test]
    fn a_database_written_before_identities_keeps_every_row() {
        let tmp = TempDb::new("migrate-keeps");
        seed_pre_migration(&tmp.0);
        let db = tmp.open();

        // Backfilled key = path, which is precisely what each row was
        // already matched by — so nothing that worked stops working.
        let media = db.list_all().unwrap();
        assert_eq!(media.len(), 2);
        for entry in &media {
            assert_eq!(entry.key, entry.path, "key should be backfilled from path");
        }
        // Row ids survive the table rebuild: anything holding one still
        // points at the same row.
        assert!(media.iter().any(|m| m.id == 7 && m.path == r"E:\"));
        assert!(media.iter().any(|m| m.id == 8));

        assert_eq!(db.get_position(r"E:\").unwrap(), Some(640.0));
        assert_eq!(db.get_position("/films/ordinary.mkv").unwrap(), Some(12.0));

        let all = db.list_bookmarks(None).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().all(|b| b.key == b.path));

        let session = db.get_session().unwrap().expect("session survives");
        assert_eq!(session.key, r"E:\");
        assert_eq!(session.path, r"E:\");
        assert_eq!(session.duration, 5400.0);
    }

    #[test]
    fn a_drive_letter_bookmark_is_orphaned_not_deleted() {
        // The honest outcome: we cannot tell which disc it belonged to, so
        // it is not thrown away and not filed under an invented identity.
        // It stops being offered on the wrong disc, and stays reachable.
        let tmp = TempDb::new("migrate-orphan");
        seed_pre_migration(&tmp.0);
        let db = tmp.open();

        let disc = disc_key("aaaa");
        assert!(
            db.list_bookmarks(Some(&disc.key)).unwrap().is_empty(),
            "a disc must not inherit the drive letter's bookmarks"
        );

        let under_drive = db.list_bookmarks(Some(r"E:\")).unwrap();
        assert_eq!(under_drive.len(), 1);
        assert_eq!(under_drive[0].name.as_deref(), Some("the good bit"));
        assert_eq!(db.count_bookmarks(r"E:\").unwrap(), 1);

        // And it can be got rid of deliberately, which is the other half of
        // orphaning rather than hiding.
        assert_eq!(db.clear_bookmarks(Some(r"E:\")).unwrap(), 1);
        assert_eq!(db.count_bookmarks(r"E:\").unwrap(), 0);
    }

    #[test]
    fn the_migration_runs_once_and_changes_nothing_the_second_time() {
        let tmp = TempDb::new("migrate-idempotent");
        seed_pre_migration(&tmp.0);

        let read = || {
            let db = tmp.open();
            (
                db.list_all().unwrap(),
                db.list_bookmarks(None).unwrap(),
                db.get_session().unwrap(),
            )
        };
        let before = read();
        let after = read();

        assert_eq!(before.0.len(), after.0.len());
        for (a, b) in before.0.iter().zip(after.0.iter()) {
            assert_eq!((a.id, &a.key, &a.path), (b.id, &b.key, &b.path));
        }
        assert_eq!(before.1.len(), after.1.len());
        for (a, b) in before.1.iter().zip(after.1.iter()) {
            assert_eq!((a.id, &a.key), (b.id, &b.key));
        }
        assert_eq!(before.2, after.2);
    }

    #[test]
    fn two_discs_from_one_drive_are_two_entries_in_the_history() {
        // The old UNIQUE(path) made this impossible, which is why a drive's
        // whole history was one row that kept being overwritten.
        let tmp = TempDb::new("history-two-discs");
        let db = tmp.open();

        db.record_play(&disc_key("1111")).unwrap();
        db.record_play(&disc_key("2222")).unwrap();

        let recent = db.recent(10).unwrap();
        assert_eq!(recent.len(), 2, "got {:?}", recent);
        assert!(recent.iter().all(|e| e.path == r"E:\"));
        // Titled by the volume label rather than by the drive, so a person
        // reading the list can tell which is which.
        let titles: Vec<_> = recent.iter().map(|e| e.title.clone()).collect();
        assert!(titles.contains(&"DISC 1111".to_string()), "{:?}", titles);
        assert!(titles.contains(&"DISC 2222".to_string()), "{:?}", titles);
    }

    #[test]
    fn a_resume_point_belongs_to_the_disc_not_the_drive() {
        let tmp = TempDb::new("resume-per-disc");
        let db = tmp.open();

        db.save_position(&disc_key("aaaa"), 640.0).unwrap();
        assert_eq!(db.get_position("disc:dvd:bbbb").unwrap(), None);
        assert_eq!(db.get_position("disc:dvd:aaaa").unwrap(), Some(640.0));

        // And the drive it happened to be in is not itself a resume point.
        assert_eq!(db.get_position(r"E:\").unwrap(), None);
    }

    #[test]
    fn a_disc_that_moves_drives_keeps_a_path_that_opens_it() {
        let tmp = TempDb::new("disc-moves");
        let db = tmp.open();

        let mut src = disc_key("aaaa");
        db.record_play(&src).unwrap();
        db.save_position(&src, 100.0).unwrap();

        src.path = r"F:\".to_string();
        db.record_play(&src).unwrap();
        db.save_position(&src, 200.0).unwrap();

        let recent = db.recent(10).unwrap();
        assert_eq!(recent.len(), 1, "still one disc");
        assert_eq!(recent[0].path, r"F:\", "the path follows the disc");
        assert_eq!(recent[0].play_count, 2);
        assert_eq!(db.get_position("disc:dvd:aaaa").unwrap(), Some(200.0));
    }

    #[test]
    fn a_file_and_an_image_are_keyed_by_their_paths_exactly_as_before() {
        let tmp = TempDb::new("paths-unchanged");
        let db = tmp.open();

        for path in ["/films/ordinary.mkv", "/films/disc one.iso"] {
            let src = SourceKey::path(path);
            let b = db.add_bookmark(&src, 30.0, Some("here")).unwrap();
            assert_eq!(b.key, path);
            assert_eq!(b.path, path);
            db.save_position(&src, 30.0).unwrap();
            assert_eq!(db.get_position(path).unwrap(), Some(30.0));
        }

        // And the two do not see each other, which is the same guarantee
        // this change is adding for discs.
        assert_eq!(db.list_bookmarks(Some("/films/ordinary.mkv")).unwrap().len(), 1);
        assert_eq!(db.list_bookmarks(Some("/films/disc one.iso")).unwrap().len(), 1);
    }

    #[test]
    fn bookmarks_merge_within_a_disc_and_never_across_two() {
        let tmp = TempDb::new("merge-per-disc");
        let db = tmp.open();

        let a = db.add_bookmark(&disc_key("aaaa"), 300.0, None).unwrap();
        let a_again = db
            .add_bookmark(&disc_key("aaaa"), 300.3, Some("same spot"))
            .unwrap();
        assert_eq!(a.id, a_again.id, "a repeated keypress is one bookmark");

        // Same path, same timestamp, different disc: a different bookmark,
        // because it is a different film.
        let b = db.add_bookmark(&disc_key("bbbb"), 300.0, None).unwrap();
        assert_ne!(a.id, b.id);
        assert_eq!(db.list_bookmarks(Some("disc:dvd:aaaa")).unwrap().len(), 1);
        assert_eq!(db.list_bookmarks(Some("disc:dvd:bbbb")).unwrap().len(), 1);
    }
}
