use crate::discogs::api::{DiscogsApi, ReleaseDetail};
use crate::discogs::models::{FetchParams, PendingImage, PendingRelease, PendingVideo, Releases};
use anyhow::Result;
use std::time::Duration;
use tokio::time::sleep;

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
) -> Result<Releases> {
    let discogs_api = DiscogsApi::new();

    match discogs_api {
        Ok(discogs_api) => {
            let mut pending_releases: Vec<PendingRelease> = Vec::new();
            let mut pending_videos: Vec<PendingVideo> = Vec::new();
            let mut pending_images: Vec<PendingImage> = Vec::new();

            let mut page = start_page.max(1);
            // Absolute index for log messages, accounting for skipped pages.
            let mut search_idx = ((page - 1) * 50) as usize;
            // Where to resume on the next fetch.
            // Default is 1 (wrap around); overridden to `page` if we stop mid-page due to limit.
            let mut next_page = 1u32;

            'outer: loop {
                let resp = discogs_api.search(&params, page).await?;
                let total_pages = resp.pagination.pages;

                // Guard against a stale cursor pointing past the end of the catalog.
                if page > total_pages {
                    next_page = 1;
                    break;
                }

                for result in resp.results {
                    println!(
                        "Pending Releases to check: {:?} | limit: {:?}",
                        pending_releases.len(),
                        params.limit
                    );

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
                        println!(
                            "  Skipping (owners {owners} below min {}).",
                            params.min_owners
                        );
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
                    let detail: ReleaseDetail = discogs_api.get_release(&result, params).await?;

                    let rating = detail.community.rating.average;

                    if let Some(min_rating) = params.min_rating {
                        if rating < min_rating {
                            println!("  Skipping (rating {rating:.2} below minimum {min_rating}).");
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

                    let images: Vec<PendingImage> = detail
                        .images
                        .iter()
                        .filter(|i| !i.resource_url.is_empty())
                        .map(|i| PendingImage {
                            discogs_id: discogs_id.clone(),
                            url: i.resource_url.clone(),
                            width: i.width.clone(),
                            height: i.height.clone(),
                            image_type: i.image_type.clone(),
                        })
                        .collect();

                    println!("  Queued (rating: {rating:.2}, videos: {})", videos.len());

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
                    pending_images.extend(images);

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

            Ok(Releases {
                releases: pending_releases,
                videos: pending_videos,
                images: pending_images,
                next_page,
            })
        }
        Err(error) => {
            eprintln!("Error fetching discogs releases, {}", error);
            Err(error)
        }
    }
}
