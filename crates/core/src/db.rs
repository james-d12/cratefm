use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};

use crate::models::{Release, ReleaseRow, ReleaseStatus, Video, VideoRow};

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.init()?;
        Ok(db)
    }

    fn init(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS releases (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                discogs_id TEXT    UNIQUE NOT NULL,
                title      TEXT,
                artist     TEXT,
                year       INTEGER,
                genre      TEXT,
                style      TEXT,
                rating     REAL,
                owners     INTEGER,
                url        TEXT,
                status     TEXT    NOT NULL DEFAULT 'to_listen',
                fetched_at TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS videos (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                release_id INTEGER NOT NULL REFERENCES releases(id),
                title      TEXT,
                url        TEXT UNIQUE
            );
            ",
        )?;
        Ok(())
    }

    /// Returns all discogs_ids already stored, regardless of status.
    pub fn known_ids(&self) -> Result<std::collections::HashSet<String>> {
        let mut stmt = self.conn.prepare("SELECT discogs_id FROM releases")?;
        let ids = stmt
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(ids)
    }

    pub fn save_releases(&self, records: &[crate::discogs::PendingRelease]) -> Result<()> {
        let now = chrono::Utc::now().naive_utc().to_string();
        let mut stmt = self.conn.prepare(
            "INSERT OR IGNORE INTO releases
                (discogs_id, title, artist, year, genre, style, rating, owners, url, status, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'to_listen', ?10)",
        )?;
        for r in records {
            stmt.execute(params![
                r.discogs_id, r.title, r.artist, r.year,
                r.genre, r.style, r.rating, r.owners, r.url, now
            ])?;
        }
        Ok(())
    }

    pub fn save_videos(&self, records: &[crate::discogs::PendingVideo]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }

        let discogs_ids: Vec<&str> = records.iter().map(|r| r.discogs_id.as_str()).collect();
        let placeholders = discogs_ids.iter().enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(", ");

        let sql = format!(
            "SELECT discogs_id, id FROM releases WHERE discogs_id IN ({placeholders})"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let id_map: std::collections::HashMap<String, i64> = stmt
            .query_map(rusqlite::params_from_iter(discogs_ids.iter()), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        let mut stmt = self.conn.prepare(
            "INSERT OR IGNORE INTO videos (release_id, title, url) VALUES (?1, ?2, ?3)",
        )?;
        for v in records {
            if let Some(&release_id) = id_map.get(&v.discogs_id) {
                stmt.execute(params![release_id, v.title, v.url])?;
            }
        }
        Ok(())
    }

    pub fn list_releases(&self, status: &ReleaseStatus) -> Result<Vec<ReleaseRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT r.id, r.discogs_id, r.title, r.artist, r.year,
                    r.genre, r.style, r.rating, r.owners, r.url, r.status, r.fetched_at,
                    COUNT(v.id) as video_count
             FROM releases r
             LEFT JOIN videos v ON v.release_id = r.id
             WHERE r.status = ?1
             GROUP BY r.id
             ORDER BY r.rating DESC",
        )?;
        let rows = stmt
            .query_map(params![status.to_string()], |row| {
                Ok(ReleaseRow {
                    release: Release {
                        id: row.get(0)?,
                        discogs_id: row.get(1)?,
                        title: row.get(2)?,
                        artist: row.get(3)?,
                        year: row.get(4)?,
                        genre: row.get(5)?,
                        style: row.get(6)?,
                        rating: row.get(7)?,
                        owners: row.get(8)?,
                        url: row.get(9)?,
                        status: row.get::<_, String>(10)?.parse().unwrap(),
                        fetched_at: row.get(11)?,
                    },
                    video_count: row.get(12)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn mark_release(&self, id: i64, status: &ReleaseStatus) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE releases SET status = ?1 WHERE id = ?2",
            params![status.to_string(), id],
        )?;
        Ok(n > 0)
    }

    /// Fetch up to `limit` to_listen releases that have at least one video, best rated first.
    pub fn next_listen_batch(&self, limit: usize) -> Result<Vec<Release>> {
        let mut stmt = self.conn.prepare(
            "SELECT r.id, r.discogs_id, r.title, r.artist, r.year,
                    r.genre, r.style, r.rating, r.owners, r.url, r.status, r.fetched_at
             FROM releases r
             WHERE r.status = 'to_listen'
               AND EXISTS (SELECT 1 FROM videos v WHERE v.release_id = r.id)
             ORDER BY r.rating DESC
             LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok(Release {
                    id: row.get(0)?,
                    discogs_id: row.get(1)?,
                    title: row.get(2)?,
                    artist: row.get(3)?,
                    year: row.get(4)?,
                    genre: row.get(5)?,
                    style: row.get(6)?,
                    rating: row.get(7)?,
                    owners: row.get(8)?,
                    url: row.get(9)?,
                    status: row.get::<_, String>(10)?.parse().unwrap(),
                    fetched_at: row.get(11)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn first_video_url(&self, release_id: i64) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT url FROM videos WHERE release_id = ?1 LIMIT 1")?;
        let url = stmt
            .query_row(params![release_id], |row| row.get(0))
            .optional()?;
        Ok(url)
    }

    pub fn list_all_videos(&self) -> Result<Vec<VideoRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT v.id, v.release_id, v.title, v.url, r.title, r.artist
             FROM videos v
             JOIN releases r ON r.id = v.release_id
             ORDER BY r.artist, r.title, v.title",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(VideoRow {
                    video: Video {
                        id: row.get(0)?,
                        release_id: row.get(1)?,
                        title: row.get(2)?,
                        url: row.get(3)?,
                    },
                    release_title: row.get(4)?,
                    release_artist: row.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn delete_video_by_url(&self, url: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM videos WHERE url = ?1", params![url])?;
        Ok(())
    }
}
