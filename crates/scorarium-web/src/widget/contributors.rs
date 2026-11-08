use askama::Template;
use scorarium_engine::contributor::Contributor;
use scorarium_engine::entity::Entity;
use scorarium_engine::library::LibraryId;
use scorarium_engine::work::{Work, WorkId};

use crate::widget::typeahead::Typeahead;

/// A work's contributors, edited in place with each change saved immediately
#[derive(Debug, Template)]
#[template(path = "widget/contributors.html")]
pub struct Contributors {
    pub library: LibraryId,
    pub work: WorkId,
    /// One entry per person and role pairing
    pub contributors: Vec<Contributor>,
    pub error: Option<String>,
}

impl Contributors {
    pub fn new(work: &Work, error: Option<String>) -> Contributors {
        Contributors {
            library: work.library(),
            work: work.id(),
            contributors: work.contributors().to_vec(),
            error,
        }
    }

    /// Build the person picker, whose pick is posted as `person`
    fn person(&self) -> Typeahead {
        Typeahead {
            name: "name".to_owned(),
            url: format!("/api/library/{}/suggest/person", self.library),
            placeholder: "Person".to_owned(),
            value: String::new(),
            picked_id: Some("person".to_owned()),
            pick_required: Some("Pick a person from the list".to_owned()),
            attrs: vec![],
        }
    }

    /// Build the role field, which suggests the library's roles and the conventional five
    fn role(&self) -> Typeahead {
        Typeahead {
            name: "role".to_owned(),
            url: format!("/api/library/{}/suggest/role", self.library),
            placeholder: "Role".to_owned(),
            value: String::new(),
            picked_id: None,
            pick_required: None,
            attrs: vec![("required", String::new())],
        }
    }
}

impl askama::filters::HtmlSafe for Contributors {}
