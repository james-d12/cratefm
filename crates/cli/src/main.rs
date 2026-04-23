use anyhow::Result;
use clap::{Parser, Subcommand};
use cratefm_core::discogs::fetch::fetch_releases;
use cratefm_core::{
    database::Db,
    models::{ListenVideo, ReleaseStatus},
};
use std::io::{self, Write as IoWrite};
use cratefm_core::discogs::models::FetchParams;

const DB_PATH: &str = "discogs.db";

#[derive(Parser)]
#[command(name = "cratefm", about = "Discogs release tracker")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Fetch releases from Discogs into the to_listen queue
    Fetch {
        #[arg(long)]
        token: String,
        #[arg(long, default_value = "Electronic")]
        genre: String,
        #[arg(long)]
        style: String,
        #[arg(long)]
        year: u32,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long, default_value_t = 10)]
        min_owners: u64,
        max_owners: Option<u64>,
        #[arg(long)]
        min_rating: Option<f64>,
    },
    /// List releases, optionally filtered to those with videos of a given status
    List {
        /// Filter: to_listen | liked | disliked | all (default: to_listen)
        #[arg(default_value = "to_listen")]
        status: String,
    },
    /// Manually update a video's status
    Mark {
        video_id: i64,
        #[arg(value_parser = ["to_listen", "liked", "disliked"])]
        status: String,
    },
    /// Listen to unrated videos one by one
    Listen {
        #[arg(long, default_value_t = 10)]
        batch: usize,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Fetch {
            token,
            genre,
            style,
            year,
            limit,
            min_owners,
            max_owners,
            min_rating,
        } => {
            let params = FetchParams {
                token,
                genre,
                style,
                year,
                limit,
                min_owners,
                max_owners,
                min_rating,
            };
            let db = Db::open(DB_PATH)?;
            let known_ids = db.known_ids()?;
            let start_page = db.get_cursor(&params.genre, &params.style, params.year)?;
            println!(
                "Searching for {} / {} releases from {} (starting page {start_page})...",
                params.genre, params.style, params.year
            );
            let releases = fetch_releases(&params, &known_ids, start_page).await?;
            db.save_releases(&releases.releases)?;
            db.save_videos(&releases.videos)?;
            db.save_images(&releases.images)?;
            db.set_cursor(
                &params.genre,
                &params.style,
                params.year,
                releases.next_page,
            )?;
            println!(
                "\nDone. {} releases, {} videos added.",
                releases.releases.len(),
                releases.videos.len()
            );
        }

        Command::List { status } => {
            let db = Db::open(DB_PATH)?;
            let filter = if status == "all" {
                None
            } else {
                Some(status.parse::<ReleaseStatus>()?)
            };
            let rows = db.list_releases(filter.as_ref())?;
            if rows.is_empty() {
                println!("No releases found.");
                return Ok(());
            }
            println!(
                "\n{:<5} {:<8} {:<8} {:<6} {:<8} {:<8} {:<8} {:<30} {}",
                "ID",
                "Rating",
                "Owners",
                "Year",
                "ToListen",
                "Liked",
                "Disliked",
                "Artist",
                "Title"
            );
            println!("{}", "-".repeat(120));
            for row in &rows {
                let r = &row.release;
                let year = r.year.map(|y| y.to_string()).unwrap_or_default();
                let artist = if r.artist.chars().count() > 29 {
                    &r.artist[..29]
                } else {
                    &r.artist
                };
                println!(
                    "{:<5} {:<8.2} {:<8} {:<6} {:<8} {:<8} {:<8} {:<30} {}",
                    r.id,
                    r.rating,
                    r.owners,
                    year,
                    row.to_listen_count,
                    row.liked_count,
                    row.disliked_count,
                    artist,
                    r.title
                );
            }
            println!();
        }

        Command::Mark { video_id, status } => {
            let status: ReleaseStatus = status.parse()?;
            let db = Db::open(DB_PATH)?;
            if db.mark_video(video_id, &status)? {
                println!("Video {video_id} marked as '{status}'.");
            } else {
                println!("No video found with id {video_id}.");
            }
        }

        Command::Listen { batch } => {
            cmd_listen(batch)?;
        }
    }

    Ok(())
}

fn cmd_listen(batch_size: usize) -> Result<()> {
    let db = Db::open(DB_PATH)?;
    let videos = db.next_listen_videos(batch_size, None)?;

    if videos.is_empty() {
        println!("No unrated videos in the queue.");
        return Ok(());
    }

    let total = videos.len();
    println!("Starting listen session: {total} unrated videos queued.");
    println!(
        "Tip: close VLC to move to the next track, or answer [q]uit after any track to stop.\n"
    );

    let tmp_dir = std::env::temp_dir().join(format!("cratefm-{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir)?;

    let result = run_listen_session(&db, &videos, &tmp_dir);
    std::fs::remove_dir_all(&tmp_dir).ok();
    result
}

fn run_listen_session(db: &Db, videos: &[ListenVideo], tmp_dir: &std::path::Path) -> Result<()> {
    let total = videos.len();
    let mut liked = 0usize;
    let mut disliked = 0usize;
    let mut skipped = 0usize;

    for (i, lv) in videos.iter().enumerate() {
        let track_num = i + 1;
        println!("\n[{track_num}/{total}] {}", "=".repeat(56));
        let year = lv.release_year.map(|y| y.to_string()).unwrap_or_default();
        println!(
            "  {} - {} ({})  |  Rating: {:.2}",
            lv.release_artist, lv.release_title, year, lv.release_rating
        );
        println!("  Video: {}", lv.video_title);
        println!("  {}", "=".repeat(56));
        println!("  Downloading...");

        // Clear old files
        for entry in std::fs::read_dir(tmp_dir)?.filter_map(|e| e.ok()) {
            let _ = std::fs::remove_file(entry.path());
        }

        let output_template = tmp_dir.join("%(id)s.%(ext)s");
        let dl = std::process::Command::new("yt-dlp")
            .args([
                "-x",
                "--audio-format",
                "mp3",
                "--audio-quality",
                "0",
                "--no-playlist",
                "-o",
                output_template.to_str().unwrap(),
                &lv.video_url,
            ])
            .output()?;

        if !dl.status.success() {
            let stderr = String::from_utf8_lossy(&dl.stderr);
            println!("  Download failed — skipping.\n  {}", stderr.trim());
            db.delete_video_by_url(&lv.video_url)?;
            skipped += 1;
            continue;
        }

        let files: Vec<std::path::PathBuf> = std::fs::read_dir(tmp_dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect();

        if files.is_empty() {
            println!("  No file found after download — skipping.");
            skipped += 1;
            continue;
        }

        let filepath = files
            .iter()
            .max_by_key(|p| p.metadata().and_then(|m| m.modified()).ok())
            .unwrap()
            .clone();

        println!("  Playing in VLC — close the window to continue...\n");
        std::process::Command::new("vlc")
            .args(["--play-and-exit", "--quiet", filepath.to_str().unwrap()])
            .status()?;

        loop {
            print!("  Liked it? [y]es / [n]o / [s]kip / [q]uit: ");
            io::stdout().flush()?;

            let mut answer = String::new();
            io::stdin().read_line(&mut answer)?;
            let answer = answer.trim().to_lowercase();

            match answer.as_str() {
                "y" | "yes" => {
                    db.mark_video(lv.video_id, &ReleaseStatus::Liked)?;
                    std::fs::remove_file(&filepath).ok();
                    println!("  Saved as liked.");
                    liked += 1;
                    break;
                }
                "n" | "no" => {
                    db.mark_video(lv.video_id, &ReleaseStatus::Disliked)?;
                    std::fs::remove_file(&filepath).ok();
                    println!("  Saved as disliked.");
                    disliked += 1;
                    break;
                }
                "s" | "skip" => {
                    std::fs::remove_file(&filepath).ok();
                    println!("  Skipped — still to_listen.");
                    skipped += 1;
                    break;
                }
                "q" | "quit" => {
                    println!("\n  Quitting listen session.");
                    println!(
                        "  Session summary — liked: {liked}  disliked: {disliked}  skipped: {skipped}"
                    );
                    return Ok(());
                }
                _ => {
                    println!("  Please enter y, n, s, or q.");
                }
            }
        }
    }

    println!(
        "\nListen session complete — liked: {liked}  disliked: {disliked}  skipped: {skipped}"
    );
    Ok(())
}
