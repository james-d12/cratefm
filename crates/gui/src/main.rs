mod pages;

use crate::pages::fetch::FetchPage;
use crate::pages::listen::ListenPage;
use crate::pages::{fetch, listen};
use cratefm_core::{
    db::Db,
    models::{ReleaseRow, ReleaseStatus, VideoRow},
};
use iced::{
    Alignment, Element, Length, Task, Theme,
    widget::{
        Space, button, column, container, horizontal_rule, pick_list, row, scrollable, text,
        text_input,
    },
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

    listen_page: ListenPage,
    fetch_page: FetchPage,

    // Releases view
    rel_status: ReleaseStatus,
    rel_search: String,
    rel_rows: Vec<ReleaseRow>,
    rel_error: Option<String>,

    // Videos view
    vid_search: String,
    vid_rows: Vec<VideoRow>,
    vid_error: Option<String>,
}

// ─── Messages ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Message {
    // Navigation
    GoFetch,
    GoReleases,
    GoVideos,
    GoListen,

    // Releases view
    RelStatusChanged(ReleaseStatus),
    RelSearch(String),
    RelRefresh,
    RelLoaded(Result<Vec<ReleaseRow>, String>),

    // Videos view
    VidSearch(String),
    VidRefresh,
    VidLoaded(Result<Vec<VideoRow>, String>),

    // Listen
    Listen(listen::Message),
    Fetch(fetch::Message),
}

// ─── App impl ────────────────────────────────────────────────────────────────

impl App {
    fn new() -> (Self, Task<Message>) {
        let app = Self {
            page: Page::Releases,
            rel_status: ReleaseStatus::ToListen,
            rel_search: String::new(),
            rel_rows: vec![],
            rel_error: None,
            vid_search: String::new(),
            vid_rows: vec![],
            vid_error: None,
            listen_page: ListenPage::new(),
            fetch_page: FetchPage::new(),
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

            // ── Releases view ─────────────────────────────────────────────
            Message::RelStatusChanged(status) => {
                self.rel_status = status.clone();
                load_releases(status)
            }
            Message::RelSearch(v) => {
                self.rel_search = v;
                Task::none()
            }
            Message::RelRefresh => load_releases(self.rel_status.clone()),
            Message::RelLoaded(result) => {
                match result {
                    Ok(rows) => {
                        self.rel_rows = rows;
                        self.rel_error = None;
                    }
                    Err(e) => self.rel_error = Some(e),
                }
                Task::none()
            }

            // ── Videos view ───────────────────────────────────────────────
            Message::VidSearch(v) => {
                self.vid_search = v;
                Task::none()
            }
            Message::VidRefresh => load_videos(),
            Message::VidLoaded(result) => {
                match result {
                    Ok(rows) => {
                        self.vid_rows = rows;
                        self.vid_error = None;
                    }
                    Err(e) => self.vid_error = Some(e),
                }
                Task::none()
            }
            Message::Listen(msg) => self.listen_page.update(msg).map(Message::Listen),
            Message::Fetch(msg) => self.fetch_page.update(msg).map(Message::Fetch),
        }
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
            Page::Fetch => self.fetch_page.view_fetch().map(Message::Fetch),
            Page::Releases => self.view_releases(),
            Page::Videos => self.view_videos(),
            Page::Listen => self.listen_page.view_listen().map(Message::Listen),
        };

        column![nav, horizontal_rule(1), body]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
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
            pick_list(
                status_opts,
                Some(self.rel_status.clone()),
                Message::RelStatusChanged
            ),
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
                (50, "ID"),
                (160, "Artist"),
                (200, "Title"),
                (55, "Year"),
                (120, "Genre"),
                (120, "Style"),
                (65, "Rating"),
                (70, "Owners"),
                (65, "To Listen"),
                (55, "Liked"),
                (65, "Disliked"),
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
                .padding(iced::Padding {
                    top: 0.0,
                    right: 16.0,
                    bottom: 16.0,
                    left: 16.0,
                }),
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
                (50, "ID"),
                (80, "Status"),
                (150, "Artist"),
                (180, "Release"),
                (200, "Video title"),
                (300, "URL"),
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
                .padding(iced::Padding {
                    top: 0.0,
                    right: 16.0,
                    bottom: 16.0,
                    left: 16.0,
                }),
            )
            .height(Length::Fill)
            .into()
        };

        column![filters, horizontal_rule(1), content]
            .height(Length::Fill)
            .into()
    }
}

// ─── Async tasks ─────────────────────────────────────────────────────────────
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
