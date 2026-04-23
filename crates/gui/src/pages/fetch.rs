use crate::DB_PATH;
use cratefm_core::db::Db;
use cratefm_core::discos::fetch_releases;
use cratefm_core::models::FetchParams;
use iced::widget::{Column, button, container, row, text, text_input};
use iced::{Alignment, Element, Task};

#[derive(Debug, Clone)]
pub enum Message {
    TokenChanged(String),
    GenreChanged(String),
    StyleChanged(String),
    YearChanged(String),
    LimitChanged(String),
    MinOwnersChanged(String),
    MaxOwnersChanged(String),
    MinRatingChanged(String),
    FetchRequested,
    FetchCompleted(Result<(usize, usize), String>),
}

#[derive(Debug, Clone)]
enum FetchState {
    Idle,
    Running,
    Done { releases: usize, videos: usize },
    Error(String),
}

struct FetchForm {
    token: String,
    genre: String,
    style: String,
    year: String,
    limit: String,
    min_owners: String,
    max_owners: String,
    min_rating: String,
}

pub struct FetchPage {
    fetch_form: FetchForm,
    fetch_state: FetchState,
}

impl FetchPage {
    pub fn new() -> FetchPage {
        FetchPage {
            fetch_form: FetchForm {
                token: String::new(),
                genre: "Electronic".into(),
                style: "Jungle".into(),
                year: "1995".into(),
                limit: "10".into(),
                min_owners: "10".into(),
                max_owners: String::new(),
                min_rating: "4.0".into(),
            },
            fetch_state: FetchState::Idle,
        }
    }

    pub fn update(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::TokenChanged(v) => {
                self.fetch_form.token = v;
                Task::none()
            }
            Message::GenreChanged(v) => {
                self.fetch_form.genre = v;
                Task::none()
            }
            Message::StyleChanged(v) => {
                self.fetch_form.style = v;
                Task::none()
            }
            Message::YearChanged(v) => {
                self.fetch_form.year = v;
                Task::none()
            }
            Message::LimitChanged(v) => {
                self.fetch_form.limit = v;
                Task::none()
            }
            Message::MinOwnersChanged(v) => {
                self.fetch_form.min_owners = v;
                Task::none()
            }
            Message::MaxOwnersChanged(v) => {
                self.fetch_form.max_owners = v;
                Task::none()
            }
            Message::MinRatingChanged(v) => {
                self.fetch_form.min_rating = v;
                Task::none()
            }

            Message::FetchRequested => {
                let params = FetchParams {
                    token: self.fetch_form.token.clone(),
                    genre: self.fetch_form.genre.clone(),
                    style: self.fetch_form.style.clone(),
                    year: self.fetch_form.year.parse().unwrap_or(2020),
                    limit: self.fetch_form.limit.parse().unwrap_or(10),
                    min_owners: self.fetch_form.min_owners.parse().unwrap_or(10),
                    max_owners: self.fetch_form.max_owners.parse().ok(),
                    min_rating: self.fetch_form.min_rating.parse().ok(),
                };
                self.fetch_state = FetchState::Running;
                Task::perform(do_fetch(params), Message::FetchCompleted)
            }
            Message::FetchCompleted(result) => {
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
                text_input("Discogs API token", &self.fetch_form.token)
                    .on_input(Message::TokenChanged)
                    .secure(true)
            ),
            labeled_field(
                "Genre",
                text_input("e.g. Electronic", &self.fetch_form.genre)
                    .on_input(Message::GenreChanged)
            ),
            labeled_field(
                "Style",
                text_input("e.g. Techno", &self.fetch_form.style).on_input(Message::StyleChanged)
            ),
            labeled_field(
                "Year",
                text_input("e.g. 2020", &self.fetch_form.year).on_input(Message::YearChanged)
            ),
            labeled_field(
                "Limit",
                text_input("number of releases", &self.fetch_form.limit)
                    .on_input(Message::LimitChanged)
            ),
            labeled_field(
                "Min owners",
                text_input("e.g. 10", &self.fetch_form.min_owners)
                    .on_input(Message::MinOwnersChanged)
            ),
            labeled_field(
                "Max owners",
                text_input("e.g. 500", &self.fetch_form.max_owners)
                    .on_input(Message::MaxOwnersChanged)
            ),
            labeled_field(
                "Min rating",
                text_input("optional, e.g. 3.5", &self.fetch_form.min_rating)
                    .on_input(Message::MinRatingChanged)
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
                b.on_press(Message::FetchRequested)
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

    let releases = fetch_releases(&params, &known_ids, start_page)
        .await
        .map_err(|e| e.to_string())?;

    db.save_releases(&releases.releases).map_err(|e| e.to_string())?;
    db.save_videos(&releases.videos).map_err(|e| e.to_string())?;
    db.save_images(&releases.images).map_err(|e| e.to_string())?;
    db.set_cursor(&params.genre, &params.style, params.year, releases.next_page)
        .map_err(|e| e.to_string())?;
    Ok((releases.releases.len(), releases.videos.len()))
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
