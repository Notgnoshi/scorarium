use askama::Template;
use scorarium_engine::contributor::Contributor;
use scorarium_engine::entity::Contributed;
use scorarium_engine::library::LibraryId;

use crate::widget::typeahead::Typeahead;

/// An entity's contributors, edited in place with each change saved immediately
#[derive(Debug, Template)]
#[template(path = "widget/contributors.html")]
pub struct Contributors {
    pub library: LibraryId,
    /// The route prefix the forms post under
    pub api: String,
    /// The edit page this widget is on, to which edits of a contributor return
    pub back: String,
    /// One entry per person and role pairing
    pub contributors: Vec<Contributor>,
    pub error: Option<String>,
}

impl Contributors {
    pub fn new<E: Contributed>(entity: &E, error: Option<String>) -> Contributors {
        Contributors {
            library: entity.library(),
            api: format!(
                "/api/library/{}/{}/{}",
                entity.library(),
                E::KIND,
                entity.id()
            ),
            back: format!(
                "/library/{}/{}/{}/edit",
                entity.library(),
                E::KIND,
                entity.id()
            ),
            contributors: entity.contributors().to_vec(),
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
