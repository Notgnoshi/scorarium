use askama::Template;
use axum::extract::{Path, State};
use axum::http::{Method, Uri};
use axum::response::{Html, IntoResponse, Response};
use scorarium_engine::Engine;
use scorarium_engine::entity::Entity;
use scorarium_engine::holding::HoldingKind;
use scorarium_engine::library::LibraryId;
use scorarium_engine::publication::IdentifierKind;
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
#[template(path = "page/library/publication.html")]
struct Publication {
    base: Base,
    logged_in: bool,
    library: LibraryId,
    draft: bool,
    title: String,
    publisher: Option<String>,
    year: Option<Year>,
    classification: Option<Classification>,
    stars: Option<Stars>,
    contributors: Vec<entity::Contributor>,
    identifiers: Vec<Identifier>,
    links: Vec<ExternalLink>,
    tags: Vec<Tag>,
    note: Option<String>,
    holdings: Vec<Holding>,
    works: Vec<Work>,
    numbered: bool,
}

/// An identifier with the label its kind is shown under
struct Identifier {
    label: Option<&'static str>,
    value: String,
}

struct Holding {
    physical: bool,
    /// The shelf location of a physical copy, or the filename of a digital one
    location: Option<String>,
    loaned_to: Option<String>,
}

struct Work {
    id: WorkId,
    draft: bool,
    title: String,
    /// The work's first catalog number, which is its most confidently recognized one
    catalog_number: Option<CatalogNumber>,
    contributors: Vec<entity::Contributor>,
}

impl From<&scorarium_engine::work::Work> for Work {
    fn from(work: &scorarium_engine::work::Work) -> Work {
        Work {
            id: work.id(),
            draft: work.is_draft(),
            title: work.title().to_owned(),
            catalog_number: work.catalog_numbers().first().cloned(),
            contributors: group_contributors(work.contributors()),
        }
    }
}

/// Order contents by first contributor, then catalog number, then title
fn sort_contents(works: &mut [Work]) {
    works.sort_by_cached_key(|work| {
        let sort_name = work
            .contributors
            .first()
            .map(|c| c.sort_name.to_lowercase());
        (
            sort_name.is_none(),
            sort_name,
            work.catalog_number.is_none(),
            work.catalog_number.clone(),
            work.title.to_lowercase(),
        )
    });
}

impl From<&scorarium_engine::publication::Identifier> for Identifier {
    fn from(identifier: &scorarium_engine::publication::Identifier) -> Identifier {
        let label = match identifier.kind() {
            IdentifierKind::Isbn => Some("ISBN"),
            IdentifierKind::Ismn => Some("ISMN"),
            IdentifierKind::PublisherNumber => None,
            IdentifierKind::PlateNumber => Some("Plate No."),
        };
        Identifier {
            label,
            value: identifier.as_str().to_owned(),
        }
    }
}

impl From<&scorarium_engine::holding::Holding> for Holding {
    fn from(holding: &scorarium_engine::holding::Holding) -> Holding {
        let (physical, location) = match &holding.kind {
            HoldingKind::Physical { location } => (true, location.clone()),
            HoldingKind::Digital { file } => (
                false,
                file.file_name()
                    .map(|name| name.to_string_lossy().into_owned()),
            ),
        };
        Holding {
            physical,
            location,
            loaned_to: holding.loaned_to.clone(),
        }
    }
}

pub async fn get(
    viewer: Viewer,
    method: Method,
    uri: Uri,
    State(engine): State<Engine>,
    header: Header,
    Path((library, publication)): Path<(String, String)>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    if library.is_private() && !viewer.is_logged_in() {
        return Ok(login_redirect(&method, &uri).into_response());
    }
    let publication: scorarium_engine::publication::Publication =
        entity::load(&engine, library.id(), &publication, &viewer).await?;

    let mut works: Vec<Work> = publication
        .works()
        .await?
        .iter()
        .filter(|work| viewer.is_logged_in() || !work.is_draft())
        .map(Into::into)
        .collect();
    sort_contents(&mut works);
    let numbered = works.iter().any(|work| work.catalog_number.is_some());

    let title = publication.title().unwrap_or("Untitled");
    let parents = vec![
        Crumb::new("Libraries", "/"),
        Crumb::new(library.name(), format!("/library/{}", library.id())),
    ];
    let page = Publication {
        base: Base::new(header, title, parents),
        logged_in: viewer.is_logged_in(),
        library: library.id(),
        draft: publication.is_draft(),
        title: title.to_owned(),
        publisher: publication.publisher().map(ToOwned::to_owned),
        year: publication.year(),
        classification: publication.classification(),
        stars: publication.stars().map(Into::into),
        contributors: group_contributors(publication.contributors()),
        identifiers: publication.identifiers().iter().map(Into::into).collect(),
        links: publication.links().iter().map(Into::into).collect(),
        tags: publication.tags().to_vec(),
        note: publication.note().map(ToOwned::to_owned),
        holdings: publication.holdings().iter().map(Into::into).collect(),
        works,
        numbered,
    };
    Ok(Html(page.render()?).into_response())
}
