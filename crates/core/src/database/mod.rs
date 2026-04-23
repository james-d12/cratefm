use rusqlite::Connection;

pub mod cursors;
pub mod images;
pub mod releases;
pub mod videos;

pub struct Db {
    pub(crate) conn: Connection,
}

impl Db {
    pub fn open(path: &str) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.init_releases()?;
        db.init_images()?;
        db.init_videos()?;
        db.init_cursors()?;
        Ok(db)
    }
}
