use anyhow::Result;
use clap::{Parser, Subcommand};
use cratefm_core::{
    db::Db,
    discogs::fetch_releases,
    models::{FetchParams, Release, ReleaseStatus},
};
use std::io::{self, Write as IoWrite};
use std::path::{Path, PathBuf};

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
        #[arg(long, default_value_t = 500)]
        max_owners: u64,
        #[arg(long)]
        min_rating: Option<f64>,
    },
    /// List releases by status
    List {
        #[arg(value_parser = ["to_listen", "liked", "disliked"])]
        status: String,
    },
    /// Manually update a release status
    Mark {
        id: i64,
        #[arg(value_parser = ["to_listen", "liked", "disliked"])]
        status: String,
    },
    /// Listen to queued releases one by one
    Listen {
        #[arg(long, default_value_t = 10)]
        batch: usize,
        /// Directory to move liked tracks to (default: delete after playing)
        #[arg(long, value_name = "DIR")]
        keep: Option<String>,
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
            println!(
                "Searching for {} / {} releases from {}...",
                params.genre, params.style, params.year
            );
            let (releases, videos) = fetch_releases(&params, &known_ids).await?;
            db.save_releases(&releases)?;
            db.save_videos(&videos)?;
            println!(
                "\nDone. {} releases, {} videos added.",
                releases.len(),
                videos.len()
            );
        }

        Command::List { status } => {
            let status: ReleaseStatus = status.parse()?;
            let db = Db::open(DB_PATH)?;
            let rows = db.list_releases(&status)?;
            if rows.is_empty() {
                println!("No releases in '{status}'.");
                return Ok(());
            }
            println!(
                "\n{:<5} {:<8} {:<8} {:<6} {:<8} {:<30} {}",
                "ID", "Rating", "Owners", "Year", "Videos", "Artist", "Title"
            );
            println!("{}", "-".repeat(110));
            for row in &rows {
                let r = &row.release;
                let year = r.year.map(|y| y.to_string()).unwrap_or_default();
                let artist = if r.artist.chars().count() > 29 {
                    &r.artist[..29]
                } else {
                    &r.artist
                };
                println!(
                    "{:<5} {:<8.2} {:<8} {:<6} {:<8} {:<30} {}",
                    r.id, r.rating, r.owners, year, row.video_count, artist, r.title
                );
            }
            println!();
        }

        Command::Mark { id, status } => {
            let status: ReleaseStatus = status.parse()?;
            let db = Db::open(DB_PATH)?;
            if db.mark_release(id, &status)? {
                println!("Release {id} marked as '{status}'.");
            } else {
                println!("No release found with id {id}.");
            }
        }

        Command::Listen { batch, keep } => {
            let keep_dir = keep.map(|k| {
                let p = expand_tilde(&k);
                std::fs::create_dir_all(&p).expect("failed to create keep directory");
                p
            });
            cmd_listen(batch, keep_dir.as_deref())?;
        }
    }

    Ok(())
}

fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(path));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

fn cmd_listen(batch_size: usize, keep_dir: Option<&Path>) -> Result<()> {
    let db = Db::open(DB_PATH)?;
    let rows = db.next_listen_batch(batch_size)?;

    if rows.is_empty() {
        println!("No releases with videos in to_listen.");
        return Ok(());
    }

    let total = rows.len();
    println!("Starting listen session: {total} releases queued.");
    println!(
        "Tip: close VLC to move to the next track, or answer [q]uit after any track to stop.\n"
    );

    let tmp_dir = std::env::temp_dir().join(format!("cratefm-{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir)?;

    let result = run_listen_session(&db, &rows, &tmp_dir, keep_dir);
    std::fs::remove_dir_all(&tmp_dir).ok();
    result
}

fn run_listen_session(
    db: &Db,
    rows: &[Release],
    tmp_dir: &Path,
    keep_dir: Option<&Path>,
) -> Result<()> {
    let total = rows.len();
    let mut liked = 0usize;
    let mut disliked = 0usize;
    let mut skipped = 0usize;

    for (i, release) in rows.iter().enumerate() {
        let video_url = match db.first_video_url(release.id)? {
            Some(url) => url,
            None => continue,
        };

        let track_num = i + 1;
        println!("\n[{track_num}/{total}] {}", "=".repeat(56));
        let year = release.year.map(|y| y.to_string()).unwrap_or_default();
        println!(
            "  {} - {} ({})  |  Rating: {:.2}",
            release.artist, release.title, year, release.rating
        );
        println!("  {}", "=".repeat(56));
        println!("  Downloading...");

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
                &video_url,
            ])
            .output()?;

        if !dl.status.success() {
            let stderr = String::from_utf8_lossy(&dl.stderr);
            println!(
                "  Download failed — removing video and skipping.\n  {}",
                stderr.trim()
            );
            db.delete_video_by_url(&video_url)?;
            skipped += 1;
            continue;
        }

        let files: Vec<PathBuf> = std::fs::read_dir(tmp_dir)?
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
            .max_by_key(|p| {
                p.metadata()
                    .and_then(|m| m.modified())
                    .ok()
            })
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
                    db.mark_release(release.id, &ReleaseStatus::Liked)?;
                    if let Some(keep) = keep_dir {
                        let dest = keep.join(filepath.file_name().unwrap());
                        std::fs::rename(&filepath, &dest)?;
                        println!("  Saved as liked — kept at {}", dest.display());
                    } else {
                        std::fs::remove_file(&filepath).ok();
                        println!("  Saved as liked.");
                    }
                    liked += 1;
                    break;
                }
                "n" | "no" => {
                    db.mark_release(release.id, &ReleaseStatus::Disliked)?;
                    std::fs::remove_file(&filepath).ok();
                    println!("  Saved as disliked.");
                    disliked += 1;
                    break;
                }
                "s" | "skip" => {
                    std::fs::remove_file(&filepath).ok();
                    println!("  Skipped — still in to_listen.");
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
