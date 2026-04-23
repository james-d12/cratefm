use crate::database::Db;
use crate::database::releases::ReleaseStatus;
use crate::discogs::models::PendingVideo;
use anyhow::Result;
use rusqlite::params;
use serde::{Deserialize, Serialize};

/// A video with its own listen status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Video {
    pub id: i64,
    pub release_id: i64,
    pub title: String,
    pub url: String,
    pub status: ReleaseStatus,
}

/// A video joined with its release info, used for the listen queue and video list.
#[derive(Debug, Clone)]
pub struct VideoRow {
    pub video: Video,
    pub release_title: String,
    pub release_artist: String,
}

#[derive(Debug, Clone)]
pub struct ListenVideo {
    pub video_id: i64,
    pub video_title: String,
    pub video_url: String,
    pub release_id: i64,
    pub release_title: String,
    pub release_artist: String,
    pub release_year: Option<i32>,
    pub release_genre: String,
    pub release_style: String,
    pub release_rating: f64,
    pub release_owners: i64,
}

impl Db {
    pub fn init_videos(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS videos (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                release_id INTEGER NOT NULL REFERENCES releases(id),
                title      TEXT,
                url        TEXT UNIQUE,
                status     TEXT NOT NULL DEFAULT 'to_listen'
            );
            ",
        )?;
        Ok(())
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
}
