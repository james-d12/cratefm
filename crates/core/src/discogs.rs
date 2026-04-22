use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use tokio::time::sleep;

use crate::models::FetchParams;

/// A release ready to be persisted.
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

// ─── Discogs search API response types ───────────────────────────────────────

#[derive(Deserialize, Debug)]
struct SearchResponse {
    results: Vec<SearchResult>,
    pagination: Pagination,
}

#[derive(Deserialize, Debug)]
struct Pagination {
    #[allow(dead_code)]
    page: u32,
    pages: u32,
}

#[derive(Deserialize, Debug)]
struct SearchResult {
    id: u64,
    #[serde(default)]
    community: SearchCommunity,
    #[serde(default)]
    formats: Vec<Format>,
}

#[derive(Deserialize, Debug, Default)]
struct SearchCommunity {
    #[serde(default)]
    have: i64,
}

#[derive(Deserialize, Debug, Default)]
struct Format {
    #[serde(default)]
    descriptions: Vec<String>,
}

// ─── Full release API response types ─────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct ReleaseDetail {
    title: String,
    year: Option<i32>,
    uri: String,
    #[serde(default)]
    artists: Vec<Artist>,
    #[serde(default)]
    community: ReleaseCommunity,
    #[serde(default)]
    videos: Vec<VideoDetail>,
}

#[derive(Deserialize, Debug)]
struct Artist {
    name: String,
}

#[derive(Deserialize, Debug, Default)]
struct ReleaseCommunity {
    #[serde(default)]
    rating: Rating,
}

#[derive(Deserialize, Debug, Default)]
struct Rating {
    #[serde(default)]
    average: f64,
}

#[derive(Deserialize, Debug)]
struct VideoDetail {
    uri: String,
    #[serde(default)]
    title: String,
}

// ─── Fetcher ──────────────────────────────────────────────────────────────────

/// Searches the Discogs API and returns releases + videos that pass all filters.
///
/// `start_page` is loaded from the DB cursor so repeated fetches don't re-scan
/// already-seen pages. Returns the next page to resume from so the caller can
/// persist it back to the DB.
///
/// The returned release slice is shuffled randomly before returning.
pub async fn fetch_releases(
    params: &FetchParams,
    known_ids: &std::collections::HashSet<String>,
    start_page: u32,
) -> Result<(Vec<PendingRelease>, Vec<PendingVideo>, u32)> {
    let client = Client::builder()
        .user_agent("CrateFM/0.1")
        .build()?;

    let mut pending_releases: Vec<PendingRelease> = Vec::new();
    let mut pending_videos: Vec<PendingVideo> = Vec::new();
    let mut page = start_page.max(1);
    // Absolute index for log messages, accounting for skipped pages.
    let mut search_idx = ((page - 1) * 50) as usize;
    // Where to resume on the next fetch.
    // Default is 1 (wrap around); overridden to `page` if we stop mid-page due to limit.
    let mut next_page = 1u32;

    'outer: loop {
        let resp = client
            .get("https://api.discogs.com/database/search")
            .header(
                "Authorization",
                format!("Discogs token={}", params.token),
            )
            .query(&[
                ("q", ""),
                ("type", "release"),
                ("genre", params.genre.as_str()),
                ("style", params.style.as_str()),
                ("year", params.year.to_string().as_str()),
                ("sort", "community"),
                ("sort_order", "desc"),
                ("page", page.to_string().as_str()),
                ("per_page", "50"),
            ])
            .send()
            .await?
            .json::<SearchResponse>()
            .await?;

        let total_pages = resp.pagination.pages;

        // Guard against a stale cursor pointing past the end of the catalog.
        if page > total_pages {
            next_page = 1;
            break;
        }

        for result in resp.results {
            println!("Pending Releases to check: {:?} | limit: {:?}", pending_releases.len(), params.limit);
            
            if pending_releases.len() >= params.limit {
                println!("Reached max results ({}). Stopping.", params.limit);
                // Re-examine this page next run — we stopped mid-page.
                next_page = page;
                break 'outer;
            }

            search_idx += 1;
            let discogs_id = result.id.to_string();

            if known_ids.contains(&discogs_id) {
                println!("[{search_idx}] Skipping (already in DB): {discogs_id}");
                continue;
            }

            let owners = result.community.have;
            println!("[{search_idx}] Checking release {discogs_id}  owners={owners}");

            if owners < params.min_owners as i64 {
                println!("  Skipping (owners {owners} below min {}).", params.min_owners);
                sleep(Duration::from_secs(1)).await;
                continue;
            }

            if let Some(max) = params.max_owners {
                if owners > max as i64 {
                    println!("  Skipping (owners above max).");
                    sleep(Duration::from_secs(1)).await;
                    continue;
                }
            }

            let is_compilation = result
                .formats
                .iter()
                .any(|f| f.descriptions.iter().any(|d| d == "Compilation"));
            if is_compilation {
                println!("  Skipping (compilation).");
                sleep(Duration::from_secs(1)).await;
                continue;
            }

            // Fetch full release for artists, rating, and videos
            let detail: ReleaseDetail = client
                .get(format!(
                    "https://api.discogs.com/releases/{}",
                    result.id
                ))
                .header(
                    "Authorization",
                    format!("Discogs token={}", params.token),
                )
                .send()
                .await?
                .json()
                .await?;

            let rating = detail.community.rating.average;

            if let Some(min_rating) = params.min_rating {
                if rating < min_rating {
                    println!(
                        "  Skipping (rating {rating:.2} below minimum {min_rating})."
                    );
                    sleep(Duration::from_secs(1)).await;
                    continue;
                }
            }

            let artist_str = if detail.artists.is_empty() {
                "Unknown Artist".to_string()
            } else {
                detail
                    .artists
                    .iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            };

            let videos: Vec<PendingVideo> = detail
                .videos
                .iter()
                .filter(|v| !v.uri.is_empty())
                .map(|v| PendingVideo {
                    discogs_id: discogs_id.clone(),
                    title: v.title.clone(),
                    url: v.uri.clone(),
                })
                .collect();

            println!(
                "  Queued (rating: {rating:.2}, videos: {})",
                videos.len()
            );

            let release_url = if detail.uri.starts_with("http") {
                detail.uri.clone()
            } else {
                format!("https://www.discogs.com{}", detail.uri)
            };

            pending_releases.push(PendingRelease {
                discogs_id: discogs_id.clone(),
                title: detail.title,
                artist: artist_str,
                year: detail.year,
                genre: params.genre.clone(),
                style: params.style.clone(),
                rating,
                owners,
                url: release_url,
            });
            pending_videos.extend(videos);

            sleep(Duration::from_secs(1)).await;
        }

        if page >= total_pages {
            // next_page stays 1 (wrap around)
            break;
        }

        page += 1;
    }

    use rand::seq::SliceRandom;
    let mut rng = rand::rng();
    pending_releases.shuffle(&mut rng);

    Ok((pending_releases, pending_videos, next_page))
}
