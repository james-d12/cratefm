use crate::database::Db;
use crate::discogs::models::PendingRelease;
use crate::models::{Release, ReleaseRow, ReleaseStatus};
use anyhow::Result;
use rusqlite::params;

impl Db {
    pub fn init_releases(&self) -> Result<()> {
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
}
