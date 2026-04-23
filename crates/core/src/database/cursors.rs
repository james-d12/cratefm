use crate::database::Db;
use anyhow::Result;
use rusqlite::params;

impl Db {
    pub fn init_cursors(&self) -> Result<()> {
        self.conn.execute_batch(
            "
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
