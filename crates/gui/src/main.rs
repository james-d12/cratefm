use std::path::PathBuf;

use cratefm_core::{
    db::Db,
    discogs::fetch_releases,
    models::{FetchParams, ListenVideo, ReleaseRow, ReleaseStatus, VideoRow},
};
use iced::{
    widget::{
        button, column, container, horizontal_rule, pick_list, row, scrollable, text, text_input,
        Space,
    },
    Alignment, Element, Length, Task, Theme,
};

const DB_PATH: &str = "discogs.db";

fn main() -> iced::Result {
    iced::application("CrateFM", App::update, App::view)
        .theme(App::theme)
        .run_with(App::new)
}

// ─── Pages ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Page {
    Fetch,
    Releases,
    Videos,
    Listen,
}

// ─── Listen state machine ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum ListenPhase {
    Idle,
    Loading,
    Downloading,
    Playing,       // VLC running — buttons enabled
    WaitingRating, // VLC closed — waiting for user action
    Done,
}

#[derive(Debug, Clone, Default)]
struct ListenStats {
    liked: usize,
    disliked: usize,
    skipped: usize,
}

#[derive(Debug, Clone)]
enum RateAction {
    Like,
    Dislike,
    Skip,
    Quit,
}

// ─── Fetch state ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum FetchState {
    Idle,
    Running,
    Done { releases: usize, videos: usize },
    Error(String),
}

// ─── App state ────────────────────────────────────────────────────────────────

struct App {
    page: Page,

    // Fetch form
    f_token: String,
    f_genre: String,
    f_style: String,
    f_year: String,
    f_limit: String,
    f_min_owners: String,
    f_max_owners: String,
    f_min_rating: String,
    fetch_state: FetchState,

    // Releases view
    rel_status: ReleaseStatus,
    rel_search: String,
    rel_rows: Vec<ReleaseRow>,
    rel_error: Option<String>,

    // Videos view
    vid_search: String,
    vid_rows: Vec<VideoRow>,
    vid_error: Option<String>,

    // Listen session
    listen_batch: String,
    listen_style: String,
    listen_phase: ListenPhase,
    listen_queue: Vec<ListenVideo>, // upcoming, not including current
    listen_total: usize,            // size of original batch
    listen_current: Option<ListenVideo>,
    listen_filepath: Option<PathBuf>,
    listen_stats: ListenStats,
    listen_error: Option<String>,
    /// Incremented each time we start a new download/play cycle so stale
    /// PlaybackDone messages from a previous VLC process are ignored.
    listen_gen: u64,
}

// ─── Messages ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Message {
    // Navigation
    GoFetch,
    GoReleases,
    GoVideos,
    GoListen,

    // Fetch form
    FToken(String),
    FGenre(String),
    FStyle(String),
    FYear(String),
    FLimit(String),
    FMinOwners(String),
    FMaxOwners(String),
    FMinRating(String),
    DoFetch,
    FetchDone(Result<(usize, usize), String>),

    // Releases view
    RelStatusChanged(ReleaseStatus),
    RelSearch(String),
    RelRefresh,
    RelLoaded(Result<Vec<ReleaseRow>, String>),

    // Videos view
    VidSearch(String),
    VidRefresh,
    VidLoaded(Result<Vec<VideoRow>, String>),

    // Utility
    OpenUrl(String),

    // Listen session
    ListenBatchInput(String),
    ListenStyleInput(String),
    ListenReset,
    ListenStart,
    ListenBatchLoaded(Result<Vec<ListenVideo>, String>),
    ListenDownloadDone(Result<PathBuf, String>),
    ListenPlaybackDone(u64), // carries generation id
    ListenRate(RateAction),
}

// ─── App impl ────────────────────────────────────────────────────────────────

impl App {
    fn new() -> (Self, Task<Message>) {
        let app = Self {
            page: Page::Releases,
            f_token: String::new(),
            f_genre: "Electronic".into(),
            f_style: "Jungle".into(),
            f_year: "1995".into(),
            f_limit: "10".into(),
            f_min_owners: "10".into(),
            f_max_owners: String::new(),
            f_min_rating: "4.0".into(),
            fetch_state: FetchState::Idle,
            rel_status: ReleaseStatus::ToListen,
            rel_search: String::new(),
            rel_rows: vec![],
            rel_error: None,
            vid_search: String::new(),
            vid_rows: vec![],
            vid_error: None,
            listen_batch: "10".into(),
            listen_style: String::new(),
            listen_phase: ListenPhase::Idle,
            listen_queue: vec![],
            listen_total: 0,
            listen_current: None,
            listen_filepath: None,
            listen_stats: ListenStats::default(),
            listen_error: None,
            listen_gen: 0,
        };
        let task = load_releases(ReleaseStatus::ToListen);
        (app, task)
    }

    fn theme(&self) -> Theme {
        Theme::Dark
    }

    fn update(&mut self, msg: Message) -> Task<Message> {
        match msg {
            // ── Navigation ────────────────────────────────────────────────
            Message::GoFetch => {
                self.page = Page::Fetch;
                Task::none()
            }
            Message::GoReleases => {
                self.page = Page::Releases;
                load_releases(self.rel_status.clone())
            }
            Message::GoVideos => {
                self.page = Page::Videos;
                load_videos()
            }
            Message::GoListen => {
                self.page = Page::Listen;
                Task::none()
            }

            // ── Fetch form ────────────────────────────────────────────────
            Message::FToken(v) => { self.f_token = v; Task::none() }
            Message::FGenre(v) => { self.f_genre = v; Task::none() }
            Message::FStyle(v) => { self.f_style = v; Task::none() }
            Message::FYear(v) => { self.f_year = v; Task::none() }
            Message::FLimit(v) => { self.f_limit = v; Task::none() }
            Message::FMinOwners(v) => { self.f_min_owners = v; Task::none() }
            Message::FMaxOwners(v) => { self.f_max_owners = v; Task::none() }
            Message::FMinRating(v) => { self.f_min_rating = v; Task::none() }

            Message::DoFetch => {
                let params = FetchParams {
                    token: self.f_token.clone(),
                    genre: self.f_genre.clone(),
                    style: self.f_style.clone(),
                    year: self.f_year.parse().unwrap_or(2020),
                    limit: self.f_limit.parse().unwrap_or(10),
                    min_owners: self.f_min_owners.parse().unwrap_or(10),
                    max_owners: self.f_max_owners.parse().ok(),
                    min_rating: self.f_min_rating.parse().ok(),
                };
                self.fetch_state = FetchState::Running;
                Task::perform(do_fetch(params), Message::FetchDone)
            }
            Message::FetchDone(result) => {
                self.fetch_state = match result {
                    Ok((r, v)) => FetchState::Done { releases: r, videos: v },
                    Err(e) => FetchState::Error(e),
                };
                Task::none()
            }

            // ── Releases view ─────────────────────────────────────────────
            Message::RelStatusChanged(status) => {
                self.rel_status = status.clone();
                load_releases(status)
            }
            Message::RelSearch(v) => { self.rel_search = v; Task::none() }
            Message::RelRefresh => load_releases(self.rel_status.clone()),
            Message::RelLoaded(result) => {
                match result {
                    Ok(rows) => { self.rel_rows = rows; self.rel_error = None; }
                    Err(e) => self.rel_error = Some(e),
                }
                Task::none()
            }

            // ── Videos view ───────────────────────────────────────────────
            Message::VidSearch(v) => { self.vid_search = v; Task::none() }
            Message::VidRefresh => load_videos(),
            Message::VidLoaded(result) => {
                match result {
                    Ok(rows) => { self.vid_rows = rows; self.vid_error = None; }
                    Err(e) => self.vid_error = Some(e),
                }
                Task::none()
            }

            // ── Utility ───────────────────────────────────────────────────
            Message::OpenUrl(url) => {
                let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
                Task::none()
            }

            // ── Listen session ────────────────────────────────────────────
            Message::ListenBatchInput(v) => { self.listen_batch = v; Task::none() }
            Message::ListenStyleInput(v) => { self.listen_style = v; Task::none() }
            Message::ListenReset => {
                self.listen_phase = ListenPhase::Idle;
                self.listen_current = None;
                self.listen_queue = vec![];
                self.listen_error = None;
                Task::none()
            }

            Message::ListenStart => {
                self.listen_phase = ListenPhase::Loading;
                self.listen_stats = ListenStats::default();
                self.listen_error = None;
                let batch: usize = self.listen_batch.parse().unwrap_or(10);
                let style = self.listen_style.trim().to_owned();
                let style_opt = if style.is_empty() { None } else { Some(style) };
                Task::perform(
                    async move {
                        let db = Db::open(DB_PATH).map_err(|e| e.to_string())?;
                        db.next_listen_videos(batch, style_opt.as_deref()).map_err(|e| e.to_string())
                    },
                    Message::ListenBatchLoaded,
                )
            }

            Message::ListenBatchLoaded(result) => match result {
                Err(e) => {
                    self.listen_phase = ListenPhase::Idle;
                    self.listen_error = Some(e);
                    Task::none()
                }
                Ok(videos) if videos.is_empty() => {
                    self.listen_phase = ListenPhase::Done;
                    Task::none()
                }
                Ok(mut videos) => {
                    self.listen_total = videos.len();
                    let current = videos.remove(0);
                    self.listen_queue = videos;
                    self.start_download(current)
                }
            },

            Message::ListenDownloadDone(result) => match result {
                Err(e) => {
                    self.listen_error = Some(format!("Download failed: {e} — skipping"));
                    self.listen_stats.skipped += 1;
                    self.advance_listen()
                }
                Ok(filepath) => {
                    self.listen_filepath = Some(filepath.clone());
                    self.listen_phase = ListenPhase::Playing;
                    let play_gen = self.listen_gen;
                    Task::perform(play_file(filepath), move |()| {
                        Message::ListenPlaybackDone(play_gen)
                    })
                }
            },

            Message::ListenPlaybackDone(play_gen) => {
                // Ignore stale messages from previous VLC processes
                if play_gen == self.listen_gen && self.listen_phase == ListenPhase::Playing {
                    self.listen_phase = ListenPhase::WaitingRating;
                }
                Task::none()
            }

            Message::ListenRate(action) => {
                let video_id = self.listen_current.as_ref().map(|v| v.video_id);

                // Clean up the downloaded file
                if let Some(path) = self.listen_filepath.take() {
                    let _ = std::fs::remove_file(&path);
                }

                match action {
                    RateAction::Quit => {
                        self.listen_phase = ListenPhase::Done;
                        return Task::none();
                    }
                    RateAction::Skip => {
                        self.listen_stats.skipped += 1;
                    }
                    RateAction::Like => {
                        self.listen_stats.liked += 1;
                        if let Some(id) = video_id {
                            let _ = Db::open(DB_PATH)
                                .and_then(|db| db.mark_video(id, &ReleaseStatus::Liked));
                        }
                    }
                    RateAction::Dislike => {
                        self.listen_stats.disliked += 1;
                        if let Some(id) = video_id {
                            let _ = Db::open(DB_PATH)
                                .and_then(|db| db.mark_video(id, &ReleaseStatus::Disliked));
                        }
                    }
                }

                self.advance_listen()
            }
        }
    }

    /// Pull the next video from the queue and start downloading it,
    /// or end the session if the queue is empty.
    fn start_download(&mut self, video: ListenVideo) -> Task<Message> {
        self.listen_gen += 1;
        self.listen_current = Some(video.clone());
        self.listen_phase = ListenPhase::Downloading;
        Task::perform(download_video(video), Message::ListenDownloadDone)
    }

    fn advance_listen(&mut self) -> Task<Message> {
        if self.listen_queue.is_empty() {
            self.listen_phase = ListenPhase::Done;
            self.listen_current = None;
            return Task::none();
        }
        let next = self.listen_queue.remove(0);
        self.start_download(next)
    }

    // ── View ─────────────────────────────────────────────────────────────────

    fn view(&self) -> Element<'_, Message> {
        let nav = row![
            nav_btn("Fetch", Message::GoFetch, self.page == Page::Fetch),
            nav_btn("Releases", Message::GoReleases, self.page == Page::Releases),
            nav_btn("Videos", Message::GoVideos, self.page == Page::Videos),
            nav_btn("Listen", Message::GoListen, self.page == Page::Listen),
            Space::with_width(Length::Fill),
        ]
        .spacing(6)
        .padding(10)
        .align_y(Alignment::Center);

        let body = match &self.page {
            Page::Fetch => self.view_fetch(),
            Page::Releases => self.view_releases(),
            Page::Videos => self.view_videos(),
            Page::Listen => self.view_listen(),
        };

        column![nav, horizontal_rule(1), body]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    // ── Fetch page ────────────────────────────────────────────────────────────

    fn view_fetch(&self) -> Element<'_, Message> {
        let is_running = matches!(self.fetch_state, FetchState::Running);

        let form = column![
            labeled_field(
                "Token",
                text_input("Discogs API token", &self.f_token)
                    .on_input(Message::FToken)
                    .secure(true)
            ),
            labeled_field("Genre", text_input("e.g. Electronic", &self.f_genre).on_input(Message::FGenre)),
            labeled_field("Style", text_input("e.g. Techno", &self.f_style).on_input(Message::FStyle)),
            labeled_field("Year", text_input("e.g. 2020", &self.f_year).on_input(Message::FYear)),
            labeled_field("Limit", text_input("number of releases", &self.f_limit).on_input(Message::FLimit)),
            labeled_field("Min owners", text_input("e.g. 10", &self.f_min_owners).on_input(Message::FMinOwners)),
            labeled_field("Max owners", text_input("e.g. 500", &self.f_max_owners).on_input(Message::FMaxOwners)),
            labeled_field("Min rating", text_input("optional, e.g. 3.5", &self.f_min_rating).on_input(Message::FMinRating)),
        ]
        .spacing(10)
        .max_width(520);

        let fetch_btn = {
            let b = button(if is_running { "Fetching…" } else { "Fetch releases" })
                .padding([8, 20]);
            if is_running { b } else { b.on_press(Message::DoFetch) }
        };

        let status_line: Element<Message> = match &self.fetch_state {
            FetchState::Idle => text("").into(),
            FetchState::Running => text("Fetching releases from Discogs — this may take a while…").into(),
            FetchState::Done { releases, videos } => {
                text(format!("Done — {releases} releases and {videos} videos saved.")).into()
            }
            FetchState::Error(e) => text(format!("Error: {e}")).into(),
        };

        container(column![form, fetch_btn, status_line].spacing(16).padding(24)).into()
    }

    // ── Releases page ─────────────────────────────────────────────────────────

    fn view_releases(&self) -> Element<'_, Message> {
        let status_opts = vec![
            ReleaseStatus::ToListen,
            ReleaseStatus::Liked,
            ReleaseStatus::Disliked,
        ];

        let filters = row![
            text("Status:"),
            pick_list(status_opts, Some(self.rel_status.clone()), Message::RelStatusChanged),
            text_input("Search artist / title…", &self.rel_search)
                .on_input(Message::RelSearch)
                .width(280),
            Space::with_width(Length::Fill),
            button("Refresh").on_press(Message::RelRefresh),
        ]
        .spacing(8)
        .padding([8, 16])
        .align_y(Alignment::Center);

        let content: Element<Message> = if let Some(err) = &self.rel_error {
            container(text(format!("Error: {err}"))).padding(16).into()
        } else {
            let search = self.rel_search.to_lowercase();
            let visible: Vec<_> = self
                .rel_rows
                .iter()
                .filter(|rr| {
                    search.is_empty()
                        || rr.release.artist.to_lowercase().contains(&search)
                        || rr.release.title.to_lowercase().contains(&search)
                        || rr.release.genre.to_lowercase().contains(&search)
                        || rr.release.style.to_lowercase().contains(&search)
                })
                .collect();

            let header = table_row(vec![
                (50, "ID"), (160, "Artist"), (200, "Title"), (55, "Year"),
                (120, "Genre"), (120, "Style"), (65, "Rating"), (70, "Owners"),
                (65, "To Listen"), (55, "Liked"), (65, "Disliked"),
            ]);

            let rows: Vec<Element<Message>> = visible
                .iter()
                .map(|rr| {
                    let r = &rr.release;
                    table_row(vec![
                        (50, r.id.to_string()),
                        (160, r.artist.clone()),
                        (200, r.title.clone()),
                        (55, r.year.map(|y| y.to_string()).unwrap_or_default()),
                        (120, r.genre.clone()),
                        (120, r.style.clone()),
                        (65, format!("{:.2}", r.rating)),
                        (70, r.owners.to_string()),
                        (65, rr.to_listen_count.to_string()),
                        (55, rr.liked_count.to_string()),
                        (65, rr.disliked_count.to_string()),
                    ])
                })
                .collect();

            scrollable(
                column(
                    std::iter::once(header)
                        .chain(std::iter::once(horizontal_rule(1).into()))
                        .chain(rows)
                        .chain(std::iter::once(
                            text(format!("{} release(s)", visible.len())).into(),
                        ))
                        .collect::<Vec<_>>(),
                )
                .spacing(2)
                .padding(iced::Padding { top: 0.0, right: 16.0, bottom: 16.0, left: 16.0 }),
            )
            .height(Length::Fill)
            .into()
        };

        column![filters, horizontal_rule(1), content]
            .height(Length::Fill)
            .into()
    }

    // ── Videos page ───────────────────────────────────────────────────────────

    fn view_videos(&self) -> Element<'_, Message> {
        let filters = row![
            text_input("Search artist / title / URL…", &self.vid_search)
                .on_input(Message::VidSearch)
                .width(380),
            Space::with_width(Length::Fill),
            button("Refresh").on_press(Message::VidRefresh),
        ]
        .spacing(8)
        .padding([8, 16])
        .align_y(Alignment::Center);

        let content: Element<Message> = if let Some(err) = &self.vid_error {
            container(text(format!("Error: {err}"))).padding(16).into()
        } else {
            let search = self.vid_search.to_lowercase();
            let visible: Vec<_> = self
                .vid_rows
                .iter()
                .filter(|vr| {
                    search.is_empty()
                        || vr.video.title.to_lowercase().contains(&search)
                        || vr.video.url.to_lowercase().contains(&search)
                        || vr.release_artist.to_lowercase().contains(&search)
                        || vr.release_title.to_lowercase().contains(&search)
                })
                .collect();

            let header = table_row(vec![
                (50, "ID"), (80, "Status"), (150, "Artist"), (180, "Release"), (200, "Video title"), (300, "URL"),
            ]);

            let rows: Vec<Element<Message>> = visible
                .iter()
                .map(|vr| {
                    table_row(vec![
                        (50, vr.video.id.to_string()),
                        (80, vr.video.status.to_string()),
                        (150, vr.release_artist.clone()),
                        (180, vr.release_title.clone()),
                        (200, vr.video.title.clone()),
                        (300, vr.video.url.clone()),
                    ])
                })
                .collect();

            scrollable(
                column(
                    std::iter::once(header)
                        .chain(std::iter::once(horizontal_rule(1).into()))
                        .chain(rows)
                        .chain(std::iter::once(
                            text(format!("{} video(s)", visible.len())).into(),
                        ))
                        .collect::<Vec<_>>(),
                )
                .spacing(2)
                .padding(iced::Padding { top: 0.0, right: 16.0, bottom: 16.0, left: 16.0 }),
            )
            .height(Length::Fill)
            .into()
        };

        column![filters, horizontal_rule(1), content]
            .height(Length::Fill)
            .into()
    }

    // ── Listen page ───────────────────────────────────────────────────────────

    fn view_listen(&self) -> Element<'_, Message> {
        match &self.listen_phase {
            ListenPhase::Idle => self.view_listen_idle(),
            ListenPhase::Loading => container(text("Loading queue…")).padding(24).into(),
            ListenPhase::Done => self.view_listen_done(),
            _ => self.view_listen_session(),
        }
    }

    fn view_listen_idle(&self) -> Element<'_, Message> {
        let error_line: Element<Message> = match &self.listen_error {
            Some(e) => text(format!("Last error: {e}")).into(),
            None => text("").into(),
        };

        container(
            column![
                text("Listen Session").size(20),
                text("Plays your to_listen queue using yt-dlp + VLC."),
                row![
                    text("Batch size:"),
                    text_input("10", &self.listen_batch)
                        .on_input(Message::ListenBatchInput)
                        .width(70),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
                row![
                    text("Style filter:"),
                    text_input("All styles", &self.listen_style)
                        .on_input(Message::ListenStyleInput)
                        .width(200),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
                button("Start session")
                    .on_press(Message::ListenStart)
                    .padding([8, 20]),
                error_line,
            ]
            .spacing(16)
            .padding(24)
            .max_width(500),
        )
        .into()
    }

    fn view_listen_done(&self) -> Element<'_, Message> {
        let s = &self.listen_stats;
        container(
            column![
                text("Session complete").size(20),
                text(format!(
                    "Liked: {}   Disliked: {}   Skipped: {}",
                    s.liked, s.disliked, s.skipped
                )),
                button("Start new session")
                    .on_press(Message::ListenReset)
                    .padding([8, 20]),
            ]
            .spacing(16)
            .padding(24),
        )
        .into()
    }

    fn view_listen_session(&self) -> Element<'_, Message> {
        let video = match &self.listen_current {
            Some(v) => v,
            None => return container(text("")).padding(24).into(),
        };

        let position = self.listen_total - self.listen_queue.len();
        let progress = text(format!("Track {position} / {}", self.listen_total));

        // Release + video card
        let year_str = video.release_year.map(|y| format!(" ({y})")).unwrap_or_default();
        let discogs_url = format!("https://www.discogs.com/release/{}", video.release_id);
        let youtube_url = video.video_url.clone();
        let card = column![
            text(format!("{}{}", video.release_title, year_str)).size(22),
            text(format!("by {}", video.release_artist)).size(16),
            text(format!(
                "{}  ·  {}  ·  Rating: {:.2}  ·  Owners: {}",
                video.release_genre, video.release_style, video.release_rating, video.release_owners
            ))
            .size(13),
            text(format!("Video: {}", video.video_title)).size(13),
            row![
                button(text("Discogs").size(12))
                    .on_press(Message::OpenUrl(discogs_url))
                    .padding([4, 10])
                    .style(button::secondary),
                button(text("YouTube").size(12))
                    .on_press(Message::OpenUrl(youtube_url))
                    .padding([4, 10])
                    .style(button::secondary),
            ]
            .spacing(8),
        ]
        .spacing(4);

        let status_line = match &self.listen_phase {
            ListenPhase::Downloading => text("Downloading with yt-dlp…"),
            ListenPhase::Playing => text("Playing in VLC — close VLC or rate below"),
            ListenPhase::WaitingRating => text("Done playing — rate this release:"),
            _ => text(""),
        };

        // Buttons — enabled once VLC has started (Playing or WaitingRating)
        let can_rate = matches!(
            self.listen_phase,
            ListenPhase::Playing | ListenPhase::WaitingRating
        );
        let rate_btn = |label: &'static str, action: RateAction| -> Element<'_, Message> {
            let b = button(text(label)).padding([10, 20]);
            if can_rate {
                b.on_press(Message::ListenRate(action)).into()
            } else {
                b.into()
            }
        };

        let buttons = row![
            rate_btn("Like", RateAction::Like),
            rate_btn("Dislike", RateAction::Dislike),
            rate_btn("Skip", RateAction::Skip),
            rate_btn("Quit", RateAction::Quit),
        ]
        .spacing(8);

        let error_line: Element<Message> = match &self.listen_error {
            Some(e) => text(format!("Note: {e}")).size(12).into(),
            None => text("").into(),
        };

        let s = &self.listen_stats;
        let stats_line = text(format!(
            "So far — liked: {}  disliked: {}  skipped: {}",
            s.liked, s.disliked, s.skipped
        ))
        .size(12);

        container(
            column![
                progress,
                horizontal_rule(1),
                card,
                horizontal_rule(1),
                status_line,
                buttons,
                stats_line,
                error_line,
            ]
            .spacing(14)
            .padding(24)
            .max_width(600),
        )
        .into()
    }
}

// ─── Async tasks ─────────────────────────────────────────────────────────────

async fn do_fetch(params: FetchParams) -> Result<(usize, usize), String> {
    let db = Db::open(DB_PATH).map_err(|e| e.to_string())?;
    let known_ids = db.known_ids().map_err(|e| e.to_string())?;
    let start_page = db
        .get_cursor(&params.genre, &params.style, params.year)
        .map_err(|e| e.to_string())?;
    let (releases, videos, next_page) = fetch_releases(&params, &known_ids, start_page)
        .await
        .map_err(|e| e.to_string())?;
    db.save_releases(&releases).map_err(|e| e.to_string())?;
    db.save_videos(&videos).map_err(|e| e.to_string())?;
    db.set_cursor(&params.genre, &params.style, params.year, next_page)
        .map_err(|e| e.to_string())?;
    Ok((releases.len(), videos.len()))
}

/// Download a video using yt-dlp and return the file path.
async fn download_video(video: ListenVideo) -> Result<PathBuf, String> {
    let video_url = video.video_url;

    let tmp_dir =
        std::env::temp_dir().join(format!("cratefm-{}", std::process::id()));
    tokio::fs::create_dir_all(&tmp_dir)
        .await
        .map_err(|e| e.to_string())?;

    // Clear any leftover files from a previous track
    if let Ok(mut entries) = tokio::fs::read_dir(&tmp_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }

    let output_template = tmp_dir.join("%(id)s.%(ext)s");
    let status = tokio::process::Command::new("yt-dlp")
        .args([
            "-x",
            "--audio-format", "mp3",
            "--audio-quality", "0",
            "--no-playlist",
            "-o", output_template.to_str().unwrap_or("%(id)s.%(ext)s"),
            &video_url,
        ])
        .status()
        .await
        .map_err(|e| format!("Failed to run yt-dlp: {e}"))?;

    if !status.success() {
        return Err("yt-dlp exited with an error".into());
    }

    // Find the file that was just written
    let mut entries = tokio::fs::read_dir(&tmp_dir)
        .await
        .map_err(|e| e.to_string())?;
    let mut best: Option<(PathBuf, std::time::SystemTime)> = None;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.is_file() {
            if let Ok(meta) = path.metadata() {
                if let Ok(modified) = meta.modified() {
                    if best.as_ref().map_or(true, |(_, t)| modified > *t) {
                        best = Some((path, modified));
                    }
                }
            }
        }
    }

    best.map(|(p, _)| p)
        .ok_or_else(|| "No file found after yt-dlp download".into())
}

/// Launch VLC and wait for it to exit. Returns unit regardless of VLC's exit code.
async fn play_file(filepath: PathBuf) {
    let _ = tokio::process::Command::new("vlc")
        .args([
            "--play-and-exit",
            "--quiet",
            filepath.to_str().unwrap_or(""),
        ])
        .status()
        .await;
}

fn load_releases(status: ReleaseStatus) -> Task<Message> {
    Task::perform(
        async move {
            let db = Db::open(DB_PATH).map_err(|e| e.to_string())?;
            db.list_releases(Some(&status)).map_err(|e| e.to_string())
        },
        Message::RelLoaded,
    )
}

fn load_videos() -> Task<Message> {
    Task::perform(
        async move {
            let db = Db::open(DB_PATH).map_err(|e| e.to_string())?;
            db.list_all_videos().map_err(|e| e.to_string())
        },
        Message::VidLoaded,
    )
}

// ─── Widget helpers ───────────────────────────────────────────────────────────

fn nav_btn(label: &str, msg: Message, active: bool) -> Element<'_, Message> {
    let b = button(text(label)).padding([6, 14]).on_press(msg);
    if active {
        b.style(button::primary).into()
    } else {
        b.style(button::secondary).into()
    }
}

fn labeled_field<'a>(
    label: &'a str,
    input: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    row![
        text(label).width(100),
        container(input.into()).width(320),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn table_row(cols: Vec<(u16, impl ToString)>) -> Element<'static, Message> {
    row(cols
        .into_iter()
        .map(|(w, s)| {
            container(text(s.to_string()).size(13))
                .width(Length::Fixed(w as f32))
                .into()
        })
        .collect::<Vec<_>>())
    .spacing(4)
    .padding([3u16, 0u16])
    .into()
}
