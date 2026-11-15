use askama::Template;
use axum::extract::State;
use axum::response::Html;
use scorarium_engine::Engine;
use scorarium_engine::duplicate::DuplicateReason;
use scorarium_engine::entity::Entity;
use scorarium_engine::library::{Library, LibraryId};
use scorarium_engine::person::Person;
use scorarium_engine::publication::Publication;
use scorarium_engine::value::title_sort_key;

use crate::auth::Session;
use crate::duplicate::describe_reasons;
use crate::error::Result;
use crate::page::library::entity::ContainedWork;
use crate::page::{Base, Crumb};
use crate::widget::header::Header;

/// A group of potential duplicates in one library
struct Group<M> {
    library: LibraryId,
    library_name: String,
    reasons: String,
    members: Vec<M>,
}

impl<M> Group<M> {
    fn new(library: &Library, reasons: &[DuplicateReason], members: Vec<M>) -> Group<M> {
        Group {
            library: library.id(),
            library_name: library.name().to_owned(),
            reasons: describe_reasons(reasons),
            members,
        }
    }
}

#[derive(Template)]
#[template(path = "page/duplicates.html")]
struct Duplicates {
    base: Base,
    people: Vec<Group<Person>>,
    works: Vec<Group<ContainedWork>>,
    publications: Vec<Group<Publication>>,
}

pub async fn get(
    _session: Session,
    State(engine): State<Engine>,
    header: Header,
) -> Result<Html<String>> {
    let mut people = Vec::new();
    let mut works = Vec::new();
    let mut publications = Vec::new();
    for library in engine.libraries().await? {
        let mut groups: Vec<_> = library
            .duplicate_people()
            .await?
            .into_iter()
            .map(|g| Group::new(&library, &g.reasons, g.entities))
            .collect();
        groups.sort_by_key(|g| g.members[0].name().to_lowercase());
        people.append(&mut groups);

        let mut groups: Vec<_> = library
            .duplicate_works()
            .await?
            .into_iter()
            .map(|g| {
                let members = g.entities.iter().map(ContainedWork::from).collect();
                Group::new(&library, &g.reasons, members)
            })
            .collect();
        groups.sort_by_key(|g| title_sort_key(&g.members[0].title));
        works.append(&mut groups);

        let mut groups: Vec<_> = library
            .duplicate_publications()
            .await?
            .into_iter()
            .map(|g| Group::new(&library, &g.reasons, g.entities))
            .collect();
        groups.sort_by_key(|g| title_sort_key(g.members[0].title().unwrap_or("Untitled")));
        publications.append(&mut groups);
    }

    let page = Duplicates {
        base: Base::new(
            header,
            "Potential duplicates",
            vec![
                Crumb::new("Libraries", "/"),
                Crumb::new("Settings", "/settings"),
            ],
        ),
        people,
        works,
        publications,
    };
    Ok(Html(page.render()?))
}
