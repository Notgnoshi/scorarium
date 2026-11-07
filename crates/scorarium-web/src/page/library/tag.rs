use std::str::FromStr;

use askama::Template;
use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse, Response};
use scorarium_engine::Engine;
use scorarium_engine::entity::{AnyEntity, Entity, EntityKind};
use scorarium_engine::library::LibraryId;
use scorarium_engine::value::{Classification, Tag};
use serde::Deserialize;

use crate::auth::Session;
use crate::error::Result;
use crate::hit::Hit;
use crate::page::library::view;
use crate::page::{Base, Crumb};
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
    hits.sort_by_key(|hit| hit.title.to_lowercase());

    let parents = vec![
        Crumb::new("Libraries", "/"),
        Crumb::new(library.name(), format!("/library/{}", library.id())),
    ];
    let page = Tagged {
        base: Base::new(header, tag.to_string(), parents),
        nav: LibraryNav::load(&library, true, Active::Tags).await?,
        library: library.id(),
        tag,
        hits,
    };
    Ok(Html(page.render()?).into_response())
}
