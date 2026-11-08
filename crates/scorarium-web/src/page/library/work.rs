use askama::Template;
use axum::extract::{Path, State};
use axum::http::{Method, Uri};
use axum::response::{Html, IntoResponse, Response};
use scorarium_engine::Engine;
use scorarium_engine::entity::Entity;
use scorarium_engine::library::LibraryId;
use scorarium_engine::publication::PublicationId;
use scorarium_engine::value::{Classification, Tag, Year};
use scorarium_engine::work::{CatalogNumber, WorkId};

use crate::auth::{Viewer, login_redirect};
use crate::error::Result;
use crate::page::library::entity::{self, ExternalLink, group_contributors};
use crate::page::library::view;
use crate::page::{Base, Crumb};
use crate::widget::header::Header;
use crate::widget::stars::Stars;

#[derive(Template)]
#[template(path = "page/library/work.html")]
struct Work {
    base: Base,
    logged_in: bool,
    library: LibraryId,
    id: WorkId,
    draft: bool,
    title: String,
    catalog_numbers: Vec<CatalogNumber>,
    key: Option<String>,
    time_signature: Option<String>,
    instrumentation: Option<String>,
    classification: Option<Classification>,
    stars: Option<Stars>,
    contributors: Vec<entity::Contributor>,
    links: Vec<ExternalLink>,
    tags: Vec<Tag>,
    note: Option<String>,
    publications: Vec<Publication>,
}

/// A publication that contains the work
struct Publication {
    id: PublicationId,
    draft: bool,
    title: String,
    contributors: Vec<entity::Contributor>,
    year: Option<Year>,
}

impl From<&scorarium_engine::publication::Publication> for Publication {
    fn from(publication: &scorarium_engine::publication::Publication) -> Publication {
        Publication {
            id: publication.id(),
            draft: publication.is_draft(),
            title: publication.title().unwrap_or("Untitled").to_owned(),
            contributors: group_contributors(publication.contributors()),
            year: publication.year(),
        }
    }
}

pub async fn get(
    viewer: Viewer,
    method: Method,
    uri: Uri,
    State(engine): State<Engine>,
    header: Header,
    Path((library, work)): Path<(String, String)>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    if library.is_private() && !viewer.is_logged_in() {
        return Ok(login_redirect(&method, &uri).into_response());
    }
    let work: scorarium_engine::work::Work =
        entity::load(&engine, library.id(), &work, &viewer).await?;

    let mut publications: Vec<Publication> = work
        .publications()
        .await?
        .iter()
        .filter(|publication| viewer.is_logged_in() || !publication.is_draft())
        .map(Into::into)
        .collect();
    publications.sort_by_cached_key(|publication| publication.title.to_lowercase());

    let parents = vec![
        Crumb::new("Libraries", "/"),
        Crumb::new(library.name(), format!("/library/{}", library.id())),
    ];
    let page = Work {
        base: Base::new(header, work.title(), parents),
        logged_in: viewer.is_logged_in(),
        library: library.id(),
        id: work.id(),
        draft: work.is_draft(),
        title: work.title().to_owned(),
        catalog_numbers: work.catalog_numbers().to_vec(),
        key: work.key().map(ToOwned::to_owned),
        time_signature: work.time_signature().map(ToOwned::to_owned),
        instrumentation: work.instrumentation().map(ToOwned::to_owned),
        classification: work.classification(),
        stars: work.stars().map(Into::into),
        contributors: group_contributors(work.contributors()),
        links: work.links().iter().map(Into::into).collect(),
        tags: work.tags().to_vec(),
        note: work.note().map(ToOwned::to_owned),
        publications,
    };
    Ok(Html(page.render()?).into_response())
}
