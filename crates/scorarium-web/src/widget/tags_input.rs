use askama::Template;
use scorarium_engine::library::LibraryId;
use scorarium_engine::value::Tag;

use crate::widget::typeahead::Typeahead;

/// An editable set of tags that posts `add_tag` and `remove_tag` for each change
#[derive(Debug, Template)]
#[template(path = "widget/tags_input.html")]
pub struct TagsInput {
    pub library: LibraryId,
    pub tags: Vec<Tag>,
}

impl TagsInput {
    /// Build the add box, which suggests the library's tags
    fn typeahead(&self) -> Typeahead {
        Typeahead {
            id: "tag".to_owned(),
            name: "tag".to_owned(),
            url: format!("/api/library/{}/suggest/tag", self.library),
            placeholder: "Add a tag".to_owned(),
            value: String::new(),
            picked: None,
            pick_required: None,
            // The pattern mirrors the engine's tag rule, so the browser refuses a tag that would
            // fail the whole Save. Uppercase passes because the engine lowercases it.
            attrs: vec![
                ("data-add", String::new()),
                ("pattern", "[A-Za-z0-9_-]+".to_owned()),
                (
                    "title",
                    "Letters, digits, hyphens, and underscores".to_owned(),
                ),
            ],
        }
    }
}

impl askama::filters::HtmlSafe for TagsInput {}
