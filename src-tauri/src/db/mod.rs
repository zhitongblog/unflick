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
        let mut path = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("unflick");
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
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(entries)
    }

    pub fn list_all(&self) -> Result<Vec<MediaEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, path, title, duration, width, height, video_codec, audio_codec, file_size, added_at, last_played, play_count
             FROM media ORDER BY title",
        )?;
        let entries = stmt
            .query_map([], |row| {
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
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(entries)
    }

    pub fn record_play(&self, path: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE media SET last_played = datetime('now'), play_count = play_count + 1 WHERE path = ?1",
            params![path],
        )?;
        Ok(())
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

    pub fn remove(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM media WHERE id = ?1", params![id])?;
        Ok(())
    }
}
