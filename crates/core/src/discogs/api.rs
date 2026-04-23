use crate::discogs::models::FetchParams;
use reqwest::Client;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub pagination: Pagination,
}

#[derive(Deserialize, Debug)]
pub struct Pagination {
    #[allow(dead_code)]
    page: u32,
    pub pages: u32,
}

#[derive(Deserialize, Debug)]
pub struct SearchResult {
    pub id: u64,
    #[serde(default)]
    pub community: SearchCommunity,
    #[serde(default)]
    pub formats: Vec<Format>,
    pub style: Vec<String>
}

#[derive(Deserialize, Debug, Default)]
pub struct SearchCommunity {
    #[serde(default)]
    pub(crate) have: i64,
}

#[derive(Deserialize, Debug, Default)]
pub struct Format {
    #[serde(default)]
    pub(crate) descriptions: Vec<String>,
}

#[derive(Deserialize, Debug)]
pub struct ReleaseDetail {
    pub(crate) title: String,
    pub(crate) year: Option<i32>,
    pub(crate) uri: String,
    #[serde(default)]
    pub(crate) artists: Vec<Artist>,
    pub(crate) styles: Vec<String>,
    #[serde(default)]
    pub(crate) community: ReleaseCommunity,
    #[serde(default)]
    pub(crate) videos: Vec<VideoDetail>,
    pub(crate) images: Vec<ReleaseImage>,
}

#[derive(Deserialize, Debug)]
pub struct ReleaseImage {
    pub(crate) height: i32,
    pub(crate) width: i32,
    pub(crate) resource_url: String,
    #[serde(rename = "type")]
    pub(crate) image_type: String,
}

#[derive(Deserialize, Debug)]
pub struct Artist {
    pub(crate) name: String,
}

#[derive(Deserialize, Debug, Default)]
pub struct ReleaseCommunity {
    #[serde(default)]
    pub(crate) rating: Rating,
}

#[derive(Deserialize, Debug, Default)]
pub struct Rating {
    #[serde(default)]
    pub(crate) average: f64,
}

#[derive(Deserialize, Debug)]
pub struct VideoDetail {
    pub(crate) uri: String,
    #[serde(default)]
    pub(crate) title: String,
}

pub struct DiscogsApi {
    client: Client,
}

impl DiscogsApi {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            client: Client::builder().user_agent("CrateFM/0.1").build()?,
        })
    }

    pub async fn search(&self, params: &FetchParams, page: u32) -> anyhow::Result<SearchResponse> {
        let resp = self
            .client
            .get("https://api.discogs.com/database/search")
            .header("Authorization", format!("Discogs token={}", params.token))
            .query(&[
                ("q", ""),
                ("type", "release"),
                ("genre", params.genre.as_str()),
                ("style", params.style.as_str()),
                ("year", params.year.to_string().as_str()),
                ("page", page.to_string().as_str()),
                ("per_page", "100"),
            ])
            .send()
            .await?
            .json::<SearchResponse>()
            .await?;
        Ok(resp)
    }

    pub async fn get_release(
        &self,
        search_result: &SearchResult,
        params: &FetchParams,
    ) -> anyhow::Result<ReleaseDetail> {
        let detail: ReleaseDetail = self
            .client
            .get(format!(
                "https://api.discogs.com/releases/{}",
                search_result.id
            ))
            .header("Authorization", format!("Discogs token={}", params.token))
            .send()
            .await?
            .json()
            .await?;
        Ok(detail)
    }
}
