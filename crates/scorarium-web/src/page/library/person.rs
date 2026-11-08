use askama::Template;
use axum::extract::{Path, State};
use axum::http::{Method, Uri};
use axum::response::{Html, IntoResponse, Response};
use scorarium_engine::Engine;
use scorarium_engine::contributor::Role;
use scorarium_engine::entity::Entity;
use scorarium_engine::library::LibraryId;
use scorarium_engine::person::PersonId;
use scorarium_engine::publication::PublicationId;
use scorarium_engine::value::Year;
use scorarium_engine::work::{CatalogNumber, WorkId};
use serde::Deserialize;

use crate::auth::{Viewer, login_redirect};
use crate::error::Result;
use crate::page::library::entity::{self, ExternalLink, group_contributors};
use crate::page::library::view;
use crate::page::{Base, Crumb};
use crate::sort;
use crate::widget::header::Header;

#[derive(Template)]
#[template(path = "page/library/person.html")]
struct Person {
    base: Base,
    library: LibraryId,
    logged_in: bool,
    id: PersonId,
    draft: bool,
    name: String,
    sort_name: String,
    links: Vec<ExternalLink>,
    publications: Vec<Publication>,
    sort: Sort,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Column {
    #[default]
    Title,
    Role,
    Year,
}

impl sort::Column for Column {
    const ALL: &'static [Self] = &[Column::Title, Column::Role, Column::Year];

    fn name(self) -> &'static str {
        match self {
            Column::Title => "title",
            Column::Role => "role",
            Column::Year => "year",
        }
    }

    fn descending_first(self) -> bool {
        self == Column::Year
    }
}

type Sort = sort::Sort<Column>;

#[derive(Deserialize)]
pub(crate) struct Query {
    sort: Option<Sort>,
}

/// A publication the person contributed to, directly or through a contained work
#[derive(Debug, PartialEq, Eq)]
struct Publication {
    id: PublicationId,
    draft: bool,
    title: String,
    roles: Vec<Role>,
    year: Option<Year>,
    works: Vec<Work>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Work {
    id: WorkId,
    draft: bool,
    title: String,
    roles: Vec<Role>,
    catalog_number: Option<CatalogNumber>,
}

impl Publication {
    fn new(publication: &scorarium_engine::publication::Publication, roles: Vec<Role>) -> Self {
        Publication {
            id: publication.id(),
            draft: publication.is_draft(),
            title: publication.title().unwrap_or("Untitled").to_owned(),
            roles,
            year: publication.year(),
            works: Vec::new(),
        }
    }
}

impl Work {
    fn new(work: &scorarium_engine::work::Work, person: PersonId) -> Self {
        Work {
            id: work.id(),
            draft: work.is_draft(),
            title: work.title().to_owned(),
            roles: roles_of(person, work.contributors()),
            catalog_number: work.catalog_numbers().first().cloned(),
        }
    }
}

/// Get the roles `person` holds among `contributors`
fn roles_of(
    person: PersonId,
    contributors: &[scorarium_engine::contributor::Contributor],
) -> Vec<Role> {
    group_contributors(contributors)
        .into_iter()
        .find(|contributor| contributor.person == person)
        .map(|contributor| contributor.roles)
        .unwrap_or_default()
}

/// Nest each work under every publication that contains it, ordered by catalog number then title
fn nest_works(
    mut publications: Vec<Publication>,
    works: Vec<(Work, Vec<Publication>)>,
) -> Vec<Publication> {
    for (work, containing) in works {
        for publication in containing {
            let index = match publications.iter().position(|p| p.id == publication.id) {
                Some(index) => index,
                None => {
                    publications.push(publication);
                    publications.len() - 1
                }
            };
            publications[index].works.push(work.clone());
        }
    }
    for publication in &mut publications {
        publication.works.sort_by_cached_key(|work| {
            (
                work.catalog_number.is_none(),
                work.catalog_number.clone(),
                work.title.to_lowercase(),
            )
        });
    }
    publications
}

/// Sort publications by the column, breaking ties with the title
fn sort_publications(publications: &mut [Publication], sort: &Sort) {
    publications.sort_by(|a, b| {
        let (a_title, b_title) = (a.title.to_lowercase(), b.title.to_lowercase());
        let by_column = match sort.column {
            Column::Title => sort::compare(Some(&a_title), Some(&b_title), sort.descending),
            Column::Role => sort::compare(
                a.roles.first().map(|role| role.as_str().to_lowercase()),
                b.roles.first().map(|role| role.as_str().to_lowercase()),
                sort.descending,
            ),
            Column::Year => sort::compare(a.year, b.year, sort.descending),
        };
        by_column.then_with(|| a_title.cmp(&b_title))
    });
}

pub async fn get(
    viewer: Viewer,
    method: Method,
    uri: Uri,
    State(engine): State<Engine>,
    header: Header,
    Path((library, person)): Path<(String, String)>,
    axum::extract::Query(query): axum::extract::Query<Query>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    if library.is_private() && !viewer.is_logged_in() {
        return Ok(login_redirect(&method, &uri).into_response());
    }
    let person: scorarium_engine::person::Person =
        entity::load(&engine, library.id(), &person, &viewer).await?;
    let visible = |draft: bool| viewer.is_logged_in() || !draft;

    let direct: Vec<Publication> = person
        .publications()
        .await?
        .iter()
        .filter(|publication| visible(publication.is_draft()))
        .map(|publication| {
            Publication::new(
                publication,
                roles_of(person.id(), publication.contributors()),
            )
        })
        .collect();
    let mut works = Vec::new();
    for work in person.works().await? {
        if !visible(work.is_draft()) {
            continue;
        }
        let containing = work
            .publications()
            .await?
            .iter()
            .filter(|publication| visible(publication.is_draft()))
            .map(|publication| Publication::new(publication, Vec::new()))
            .collect();
        works.push((Work::new(&work, person.id()), containing));
    }

    let mut publications = nest_works(direct, works);
    let sort = query.sort.unwrap_or_default();
    sort_publications(&mut publications, &sort);

    let parents = vec![
        Crumb::new("Libraries", "/"),
        Crumb::new(library.name(), format!("/library/{}", library.id())),
    ];
    let page = Person {
        base: Base::new(header, person.name(), parents),
        library: library.id(),
        logged_in: viewer.is_logged_in(),
        id: person.id(),
        draft: person.is_draft(),
        name: person.name().to_owned(),
        sort_name: person.sort_name().to_owned(),
        links: person.links().iter().map(Into::into).collect(),
        publications,
        sort,
    };
    Ok(Html(page.render()?).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nest_works_lists_a_work_under_each_publication_and_keeps_direct_roles() {
        let role = |role: &str| role.parse::<Role>().unwrap();
        let publication = |id: &str, title: &str, roles: Vec<Role>| Publication {
            id: id.parse().unwrap(),
            draft: false,
            title: title.into(),
            roles,
            year: None,
            works: vec![],
        };
        let nocturne = Work {
            id: "1".parse().unwrap(),
            draft: false,
            title: "Nocturne".into(),
            roles: vec![role("composer")],
            catalog_number: Some("Op. 9/2".parse().unwrap()),
        };
        let nested = nest_works(
            vec![publication("1", "Nocturnes", vec![role("editor")])],
            vec![(
                nocturne.clone(),
                vec![
                    publication("2", "Album", vec![]),
                    publication("1", "Nocturnes", vec![]),
                ],
            )],
        );
        let mut nocturnes = publication("1", "Nocturnes", vec![role("editor")]);
        nocturnes.works.push(nocturne.clone());
        let mut album = publication("2", "Album", vec![]);
        album.works.push(nocturne);
        assert_eq!(nested, [nocturnes, album]);
    }
}
