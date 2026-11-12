use askama::Template;
use scorarium_engine::Engine;
use scorarium_engine::asset::StagedId;
use scorarium_engine::person::Person;
use scorarium_engine::publication::Publication;
use serde::Deserialize;

use crate::widget::file_input::FileInput;
use crate::widget::picture::Picture;

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct PictureForm {
    picture: String,
    remove_picture: Option<String>,
}

pub(crate) enum PictureEdit {
    Keep,
    Store(StagedId),
    Clear,
}

impl PictureForm {
    pub fn parse(&self) -> PictureEdit {
        if !self.picture.is_empty() {
            let Ok(id) = self.picture.parse();
            PictureEdit::Store(id)
        } else if self.remove_picture.is_some() {
            PictureEdit::Clear
        } else {
            PictureEdit::Keep
        }
    }
}

#[derive(Template)]
#[template(path = "widget/picture_input.html")]
pub(crate) struct PictureInput {
    picture: Picture,
    /// The staged file that the form will store as the picture once submitted
    staged: Option<StagedId>,
    remove: bool,
    input: FileInput,
}

impl askama::filters::HtmlSafe for PictureInput {}

impl PictureInput {
    /// Show the person's stored profile picture, or the placeholder
    pub fn profile(person: &Person, files_allowed: bool) -> PictureInput {
        PictureInput::new(Picture::from(person), files_allowed)
    }

    /// Show the publication's stored cover, or the placeholder
    pub fn cover(publication: &Publication, files_allowed: bool) -> PictureInput {
        PictureInput::new(Picture::from(publication), files_allowed)
    }

    fn new(picture: Picture, files_allowed: bool) -> PictureInput {
        PictureInput {
            picture,
            staged: None,
            remove: false,
            input: FileInput {
                url: "/api/stage".to_owned(),
                accept: Some("image/*"),
                hint: "Paste a picture or URL",
                disabled: !files_allowed,
                multiple: false,
            },
        }
    }

    /// Draw the input as a submitted form left it, for a form that was not saved
    pub fn resubmitted(mut self, engine: &Engine, form: &PictureForm) -> PictureInput {
        let Ok(id) = form.picture.parse::<StagedId>();
        if !form.picture.is_empty() && engine.staged(&id).is_some() {
            self.picture = self.picture.showing(Some(format!("/api/stage/{id}")));
            self.staged = Some(id);
        } else if form.remove_picture.is_some() {
            self.picture = self.picture.showing(None);
            self.remove = true;
        }
        self
    }

    /// Draw the picture that the script shows for a newly staged file, once it has a source
    fn staged_template(&self) -> Picture {
        self.picture.clone().showing(Some(String::new()))
    }

    /// Draw the placeholder that the script shows when the picture is removed
    fn placeholder_template(&self) -> Picture {
        self.picture.clone().showing(None)
    }
}
