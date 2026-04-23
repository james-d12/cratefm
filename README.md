# CrateFM

A music discovery tool built on the [Discogs](https://www.discogs.com) database. Search for releases by genre, style, and year, build a listening queue, and rate what you hear — all stored locally in a SQLite database.

Available as both a **CLI** and a **GUI** (built with [Iced](https://github.com/iced-rs/iced)).

---

## Features

- Search Discogs by genre, style, year, owner count, and minimum rating
- Filters out compilations and multi-style releases for cleaner results
- Remembers where it left off — pagination cursors pick up where the last fetch stopped
- Listen sessions download audio via `yt-dlp` and play it in VLC, then prompt you to rate each track
- All data (releases, videos, album art) stored in a local `discogs.db` SQLite file

## Prerequisites

- [Rust](https://rustup.rs) (2024 edition)
- A [Discogs API token](https://www.discogs.com/settings/developers)
- **CLI listen command only:** [`yt-dlp`](https://github.com/yt-dlp/yt-dlp) and [VLC](https://www.videolan.org/vlc/)

## Building

```bash
cargo build --release
```

This produces two binaries in `target/release/`:

| Binary | Description |
|---|---|
| `cratefm` | CLI |
| `cratefm-gui` | GUI |

## CLI Usage

### Fetch releases

```bash
cratefm fetch \
  --token YOUR_DISCOGS_TOKEN \
  --style "Deep House" \
  --year 2020 \
  --limit 20 \
  --min-owners 50 \
  --min-rating 4.0
```

| Flag | Default | Description |
|---|---|---|
| `--token` | — | Discogs API token (required) |
| `--genre` | `Electronic` | Genre to search |
| `--style` | — | Sub-genre / style (required) |
| `--year` | — | Release year (required) |
| `--limit` | `10` | Max releases to fetch |
| `--min-owners` | `10` | Minimum number of Discogs owners |
| `--max-owners` | — | Maximum number of Discogs owners |
| `--min-rating` | — | Minimum community rating |

### List releases

```bash
cratefm list                  # queued (to_listen)
cratefm list liked
cratefm list disliked
cratefm list all
```

### Listen session

Opens each unrated video in VLC and asks you to rate it:

```bash
cratefm listen --batch 10
```

After each track: `[y]es / [n]o / [s]kip / [q]uit`

### Mark a video manually

```bash
cratefm mark <video_id> liked
```

## GUI Usage

```bash
cratefm-gui
```

The GUI has five pages accessible from the nav bar:

- **Fetch** — configure and run a Discogs search
- **Releases** — browse fetched releases
- **Videos** — browse linked videos
- **Images** — browse album art
- **Listen** — play and rate your queue

## Project Structure

```
crates/
  core/   # Discogs API client, SQLite database layer
  cli/    # Command-line interface
  gui/    # Iced desktop GUI
```
