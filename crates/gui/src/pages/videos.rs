use crate::DB_PATH;
use cratefm_core::database::Db;
use iced::widget::{
    Space, button, column, container, horizontal_rule, row, scrollable, text, text_input,
};
use iced::{Alignment, Element, Length, Task};
use cratefm_core::database::videos::VideoRow;

#[derive(Debug, Clone)]
pub enum Message {
    Search(String),
    Refresh,
    VideoLoaded(Result<Vec<VideoRow>, String>),
}

pub struct VideosPage {
    search: String,
    rows: Vec<VideoRow>,
    error: Option<String>,
}

impl VideosPage {
    pub fn new() -> VideosPage {
        VideosPage {
            search: String::new(),
            rows: vec![],
            error: None,
        }
    }

    pub fn load(&self) -> Task<Message> {
        load_videos()
    }

    pub fn update(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::Search(v) => {
                self.search = v;
                Task::none()
            }
            Message::Refresh => load_videos(),
            Message::VideoLoaded(result) => {
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

    pub fn view_videos(&self) -> Element<'_, Message> {
        let filters = row![
            text_input("Search artist / title / URL…", &self.search)
                .on_input(Message::Search)
                .width(380),
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

        iced::widget::column![filters, horizontal_rule(1), content]
            .height(Length::Fill)
            .into()
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

fn load_videos() -> Task<Message> {
    Task::perform(
        async move {
            let db = Db::open(DB_PATH).map_err(|e| e.to_string())?;
            db.list_all_videos().map_err(|e| e.to_string())
        },
        Message::VideoLoaded,
    )
}
