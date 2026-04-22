use crate::{DB_PATH, ListenPhase, ListenStats, RateAction, download_video, play_file};
use cratefm_core::db::Db;
use cratefm_core::models::{ListenVideo, ReleaseStatus};
use iced::widget::{button, container, horizontal_rule, row, text, text_input};
use iced::{Alignment, Element, Task};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum Message {
    OpenUrl(String),
    ListenBatchInput(String),
    ListenStyleInput(String),
    ListenReset,
    ListenStart,
    ListenBatchLoaded(Result<Vec<ListenVideo>, String>),
    ListenDownloadDone(Result<PathBuf, String>),
    ListenPlaybackDone(u64), // carries generation id
    ListenRate(RateAction),
}

pub struct ListenPage {
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

impl ListenPage {
    pub fn new() -> ListenPage {
        ListenPage {
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
        }
    }

    pub fn update(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::ListenBatchInput(v) => {
                self.listen_batch = v;
                Task::none()
            }
            Message::ListenStyleInput(v) => {
                self.listen_style = v;
                Task::none()
            }
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
                        db.next_listen_videos(batch, style_opt.as_deref())
                            .map_err(|e| e.to_string())
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
            Message::OpenUrl(url) => {
                let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
                Task::none()
            }
        }
    }

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

    pub fn view_listen(&self) -> Element<'_, Message> {
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
            iced::widget::column![
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
            iced::widget::column![
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
        let year_str = video
            .release_year
            .map(|y| format!(" ({y})"))
            .unwrap_or_default();
        let discogs_url = format!("https://www.discogs.com/release/{}", video.release_id);
        let youtube_url = video.video_url.clone();
        let card = iced::widget::column![
            text(format!("{}{}", video.release_title, year_str)).size(22),
            text(format!("by {}", video.release_artist)).size(16),
            text(format!(
                "{}  ·  {}  ·  Rating: {:.2}  ·  Owners: {}",
                video.release_genre,
                video.release_style,
                video.release_rating,
                video.release_owners
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
            iced::widget::column![
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
