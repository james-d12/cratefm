use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseStatus {
    ToListen,
    Liked,
    Disliked,
}

impl std::fmt::Display for ReleaseStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReleaseStatus::ToListen => write!(f, "to_listen"),
            ReleaseStatus::Liked => write!(f, "liked"),
            ReleaseStatus::Disliked => write!(f, "disliked"),
        }
    }
}

impl std::str::FromStr for ReleaseStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "to_listen" => Ok(ReleaseStatus::ToListen),
            "liked" => Ok(ReleaseStatus::Liked),
            "disliked" => Ok(ReleaseStatus::Disliked),
            other => Err(anyhow::anyhow!("unknown status: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Release {
    pub id: i64,
    pub discogs_id: String,
    pub title: String,
    pub artist: String,
    pub year: Option<i32>,
    pub genre: String,
    pub style: String,
    pub rating: f64,
    pub owners: i64,
    pub url: String,
    pub fetched_at: NaiveDateTime,
}

/// A video with its own listen status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Video {
    pub id: i64,
    pub release_id: i64,
    pub title: String,
    pub url: String,
    pub status: ReleaseStatus,
}

/// A release joined with per-status video counts, used for the `list` command.
#[derive(Debug, Clone)]
pub struct ReleaseRow {
    pub release: Release,
    pub to_listen_count: i64,
    pub liked_count: i64,
    pub disliked_count: i64,
}

impl ReleaseRow {
    pub fn video_count(&self) -> i64 {
        self.to_listen_count + self.liked_count + self.disliked_count
    }
}

/// A video joined with its release info, used for the listen queue and video list.
#[derive(Debug, Clone)]
pub struct VideoRow {
    pub video: Video,
    pub release_title: String,
    pub release_artist: String,
}

/// A video with full release context, used for the listen session.
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

/// Parameters for the `fetch` command.
#[derive(Debug, Clone)]
pub struct FetchParams {
    pub token: String,
    pub genre: String,
    pub style: String,
    pub year: u32,
    pub limit: usize,
    pub min_owners: u64,
    pub max_owners: u64,
    pub min_rating: Option<f64>,
}
