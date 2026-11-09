use askama::Template;
use scorarium_engine::entity::Entity;
use scorarium_engine::person::Person;
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
    /// The URL that removes the picture, or `None` when there is no picture to remove
    remove_url: Option<String>,
}

impl askama::filters::HtmlSafe for PictureBox {}

impl PictureBox {
    pub fn cover(publication: &Publication, files_allowed: bool) -> PictureBox {
        let api = format!(
            "/api/library/{}/publication/{}/cover",
            publication.library(),
            publication.id()
        );
        PictureBox {
            legend: "Cover",
            picture: Picture::from(publication),
            input: FileInput {
                url: format!("{api}/upload"),
                accept: Some("image/*"),
                hint: "Paste a picture or URL",
                disabled: !files_allowed,
                multiple: false,
            },
            remove_url: publication.cover().map(|_| format!("{api}/remove")),
        }
    }

    pub fn profile(person: &Person, files_allowed: bool) -> PictureBox {
        let api = format!(
            "/api/library/{}/person/{}/profile",
            person.library(),
            person.id()
        );
        PictureBox {
            legend: "Profile picture",
            picture: Picture::from(person),
            input: FileInput {
                url: format!("{api}/upload"),
                accept: Some("image/*"),
                hint: "Paste a picture or URL",
                disabled: !files_allowed,
                multiple: false,
            },
            remove_url: person.profile().map(|_| format!("{api}/remove")),
        }
    }
}
