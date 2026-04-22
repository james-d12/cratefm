mod pages;

use crate::pages::fetch::FetchPage;
use crate::pages::listen::ListenPage;
use crate::pages::releases::ReleasesPage;
use crate::pages::videos::VideosPage;
use crate::pages::{fetch, listen, releases, videos};
use cratefm_core::models::ReleaseStatus;
use iced::{
    Alignment, Element, Length, Task, Theme,
    widget::{Space, button, column, horizontal_rule, row, text},
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

// ─── App state ────────────────────────────────────────────────────────────────

struct App {
    page: Page,

    listen_page: ListenPage,
    fetch_page: FetchPage,
    videos_page: VideosPage,
    releases_page: ReleasesPage,
}

// ─── Messages ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Message {
    // Navigation
    GoFetch,
    GoReleases,
    GoVideos,
    GoListen,

    // Listen
    Listen(listen::Message),
    Fetch(fetch::Message),
    Videos(videos::Message),
    Releases(releases::Message),
}

// ─── App impl ────────────────────────────────────────────────────────────────

impl App {
    fn new() -> (Self, Task<Message>) {
        let app = Self {
            page: Page::Releases,
            listen_page: ListenPage::new(),
            fetch_page: FetchPage::new(),
            videos_page: VideosPage::new(),
            releases_page: ReleasesPage::new(),
        };
        let task = releases::load_releases(ReleaseStatus::ToListen).map(Message::Releases);
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
                Task::none()
            }
            Message::GoVideos => {
                self.page = Page::Videos;
                Task::none()
            }
            Message::GoListen => {
                self.page = Page::Listen;
                Task::none()
            }

            Message::Listen(msg) => self.listen_page.update(msg).map(Message::Listen),
            Message::Fetch(msg) => self.fetch_page.update(msg).map(Message::Fetch),
            Message::Videos(msg) => self.videos_page.update(msg).map(Message::Videos),
            Message::Releases(msg) => self.releases_page.update(msg).map(Message::Releases),
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
            Page::Releases => self.releases_page.view_releases().map(Message::Releases),
            Page::Videos => self.videos_page.view_videos().map(Message::Videos),
            Page::Listen => self.listen_page.view_listen().map(Message::Listen),
        };

        column![nav, horizontal_rule(1), body]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
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
