use crate::DB_PATH;
use cratefm_core::database::Db;
use iced::widget::{
    Space, button, column, container, horizontal_rule, pick_list, row, scrollable, text, text_input,
};
use iced::{Alignment, Element, Length, Task};
use cratefm_core::database::releases::{ReleaseRow, ReleaseStatus};

#[derive(Debug, Clone)]
pub enum Message {
    StatusChanged(ReleaseStatus),
    Search(String),
    Refresh,
    ReleaseLoaded(Result<Vec<ReleaseRow>, String>),
}
pub struct ReleasesPage {
    status: ReleaseStatus,
    search: String,
    rows: Vec<ReleaseRow>,
    error: Option<String>,
}

impl ReleasesPage {
    pub fn new() -> ReleasesPage {
        ReleasesPage {
            status: ReleaseStatus::ToListen,
            search: String::new(),
            rows: vec![],
            error: None,
        }
    }

    pub fn load(&self) -> Task<Message> {
        load_releases(self.status.clone())
    }

    pub fn update(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::StatusChanged(status) => {
                self.status = status.clone();
                load_releases(status)
            }
            Message::Search(v) => {
                self.search = v;
                Task::none()
            }
            Message::Refresh => load_releases(self.status.clone()),
            Message::ReleaseLoaded(result) => {
                match result {
                    Ok(rows) => {
                        self.rows = rows;
                        self.error = None;
                    }
                    Err(e) => self.error = Some(e),
                }
                Task::none()
            }
        }
    }

    pub fn view_releases(&self) -> Element<'_, Message> {
        let status_opts = vec![
            ReleaseStatus::ToListen,
            ReleaseStatus::Liked,
            ReleaseStatus::Disliked,
        ];

        let filters = row![
            text("Status:"),
            pick_list(
                status_opts,
                Some(self.status.clone()),
                Message::StatusChanged
            ),
            text_input("Search artist / title…", &self.search)
                .on_input(Message::Search)
                .width(280),
            Space::with_width(Length::Fill),
            button("Refresh").on_press(Message::Refresh),
        ]
        .spacing(8)
        .padding([8, 16])
        .align_y(Alignment::Center);

        let content: Element<Message> = if let Some(err) = &self.error {
            container(text(format!("Error: {err}"))).padding(16).into()
        } else {
            let search = self.search.to_lowercase();
            let visible: Vec<_> = self
                .rows
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
                        (250, r.url.clone()),
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

        iced::widget::column![filters, horizontal_rule(1), content]
            .height(Length::Fill)
            .into()
    }
}

fn load_releases(status: ReleaseStatus) -> Task<Message> {
    Task::perform(
        async move {
            let db = Db::open(DB_PATH).map_err(|e| e.to_string())?;
            db.list_releases(Some(&status)).map_err(|e| e.to_string())
        },
        Message::ReleaseLoaded,
    )
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
