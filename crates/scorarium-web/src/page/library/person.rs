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

use crate::auth::{Viewer, login_redirect};
use crate::error::Result;
use crate::page::library::entity::{self, ExternalLink, group_contributors};
use crate::page::library::view;
use crate::page::{Base, Crumb};
use crate::widget::header::Header;

#[derive(Template)]
#[template(path = "page/library/person.html")]
struct Person {
    base: Base,
    library: LibraryId,
    draft: bool,
    name: String,
    sort_name: String,
    links: Vec<ExternalLink>,
    publications: Vec<Publication>,
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

/// Nest each work under every publication that contains it, then sort
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
    publications.sort_by_cached_key(|publication| publication.title.to_lowercase());
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

pub async fn get(
    viewer: Viewer,
    method: Method,
    uri: Uri,
    State(engine): State<Engine>,
    header: Header,
    Path((library, person)): Path<(String, String)>,
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

    let parents = vec![
        Crumb::new("Libraries", "/"),
        Crumb::new(library.name(), format!("/library/{}", library.id())),
    ];
    let page = Person {
        base: Base::new(header, person.name(), parents),
        library: library.id(),
        draft: person.is_draft(),
        name: person.name().to_owned(),
        sort_name: person.sort_name().to_owned(),
        links: person.links().iter().map(Into::into).collect(),
        publications: nest_works(direct, works),
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
        assert_eq!(nested, [album, nocturnes]);
    }
}
