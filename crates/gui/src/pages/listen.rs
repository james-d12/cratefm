use crate::DB_PATH;
use cratefm_core::database::releases::ReleaseStatus;
use cratefm_core::database::videos::ListenVideo;
use cratefm_core::database::Db;
use iced::widget::{button, container, horizontal_rule, row, text, text_input};
use iced::{Alignment, Element, Task};
use rodio::{Decoder, OutputStream, Sink};
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum Message {
    OpenUrl(String),
    ListenBatchInput(String),
    ListenStyleInput(String),
    ListenReset,
    ListenStart,
    ListenBatchLoaded(Result<Vec<ListenVideo>, String>),
    ListenDownloadDone(Result<PathBuf, String>),
    ListenPlaybackDone(u64),
    ListenPlayPause,
    ListenStop,
    ListenRate(RateAction),
}

#[derive(Debug, Clone, PartialEq)]
enum ListenPhase {
    Idle,
    Loading,
    Downloading,
    Playing,
    WaitingRating,
    Done,
}

#[derive(Debug, Clone)]
pub enum RateAction {
    Like,
    Dislike,
    Skip,
    Quit,
}

#[derive(Debug, Clone, Default)]
struct ListenStats {
    liked: usize,
    disliked: usize,
    skipped: usize,
}

struct PlaybackHandle {
    sink: Arc<Sink>,
    _stream: OutputStream,
}

pub struct ListenPage {
    listen_batch: String,
    listen_style: String,
    listen_phase: ListenPhase,
    listen_queue: Vec<ListenVideo>,
    listen_total: usize,
    listen_current: Option<ListenVideo>,
    listen_filepath: Option<PathBuf>,
    listen_stats: ListenStats,
    listen_error: Option<String>,
    listen_gen: u64,
    listen_paused: bool,
    listen_handle: Option<PlaybackHandle>,
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
            listen_paused: false,
            listen_handle: None,
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
                self.stop_playback();
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
                    self.start_playback(filepath)
                }
            },
            Message::ListenPlaybackDone(play_gen) => {
                if play_gen == self.listen_gen && self.listen_phase == ListenPhase::Playing {
                    self.listen_handle = None;
                    self.listen_paused = false;
                    self.listen_phase = ListenPhase::WaitingRating;
                }
                Task::none()
            }
            Message::ListenPlayPause => {
                if let Some(handle) = &self.listen_handle {
                    if self.listen_paused {
                        handle.sink.play();
                        self.listen_paused = false;
                    } else {
                        handle.sink.pause();
                        self.listen_paused = true;
                    }
                }
                Task::none()
            }
            Message::ListenStop => {
                self.stop_playback();
                self.listen_phase = ListenPhase::WaitingRating;
                Task::none()
            }
            Message::ListenRate(action) => {
                let video_id = self.listen_current.as_ref().map(|v| v.video_id);

                self.stop_playback();

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

    fn stop_playback(&mut self) {
        if let Some(handle) = self.listen_handle.take() {
            handle.sink.stop();
        }
        self.listen_paused = false;
    }

    fn start_download(&mut self, video: ListenVideo) -> Task<Message> {
        self.listen_gen += 1;
        self.listen_current = Some(video.clone());
        self.listen_phase = ListenPhase::Downloading;
        Task::perform(download_video(video), Message::ListenDownloadDone)
    }

    fn start_playback(&mut self, filepath: PathBuf) -> Task<Message> {
        let (stream, stream_handle) = match OutputStream::try_default() {
            Ok(pair) => pair,
            Err(e) => {
                self.listen_error = Some(format!("Audio output error: {e} — skipping"));
                self.listen_stats.skipped += 1;
                return self.advance_listen();
            }
        };

        let sink = match Sink::try_new(&stream_handle) {
            Ok(s) => Arc::new(s),
            Err(e) => {
                self.listen_error = Some(format!("Audio sink error: {e} — skipping"));
                self.listen_stats.skipped += 1;
                return self.advance_listen();
            }
        };

        let file = match std::fs::File::open(&filepath) {
            Ok(f) => f,
            Err(e) => {
                self.listen_error = Some(format!("File open error: {e} — skipping"));
                self.listen_stats.skipped += 1;
                return self.advance_listen();
            }
        };

        let decoder = match Decoder::new(BufReader::new(file)) {
            Ok(d) => d,
            Err(e) => {
                self.listen_error = Some(format!("Audio decode error: {e} — skipping"));
                self.listen_stats.skipped += 1;
                return self.advance_listen();
            }
        };

        sink.append(decoder);

        let sink_clone = Arc::clone(&sink);
        let play_gen = self.listen_gen;

        self.listen_handle = Some(PlaybackHandle { sink, _stream: stream });
        self.listen_phase = ListenPhase::Playing;
        self.listen_paused = false;

        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || sink_clone.sleep_until_end())
                    .await
                    .ok();
            },
            move |()| Message::ListenPlaybackDone(play_gen),
        )
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
                text("Plays your to_listen queue using yt-dlp."),
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

        let controls: Element<Message> = match &self.listen_phase {
            ListenPhase::Downloading => text("Downloading with yt-dlp…").into(),
            ListenPhase::Playing => {
                let pause_label = if self.listen_paused { "Resume" } else { "Pause" };
                let status = if self.listen_paused { "Paused" } else { "Now playing" };
                iced::widget::column![
                    text(status),
                    row![
                        button(text(pause_label))
                            .on_press(Message::ListenPlayPause)
                            .padding([8, 20]),
                        button(text("Stop"))
                            .on_press(Message::ListenStop)
                            .padding([8, 20]),
                    ]
                    .spacing(8),
                ]
                .spacing(8)
                .into()
            }
            ListenPhase::WaitingRating => iced::widget::column![
                text("Done playing — rate this release:"),
                row![
                    button(text("Like"))
                        .on_press(Message::ListenRate(RateAction::Like))
                        .padding([8, 20]),
                    button(text("Dislike"))
                        .on_press(Message::ListenRate(RateAction::Dislike))
                        .padding([8, 20]),
                    button(text("Skip"))
                        .on_press(Message::ListenRate(RateAction::Skip))
                        .padding([8, 20]),
                    button(text("Quit"))
                        .on_press(Message::ListenRate(RateAction::Quit))
                        .padding([8, 20]),
                ]
                .spacing(8),
            ]
            .spacing(8)
            .into(),
            _ => text("").into(),
        };

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
                controls,
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

async fn download_video(video: ListenVideo) -> Result<PathBuf, String> {
    let video_url = video.video_url;

    let tmp_dir = std::env::temp_dir().join(format!("cratefm-{}", std::process::id()));
    tokio::fs::create_dir_all(&tmp_dir)
        .await
        .map_err(|e| e.to_string())?;

    if let Ok(mut entries) = tokio::fs::read_dir(&tmp_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }

    let output_template = tmp_dir.join("%(id)s.%(ext)s");
    let status = tokio::process::Command::new("yt-dlp")
        .args([
            "-x",
            "--audio-format",
            "mp3",
            "--audio-quality",
            "0",
            "--no-playlist",
            "-o",
            output_template.to_str().unwrap_or("%(id)s.%(ext)s"),
            &video_url,
        ])
        .status()
        .await
        .map_err(|e| format!("Failed to run yt-dlp: {e}"))?;

    if !status.success() {
        return Err("yt-dlp exited with an error".into());
    }

    let mut entries = tokio::fs::read_dir(&tmp_dir)
        .await
        .map_err(|e| e.to_string())?;
    let mut best: Option<(PathBuf, std::time::SystemTime)> = None;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.is_file() {
            if let Ok(meta) = path.metadata() {
                if let Ok(modified) = meta.modified() {
                    if best.as_ref().is_none_or(|(_, t)| modified > *t) {
                        best = Some((path, modified));
                    }
                }
            }
        }
    }

    best.map(|(p, _)| p)
        .ok_or_else(|| "No file found after yt-dlp download".into())
}
