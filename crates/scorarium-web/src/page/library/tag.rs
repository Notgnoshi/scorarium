use std::str::FromStr;

use askama::Template;
use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse, Response};
use scorarium_engine::Engine;
use scorarium_engine::entity::{AnyEntity, Entity};
use scorarium_engine::library::LibraryId;
use scorarium_engine::value::Tag;
use serde::Deserialize;

use crate::auth::Session;
use crate::error::Result;
use crate::hit::Hit;
use crate::page::library::view;
use crate::page::{Base, Crumb};
use crate::sort;
use crate::widget::header::Header;
use crate::widget::library_nav::{Active, LibraryNav};

#[derive(Template)]
#[template(path = "page/library/tag.html")]
struct Tagged {
    base: Base,
    nav: LibraryNav,
    library: LibraryId,
    tag: Tag,
    hits: Vec<Hit>,
    sort: Sort,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Column {
    #[default]
    Title,
    Kind,
}

impl sort::Column for Column {
    const ALL: &'static [Self] = &[Column::Title, Column::Kind];

    fn name(self) -> &'static str {
        match self {
            Column::Title => "title",
            Column::Kind => "kind",
        }
    }
}

type Sort = sort::Sort<Column>;

#[derive(Deserialize)]
pub(crate) struct Query {
    sort: Option<Sort>,
}

/// Sort hits by the column, title breaking ties
fn sort_hits(hits: &mut [Hit], sort: &Sort) {
    hits.sort_by(|a, b| {
        let (a_title, b_title) = (a.title.to_lowercase(), b.title.to_lowercase());
        let by_column = match sort.column {
            Column::Title => sort::compare(Some(&a_title), Some(&b_title), sort.descending),
            Column::Kind => {
                sort::compare(Some(a.kind_rank()), Some(b.kind_rank()), sort.descending)
            }
        };
        by_column.then_with(|| a_title.cmp(&b_title))
    });
}

#[derive(Deserialize)]
pub(crate) struct Params {
    id: String,
    tag: String,
}

pub async fn get(
    _session: Session,
    State(engine): State<Engine>,
    header: Header,
    Path(params): Path<Params>,
    axum::extract::Query(query): axum::extract::Query<Query>,
) -> Result<Response> {
    let library = view::load(&engine, &params.id).await?;
    let tag = Tag::from_str(&params.tag).map_err(|_| scorarium_engine::Error::NotFound)?;
    let (publications, works) = library.tagged_with(&tag).await?;
    let entities = publications
        .into_iter()
        .filter(|publication| !publication.is_draft())
        .map(AnyEntity::Publication)
        .chain(
            works
                .into_iter()
                .filter(|work| !work.is_draft())
                .map(AnyEntity::Work),
        );
    let mut hits: Vec<Hit> = entities
        .map(|entity| Hit::new(library.id(), library.name().to_owned(), &entity))
        .collect();
    let sort = query.sort.unwrap_or_default();
    sort_hits(&mut hits, &sort);

    let parents = vec![
        Crumb::new("Libraries", "/"),
        Crumb::new(library.name(), format!("/library/{}", library.id())),
        Crumb::new("Tags", format!("/library/{}/tags", library.id())),
    ];
    let page = Tagged {
        base: Base::new(header, tag.to_string(), parents),
        nav: LibraryNav::load(&library, true, Active::Tags).await?,
        library: library.id(),
        tag,
        hits,
        sort,
    };
    Ok(Html(page.render()?).into_response())
}
