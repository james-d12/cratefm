#[derive(Debug, Clone)]
pub struct PendingRelease {
    pub discogs_id: String,
    pub title: String,
    pub artist: String,
    pub year: Option<i32>,
    pub genre: String,
    pub style: String,
    pub rating: f64,
    pub owners: i64,
    pub url: String,
}

/// A video ready to be persisted.
#[derive(Debug, Clone)]
pub struct PendingVideo {
    pub discogs_id: String,
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct PendingImage {
    pub discogs_id: String,
    pub height: i32,
    pub width: i32,
    pub url: String,
    pub image_type: String,
}

pub struct Releases {
    pub releases: Vec<PendingRelease>,
    pub videos: Vec<PendingVideo>,
    pub images: Vec<PendingImage>,
    pub next_page: u32,
}

#[derive(Debug, Clone)]
pub struct FetchParams {
    pub token: String,
    pub genre: String,
    pub style: String,
    pub year: u32,
    pub limit: usize,
    pub min_owners: u64,
    pub max_owners: Option<u64>,
    pub min_rating: Option<f64>,
}
