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
use serde::Deserialize;

use crate::auth::{Viewer, login_redirect};
use crate::error::Result;
use crate::page::library::entity::{self, ExternalLink, group_contributors, is_primary};
use crate::page::library::view;
use crate::page::{Base, Crumb};
use crate::sort;
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
    sort: Sort,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Column {
    #[default]
    Title,
    Contributors,
    Year,
}

impl sort::Column for Column {
    const ALL: &'static [Self] = &[Column::Title, Column::Contributors, Column::Year];

    fn name(self) -> &'static str {
        match self {
            Column::Title => "title",
            Column::Contributors => "contributors",
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

/// A publication that contains the work
struct Publication {
    id: PublicationId,
    draft: bool,
    title: String,
    contributors: Vec<entity::Contributor>,
    year: Option<Year>,
}

impl Publication {
    /// Get the lowercase sort name of the first composer or author
    fn primary_sort_name(&self) -> Option<String> {
        self.contributors
            .iter()
            .find(|c| c.roles.iter().any(is_primary))
            .map(|c| c.sort_name.to_lowercase())
    }
}

/// Sort publications by the column, title breaking ties
fn sort_publications(publications: &mut [Publication], sort: &Sort) {
    publications.sort_by(|a, b| {
        let (a_title, b_title) = (a.title.to_lowercase(), b.title.to_lowercase());
        let by_column = match sort.column {
            Column::Title => sort::compare(Some(&a_title), Some(&b_title), sort.descending),
            Column::Contributors => sort::compare(
                a.primary_sort_name(),
                b.primary_sort_name(),
                sort.descending,
            ),
            Column::Year => sort::compare(a.year, b.year, sort.descending),
        };
        by_column.then_with(|| a_title.cmp(&b_title))
    });
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
    axum::extract::Query(query): axum::extract::Query<Query>,
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
    let sort = query.sort.unwrap_or_default();
    sort_publications(&mut publications, &sort);

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
        sort,
    };
    Ok(Html(page.render()?).into_response())
}
