use std::collections::HashSet;

use askama::Template;
use scorarium_engine::entity::{Contributed, Entity};
use scorarium_engine::library::LibraryId;
use scorarium_engine::person::PersonId;
use scorarium_engine::publication::{Publication, PublicationId};
use scorarium_engine::value::Classification;
use scorarium_engine::work::{Work, WorkId};

use crate::page::library::entity::{ContainedWork, book_order, is_primary};
use crate::widget::typeahead::Typeahead;

/// The works the most recent action created or linked
#[derive(Debug, Default)]
pub struct Marks {
    pub created: HashSet<WorkId>,
    pub linked: HashSet<WorkId>,
}

/// The add form's values
#[derive(Debug)]
pub struct AddFields {
    pub name: String,
    pub person: String,
    pub title: String,
    pub work: String,
    pub catalog_number: String,
    pub role: String,
}

/// A publication's works, added and removed in place with each change saved immediately
#[derive(Debug, Template)]
#[template(path = "widget/contents.html")]
pub struct Contents {
    pub library: LibraryId,
    pub publication: PublicationId,
    /// The edit page the box is on, carried as `back` by the edit pencils
    pub back: String,
    /// The works in book order
    pub works: Vec<ContainedWork>,
    /// Whether any work has a catalog number
    pub numbered: bool,
    pub marks: Marks,
    pub add: AddFields,
    pub error: Option<String>,
}

impl Contents {
    /// Build the box with no marks, no error, and the pre-filled add form
    pub fn new(publication: &Publication, works: &[Work]) -> Contents {
        let mut contained: Vec<ContainedWork> = works.iter().map(Into::into).collect();
        book_order(&mut contained);
        let numbered = contained.iter().any(|work| work.catalog_number.is_some());
        Contents {
            library: publication.library(),
            publication: publication.id(),
            back: format!(
                "/library/{}/publication/{}/edit",
                publication.library(),
                publication.id()
            ),
            works: contained,
            numbered,
            marks: Marks::default(),
            add: prefill(publication, works),
            error: None,
        }
    }

    fn api(&self) -> String {
        format!(
            "/api/library/{}/publication/{}",
            self.library, self.publication
        )
    }

    /// Build the person picker, whose pick is posted as `person` and whose free text is a new person
    fn person(&self) -> Typeahead {
        Typeahead {
            id: "contents-name".to_owned(),
            name: "name".to_owned(),
            url: format!("/api/library/{}/suggest/person", self.library),
            placeholder: "Person".to_owned(),
            value: self.add.name.clone(),
            picked: Some(("person", self.add.person.clone())),
            pick_required: None,
            attrs: vec![],
        }
    }

    /// Build the work picker, narrowed to the picked person's works
    fn work(&self) -> Typeahead {
        Typeahead {
            id: "contents-title".to_owned(),
            name: "title".to_owned(),
            url: format!("/api/library/{}/suggest/work", self.library),
            placeholder: "Work".to_owned(),
            value: self.add.title.clone(),
            picked: Some(("work", self.add.work.clone())),
            pick_required: None,
            attrs: vec![
                ("data-typeahead-with", "person".to_owned()),
                // A typed title is more often a new work than an existing one.
                ("data-no-default-pick", String::new()),
                ("required", String::new()),
            ],
        }
    }

    /// Build the role field, which suggests the library's roles and the conventional five
    fn role(&self) -> Typeahead {
        Typeahead {
            id: "contents-role".to_owned(),
            name: "role".to_owned(),
            url: format!("/api/library/{}/suggest/role", self.library),
            placeholder: "Role".to_owned(),
            value: self.add.role.clone(),
            picked: None,
            pick_required: None,
            attrs: vec![("required", String::new())],
        }
    }
}

/// Pre-fill the sole composer or author across the publication and its works, if there is one
fn prefill(publication: &Publication, works: &[Work]) -> AddFields {
    let primary: Vec<(PersonId, &str, &str)> = publication
        .contributors()
        .iter()
        .chain(works.iter().flat_map(|work| work.contributors()))
        .filter(|c| is_primary(&c.role))
        .map(|c| (c.person, c.name.as_str(), c.role.as_str()))
        .collect();
    let (person, name, role) = match primary.first() {
        Some(&(person, name, role)) if primary.iter().all(|(p, _, _)| *p == person) => {
            (person.to_string(), name.to_owned(), role.to_owned())
        }
        _ => {
            let role = match publication.classification() {
                Some(Classification::Music) => "composer",
                _ => "author",
            };
            (String::new(), String::new(), role.to_owned())
        }
    };
    AddFields {
        name,
        person,
        title: String::new(),
        work: String::new(),
        catalog_number: String::new(),
        role,
    }
}

impl askama::filters::HtmlSafe for Contents {}
