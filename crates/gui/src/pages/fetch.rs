use crate::DB_PATH;
use cratefm_core::db::Db;
use cratefm_core::discogs::fetch_releases;
use cratefm_core::models::FetchParams;
use iced::widget::{Column, button, container, row, text, text_input};
use iced::{Alignment, Element, Task};

#[derive(Debug, Clone)]
pub enum Message {
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
}

#[derive(Debug, Clone)]
enum FetchState {
    Idle,
    Running,
    Done { releases: usize, videos: usize },
    Error(String),
}

pub struct FetchPage {
    f_token: String,
    f_genre: String,
    f_style: String,
    f_year: String,
    f_limit: String,
    f_min_owners: String,
    f_max_owners: String,
    f_min_rating: String,
    fetch_state: FetchState,
}

impl FetchPage {
    pub fn new() -> FetchPage {
        FetchPage {
            f_token: String::new(),
            f_genre: "Electronic".into(),
            f_style: "Jungle".into(),
            f_year: "1995".into(),
            f_limit: "10".into(),
            f_min_owners: "10".into(),
            f_max_owners: String::new(),
            f_min_rating: "4.0".into(),
            fetch_state: FetchState::Idle,
        }
    }

    pub fn update(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::FToken(v) => {
                self.f_token = v;
                Task::none()
            }
            Message::FGenre(v) => {
                self.f_genre = v;
                Task::none()
            }
            Message::FStyle(v) => {
                self.f_style = v;
                Task::none()
            }
            Message::FYear(v) => {
                self.f_year = v;
                Task::none()
            }
            Message::FLimit(v) => {
                self.f_limit = v;
                Task::none()
            }
            Message::FMinOwners(v) => {
                self.f_min_owners = v;
                Task::none()
            }
            Message::FMaxOwners(v) => {
                self.f_max_owners = v;
                Task::none()
            }
            Message::FMinRating(v) => {
                self.f_min_rating = v;
                Task::none()
            }

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
                    Ok((r, v)) => FetchState::Done {
                        releases: r,
                        videos: v,
                    },
                    Err(e) => FetchState::Error(e),
                };
                Task::none()
            }
        }
    }

    pub fn view_fetch(&self) -> Element<'_, Message> {
        let is_running = matches!(self.fetch_state, FetchState::Running);

        let form: Column<Message> = iced::widget::column![
            labeled_field(
                "Token",
                text_input("Discogs API token", &self.f_token)
                    .on_input(Message::FToken)
                    .secure(true)
            ),
            labeled_field(
                "Genre",
                text_input("e.g. Electronic", &self.f_genre).on_input(Message::FGenre)
            ),
            labeled_field(
                "Style",
                text_input("e.g. Techno", &self.f_style).on_input(Message::FStyle)
            ),
            labeled_field(
                "Year",
                text_input("e.g. 2020", &self.f_year).on_input(Message::FYear)
            ),
            labeled_field(
                "Limit",
                text_input("number of releases", &self.f_limit).on_input(Message::FLimit)
            ),
            labeled_field(
                "Min owners",
                text_input("e.g. 10", &self.f_min_owners).on_input(Message::FMinOwners)
            ),
            labeled_field(
                "Max owners",
                text_input("e.g. 500", &self.f_max_owners).on_input(Message::FMaxOwners)
            ),
            labeled_field(
                "Min rating",
                text_input("optional, e.g. 3.5", &self.f_min_rating).on_input(Message::FMinRating)
            ),
        ]
        .spacing(10)
        .max_width(520);

        let fetch_btn = {
            let b = button(if is_running {
                "Fetching…"
            } else {
                "Fetch releases"
            })
            .padding([8, 20]);
            if is_running {
                b
            } else {
                b.on_press(Message::DoFetch)
            }
        };

        let status_line: Element<Message> = match &self.fetch_state {
            FetchState::Idle => text("").into(),
            FetchState::Running => {
                text("Fetching releases from Discogs — this may take a while…").into()
            }
            FetchState::Done { releases, videos } => text(format!(
                "Done — {releases} releases and {videos} videos saved."
            ))
            .into(),
            FetchState::Error(e) => text(format!("Error: {e}")).into(),
        };

        container(
            iced::widget::column![form, fetch_btn, status_line]
                .spacing(16)
                .padding(24),
        )
        .into()
    }
}

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

fn labeled_field<'a>(
    label: &'a str,
    input: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    row![text(label).width(100), container(input.into()).width(320),]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
}
