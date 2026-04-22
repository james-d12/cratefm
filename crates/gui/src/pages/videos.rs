use crate::DB_PATH;
use cratefm_core::db::Db;
use cratefm_core::models::VideoRow;
use iced::widget::{
    Space, button, column, container, horizontal_rule, row, scrollable, text, text_input,
};
use iced::{Alignment, Element, Length, Task};

#[derive(Debug, Clone)]
pub enum Message {
    VidSearch(String),
    VidRefresh,
    VidLoaded(Result<Vec<VideoRow>, String>),
}

pub struct VideosPage {
    vid_search: String,
    vid_rows: Vec<VideoRow>,
    vid_error: Option<String>,
}

impl VideosPage {
    pub fn new() -> VideosPage {
        VideosPage {
            vid_search: String::new(),
            vid_rows: vec![],
            vid_error: None,
        }
    }

    pub fn update(&mut self, msg: Message) -> Task<Message> {
        match msg {
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
        }
    }

    pub fn view_videos(&self) -> Element<'_, Message> {
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
        Message::VidLoaded,
    )
}
