use askama::Template;
use scorarium_engine::entity::Entity;
use scorarium_engine::publication::Publication;

use crate::widget::file_input::FileInput;
use crate::widget::picture::Picture;

/// The box on an edit page that shows a picture and changes it
#[derive(Template)]
#[template(path = "widget/picture_box.html")]
pub(crate) struct PictureBox {
    legend: &'static str,
    picture: Picture,
    input: FileInput,
}

impl askama::filters::HtmlSafe for PictureBox {}

impl PictureBox {
    pub fn cover(publication: &Publication, files_allowed: bool) -> PictureBox {
        PictureBox {
            legend: "Cover",
            picture: Picture::from(publication),
            input: FileInput {
                url: format!(
                    "/api/library/{}/publication/{}/cover/upload",
                    publication.library(),
                    publication.id()
                ),
                accept: Some("image/*"),
                disabled: !files_allowed,
            },
        }
    }
}
