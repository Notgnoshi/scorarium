use askama::Template;
use scorarium_engine::Engine;
use scorarium_engine::asset::StagedId;
use scorarium_engine::link::HttpUrl;
use scorarium_engine::person::Person;
use scorarium_engine::publication::Publication;
use serde::Deserialize;

use crate::icon::Icon;
use crate::proposal::Candidate;
use crate::widget::file_input::FileInput;
use crate::widget::picture::Picture;
use crate::widget::refusals::Refusals;

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct PictureForm {
    picture: String,
    remove_picture: Option<String>,
    picture_url: String,
}

pub(crate) enum PictureEdit {
    Keep,
    Store(StagedId),
    Clear,
}

impl PictureForm {
    pub fn parse(&self, refusals: &mut Refusals) -> Option<PictureEdit> {
        if !self.picture_url.trim().is_empty() {
            refusals.refuse_field("picture_url", "Not fetched.");
            return None;
        }
        Some(if !self.picture.is_empty() {
            let Ok(id) = self.picture.parse();
            PictureEdit::Store(id)
        } else if self.remove_picture.is_some() {
            PictureEdit::Clear
        } else {
            PictureEdit::Keep
        })
    }
}

#[derive(Template)]
#[template(path = "widget/picture_candidates.html")]
pub(crate) struct PictureCandidates {
    candidates: Vec<Candidate<HttpUrl>>,
}

impl PictureCandidates {
    pub fn new(candidates: Vec<Candidate<HttpUrl>>) -> PictureCandidates {
        PictureCandidates { candidates }
    }
}

impl askama::filters::HtmlSafe for PictureCandidates {}

#[derive(Template)]
#[template(path = "widget/picture_input.html")]
pub(crate) struct PictureInput {
    src: Option<String>,
    icon: Icon,
    label: &'static str,
    remove: bool,
    input: FileInput,
    candidates: PictureCandidates,
}

impl askama::filters::HtmlSafe for PictureInput {}

impl PictureInput {
    pub fn profile(
        person: &Person,
        files_allowed: bool,
        candidates: PictureCandidates,
    ) -> PictureInput {
        PictureInput::new(
            Picture::from(person),
            "Profile picture",
            files_allowed,
            candidates,
        )
    }

    pub fn cover(
        publication: &Publication,
        files_allowed: bool,
        candidates: PictureCandidates,
    ) -> PictureInput {
        PictureInput::new(
            Picture::from(publication),
            "Cover",
            files_allowed,
            candidates,
        )
    }

    fn new(
        picture: Picture,
        label: &'static str,
        files_allowed: bool,
        candidates: PictureCandidates,
    ) -> PictureInput {
        PictureInput {
            src: picture.src().map(str::to_owned),
            icon: picture.icon(),
            label,
            remove: false,
            input: FileInput {
                url: "/api/stage".to_owned(),
                accept: Some("image/*"),
                multiple: false,
                hint: "Paste a URL",
                disabled: !files_allowed,
                staged_name: "picture".to_owned(),
                staged: None,
                shows_file: false,
                url_name: "picture_url".to_owned(),
                url_text: String::new(),
                refusal: None,
                browse_icon: Some("Browse for a picture"),
            },
            candidates,
        }
    }

    /// Draw the input as a submitted form left it, for a form that was not saved
    pub fn resubmitted(
        mut self,
        engine: &Engine,
        form: &PictureForm,
        refusals: &Refusals,
    ) -> PictureInput {
        self.input.url_text.clone_from(&form.picture_url);
        self.input.refusal = refusals.of("picture_url").map(str::to_owned);
        let Ok(id) = form.picture.parse::<StagedId>();
        let staged = engine.staged(&id).filter(|_| !form.picture.is_empty());
        if staged.is_some() {
            self.src = Some(format!("/api/stage/{id}"));
            self.input.staged = staged;
        } else if form.remove_picture.is_some() {
            self.src = None;
            self.remove = true;
        }
        self
    }
}
