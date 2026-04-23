use crate::DB_PATH;
use cratefm_core::database::Db;
use cratefm_core::database::images::ImageRow;
use iced::widget::{
    Space, button, column, container, horizontal_rule, row, scrollable, text, text_input,
};
use iced::{Alignment, Element, Length, Task};

#[derive(Debug, Clone)]
pub enum Message {
    Search(String),
    Refresh,
    ImageLoaded(Result<Vec<ImageRow>, String>),
}

pub struct ImagesPage {
    search: String,
    rows: Vec<ImageRow>,
    error: Option<String>,
}

impl ImagesPage {
    pub fn new() -> ImagesPage {
        ImagesPage {
            search: String::new(),
            rows: vec![],
            error: None,
        }
    }

    pub fn load(&self) -> Task<Message> {
        load_images()
    }

    pub fn update(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::Search(v) => {
                self.search = v;
                Task::none()
            }
            Message::Refresh => load_images(),
            Message::ImageLoaded(result) => {
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

    pub fn view_images(&self) -> Element<'_, Message> {
        let filters = row![
            text_input("Search artist / URL…", &self.search)
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
                .filter(|ir| {
                    search.is_empty()
                        || ir.image.url.to_lowercase().contains(&search)
                        || ir.release_artist.to_lowercase().contains(&search)
                        || ir.release_title.to_lowercase().contains(&search)
                })
                .collect();

            let header = table_row(vec![
                (50, "ID"),
                (300, "URL"),
                (100, "Type"),
                (150, "Artist"),
                (180, "Release"),
                (80, "Width"),
                (80, "Height"),
            ]);

            let rows: Vec<Element<Message>> = visible
                .iter()
                .map(|ir| {
                    table_row(vec![
                        (50, ir.image.id.to_string()),
                        (300, ir.image.url.to_string()),
                        (100, ir.image.image_type.to_string()),
                        (150, ir.release_artist.clone()),
                        (180, ir.release_title.clone()),
                        (80, ir.image.width.to_string()),
                        (80, ir.image.height.to_string()),
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

fn load_images() -> Task<Message> {
    Task::perform(
        async move {
            let db = Db::open(DB_PATH).map_err(|e| e.to_string())?;
            db.list_all_images().map_err(|e| e.to_string())
        },
        Message::ImageLoaded,
    )
}
