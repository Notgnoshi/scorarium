use askama::Template;
use axum::extract::{Path, State};
use axum::http::{Method, Uri};
use axum::response::{Html, IntoResponse, Response};
use scorarium_engine::Engine;
use scorarium_engine::entity::{Contributed, Entity};
use scorarium_engine::holding::{HoldingId, HoldingKind};
use scorarium_engine::library::LibraryId;
use scorarium_engine::publication::PublicationId;
use scorarium_engine::value::{Classification, Tag, Year};
use serde::Deserialize;

use crate::auth::{Viewer, login_redirect};
use crate::error::Result;
use crate::icon::Icon;
use crate::page::library::entity::{
    self, ContainedWork, ExternalLink, book_order, group_contributors,
};
use crate::page::library::view;
use crate::page::{Base, Crumb};
use crate::sort;
use crate::widget::header::Header;
use crate::widget::identifiers_input::kind_label;
use crate::widget::picture::Picture;
use crate::widget::stars::Stars;

#[derive(Template)]
#[template(path = "page/library/publication.html")]
struct Publication {
    base: Base,
    picture: Picture,
    logged_in: bool,
    library: LibraryId,
    id: PublicationId,
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
    works: Vec<ContainedWork>,
    numbered: bool,
    sort: Sort,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Column {
    /// The order the works appear in the book
    #[default]
    Contents,
    Title,
    Number,
    Contributors,
}

impl sort::Column for Column {
    const ALL: &'static [Self] = &[
        Column::Contents,
        Column::Title,
        Column::Number,
        Column::Contributors,
    ];

    fn name(self) -> &'static str {
        match self {
            Column::Contents => "contents",
            Column::Title => "title",
            Column::Number => "number",
            Column::Contributors => "contributors",
        }
    }
}

type Sort = sort::Sort<Column>;

#[derive(Deserialize)]
pub(crate) struct Query {
    sort: Option<Sort>,
}

/// An identifier with the label its kind is shown under
struct Identifier {
    label: Option<&'static str>,
    value: String,
}

struct Holding {
    id: HoldingId,
    icon: Icon,
    /// The shelf location of a physical copy
    location: Option<String>,
    /// The filename of a digital copy
    filename: Option<String>,
    loaned_to: Option<String>,
}

/// Order contents by the column, title breaking ties
fn sort_contents(works: &mut [ContainedWork], sort: &Sort) {
    if sort.column == Column::Contents {
        book_order(works);
        return;
    }
    works.sort_by(|a, b| {
        let (a_title, b_title) = (a.title.to_lowercase(), b.title.to_lowercase());
        let by_column = match sort.column {
            Column::Number => sort::compare(
                a.catalog_number.as_ref(),
                b.catalog_number.as_ref(),
                sort.descending,
            ),
            Column::Contributors => sort::compare(
                a.primary_sort_name(),
                b.primary_sort_name(),
                sort.descending,
            ),
            Column::Contents | Column::Title => {
                sort::compare(Some(&a_title), Some(&b_title), sort.descending)
            }
        };
        by_column.then_with(|| a_title.cmp(&b_title))
    });
}

impl From<&scorarium_engine::publication::Identifier> for Identifier {
    fn from(identifier: &scorarium_engine::publication::Identifier) -> Identifier {
        Identifier {
            label: kind_label(identifier.kind()),
            value: identifier.as_str().to_owned(),
        }
    }
}

impl From<&scorarium_engine::holding::Holding> for Holding {
    fn from(holding: &scorarium_engine::holding::Holding) -> Holding {
        let (icon, location, filename) = match &holding.kind {
            HoldingKind::Physical { location } => (Icon::PhysicalCopy, location.clone(), None),
            HoldingKind::Digital { file } => (
                Icon::DigitalFile,
                None,
                file.file_name()
                    .map(|name| name.to_string_lossy().into_owned()),
            ),
        };
        Holding {
            id: holding.id,
            icon,
            location,
            filename,
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
    axum::extract::Query(query): axum::extract::Query<Query>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    if library.is_private() && !viewer.is_logged_in() {
        return Ok(login_redirect(&method, &uri).into_response());
    }
    let publication: scorarium_engine::publication::Publication =
        entity::load(&engine, library.id(), &publication, &viewer).await?;

    let mut works: Vec<ContainedWork> = publication
        .works()
        .await?
        .iter()
        .filter(|work| viewer.is_logged_in() || !work.is_draft())
        .map(Into::into)
        .collect();
    let sort = query.sort.unwrap_or_default();
    sort_contents(&mut works, &sort);
    let numbered = works.iter().any(|work| work.catalog_number.is_some());

    let title = publication.title().unwrap_or("Untitled");
    let parents = vec![
        Crumb::new("Libraries", "/"),
        Crumb::new(library.name(), format!("/library/{}", library.id())),
    ];
    let page = Publication {
        picture: Picture::from(&publication),
        base: Base::new(header, title, parents),
        logged_in: viewer.is_logged_in(),
        library: library.id(),
        id: publication.id(),
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
        sort,
    };
    Ok(Html(page.render()?).into_response())
}
