use crate::database::Db;
use crate::discogs::models::PendingImage;
use crate::models::{Image, ImageRow};
use anyhow::Result;
use rusqlite::params;

impl Db {
    pub fn init_images(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS images (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                release_id   INTEGER NOT NULL REFERENCES releases(id),
                url          TEXT UNIQUE,
                width        INTEGER,
                height       INTEGER,
                image_type   TEXT NOT NULL
            );
            ",
        )?;
        Ok(())
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
}
