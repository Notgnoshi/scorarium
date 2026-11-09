use askama::Template;
use scorarium_engine::publication::Publication;

use crate::widget::picture::Picture;

/// A box that holds a picture edit widget
#[derive(Template)]
#[template(path = "widget/picture_box.html")]
pub(crate) struct PictureBox {
    legend: &'static str,
    picture: Picture,
}

impl askama::filters::HtmlSafe for PictureBox {}

impl PictureBox {
    pub fn cover(publication: &Publication) -> PictureBox {
        PictureBox {
            legend: "Cover",
            picture: Picture::from(publication),
        }
    }
}
