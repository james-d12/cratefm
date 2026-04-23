use crate::discogs::models::{PendingImage, PendingRelease, PendingVideo};
use crate::models::{
    Image, ImageRow, ListenVideo, Release, ReleaseRow, ReleaseStatus, Video, VideoRow,
};
use anyhow::Result;
use rusqlite::{Connection, params};

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
                fetched_at TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS images (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                release_id   INTEGER NOT NULL REFERENCES releases(id),
                url          TEXT UNIQUE,
                width        INTEGER,
                height       INTEGER,
                image_type   TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS videos (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                release_id INTEGER NOT NULL REFERENCES releases(id),
                title      TEXT,
                url        TEXT UNIQUE,
                status     TEXT NOT NULL DEFAULT 'to_listen'
            );

            CREATE TABLE IF NOT EXISTS fetch_cursors (
                genre      TEXT    NOT NULL,
                style      TEXT    NOT NULL,
                year       INTEGER NOT NULL,
                next_page  INTEGER NOT NULL DEFAULT 1,
                updated_at TIMESTAMP,
                PRIMARY KEY (genre, style, year)
            );
            ",
        )?;
        Ok(())
    }

    /// Returns all discogs_ids already stored.
    pub fn known_ids(&self) -> Result<std::collections::HashSet<String>> {
        let mut stmt = self.conn.prepare("SELECT discogs_id FROM releases")?;
        let ids = stmt
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(ids)
    }

    /// List releases, optionally filtered to those that have at least one video
    /// with the given status. `None` returns all releases.
    pub fn list_releases(&self, video_status: Option<&ReleaseStatus>) -> Result<Vec<ReleaseRow>> {
        let filter_clause = match video_status {
            Some(s) => format!(
                "WHERE EXISTS (SELECT 1 FROM videos v2 WHERE v2.release_id = r.id AND v2.status = '{}')",
                s
            ),
            None => String::new(),
        };

        let sql = format!(
            "SELECT r.id, r.discogs_id, r.title, r.artist, r.year,
                    r.genre, r.style, r.rating, r.owners, r.url, r.fetched_at,
                    COUNT(CASE WHEN v.status = 'to_listen' THEN 1 END) AS to_listen_count,
                    COUNT(CASE WHEN v.status = 'liked'     THEN 1 END) AS liked_count,
                    COUNT(CASE WHEN v.status = 'disliked'  THEN 1 END) AS disliked_count
             FROM releases r
             LEFT JOIN videos v ON v.release_id = r.id
             {filter_clause}
             GROUP BY r.id
             ORDER BY r.rating DESC"
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map([], |row| {
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
                        fetched_at: row.get(10)?,
                    },
                    to_listen_count: row.get(11)?,
                    liked_count: row.get(12)?,
                    disliked_count: row.get(13)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// List all videos joined with their release info.
    pub fn list_all_videos(&self) -> Result<Vec<VideoRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT v.id, v.release_id, v.title, v.url, v.status, r.title, r.artist
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
                        status: row.get::<_, String>(4)?.parse().unwrap(),
                    },
                    release_title: row.get(5)?,
                    release_artist: row.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn list_all_images(&self) -> Result<Vec<ImageRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT i.id, i.release_id, i.url, i.width, i.height, i.image_type, r.title, r.artist
             FROM images i
             JOIN releases r ON r.id = i.release_id
             ORDER BY r.artist, r.title",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ImageRow {
                    image: Image {
                        id: row.get(0)?,
                        release_id: row.get(1)?,
                        url: row.get(2)?,
                        width: row.get(3)?,
                        height: row.get(4)?,
                        image_type: row.get(5)?,
                    },
                    release_title: row.get(6)?,
                    release_artist: row.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Return up to `limit` unrated (to_listen) videos, best-rated releases first.
    /// Each video carries enough release context to display a listen card.
    /// If `style` is `Some(s)` with a non-empty string, only videos whose release
    /// style matches (case-insensitive) are returned.
    pub fn next_listen_videos(
        &self,
        limit: usize,
        style: Option<&str>,
    ) -> Result<Vec<ListenVideo>> {
        let style_filter = style.filter(|s| !s.is_empty());

        let sql = format!(
            "SELECT v.id, v.title, v.url,
                    r.id, r.title, r.artist, r.year,
                    r.genre, r.style, r.rating, r.owners
             FROM videos v
             JOIN releases r ON r.id = v.release_id
             WHERE v.status = 'to_listen'{}
             ORDER BY r.rating DESC, r.id, v.id
             LIMIT ?1",
            if style_filter.is_some() {
                " AND r.style LIKE ?2"
            } else {
                ""
            }
        );

        let map_row = |row: &rusqlite::Row<'_>| {
            Ok(ListenVideo {
                video_id: row.get(0)?,
                video_title: row.get(1)?,
                video_url: row.get(2)?,
                release_id: row.get(3)?,
                release_title: row.get(4)?,
                release_artist: row.get(5)?,
                release_year: row.get(6)?,
                release_genre: row.get(7)?,
                release_style: row.get(8)?,
                release_rating: row.get(9)?,
                release_owners: row.get(10)?,
            })
        };

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = if let Some(s) = style_filter {
            stmt.query_map(params![limit as i64, s], map_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            stmt.query_map(params![limit as i64], map_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        Ok(rows)
    }

    // ── Write helpers ─────────────────────────────────────────────────────────

    pub fn save_releases(&self, records: &[PendingRelease]) -> Result<()> {
        let now = chrono::Utc::now().naive_utc().to_string();
        let mut stmt = self.conn.prepare(
            "INSERT OR IGNORE INTO releases
                (discogs_id, title, artist, year, genre, style, rating, owners, url, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?;
        for r in records {
            stmt.execute(params![
                r.discogs_id,
                r.title,
                r.artist,
                r.year,
                r.genre,
                r.style,
                r.rating,
                r.owners,
                r.url,
                now
            ])?;
        }
        Ok(())
    }

    pub fn save_videos(&self, records: &[PendingVideo]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }

        let discogs_ids: Vec<&str> = records.iter().map(|r| r.discogs_id.as_str()).collect();
        let placeholders = discogs_ids
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(", ");

        let sql =
            format!("SELECT discogs_id, id FROM releases WHERE discogs_id IN ({placeholders})");
        let mut stmt = self.conn.prepare(&sql)?;
        let id_map: std::collections::HashMap<String, i64> = stmt
            .query_map(rusqlite::params_from_iter(discogs_ids.iter()), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        let mut stmt = self
            .conn
            .prepare("INSERT OR IGNORE INTO videos (release_id, title, url) VALUES (?1, ?2, ?3)")?;
        for v in records {
            if let Some(&release_id) = id_map.get(&v.discogs_id) {
                stmt.execute(params![release_id, v.title, v.url])?;
            }
        }
        Ok(())
    }

    pub fn save_images(&self, records: &[PendingImage]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }

        let discogs_ids: Vec<&str> = records.iter().map(|r| r.discogs_id.as_str()).collect();
        let placeholders = discogs_ids
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(", ");

        let sql =
            format!("SELECT discogs_id, id FROM releases WHERE discogs_id IN ({placeholders})");
        let mut stmt = self.conn.prepare(&sql)?;
        let id_map: std::collections::HashMap<String, i64> = stmt
            .query_map(rusqlite::params_from_iter(discogs_ids.iter()), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        let mut stmt = self.conn.prepare(
            "INSERT OR IGNORE INTO images (release_id, url, width, height, image_type) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for v in records {
            if let Some(&release_id) = id_map.get(&v.discogs_id) {
                stmt.execute(params![release_id, v.url, v.width, v.height, v.image_type])?;
            }
        }
        Ok(())
    }

    /// Update the status of a single video. Returns `true` if a row was updated.
    pub fn mark_video(&self, video_id: i64, status: &ReleaseStatus) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE videos SET status = ?1 WHERE id = ?2",
            params![status.to_string(), video_id],
        )?;
        Ok(n > 0)
    }

    pub fn delete_video_by_url(&self, url: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM videos WHERE url = ?1", params![url])?;
        Ok(())
    }

    // ── Fetch cursors ─────────────────────────────────────────────────────────

    /// Returns the next page to fetch for this (genre, style, year) query.
    /// Defaults to page 1 if no cursor has been stored yet.
    pub fn get_cursor(&self, genre: &str, style: &str, year: u32) -> Result<u32> {
        match self.conn.query_row(
            "SELECT next_page FROM fetch_cursors WHERE genre = ?1 AND style = ?2 AND year = ?3",
            params![genre, style, year],
            |row| row.get(0),
        ) {
            Ok(page) => Ok(page),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(1),
            Err(e) => Err(e.into()),
        }
    }

    /// Persists the next page to fetch for this (genre, style, year) query.
    pub fn set_cursor(&self, genre: &str, style: &str, year: u32, next_page: u32) -> Result<()> {
        let now = chrono::Utc::now().naive_utc().to_string();
        self.conn.execute(
            "INSERT INTO fetch_cursors (genre, style, year, next_page, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(genre, style, year) DO UPDATE SET next_page = excluded.next_page, updated_at = excluded.updated_at",
            params![genre, style, year, next_page, now],
        )?;
        Ok(())
    }
}
