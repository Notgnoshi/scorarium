use askama::Template;
use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse, Response};
use scorarium_engine::Engine;
use scorarium_engine::library::LibraryId;
use scorarium_engine::value::Tag;

use crate::auth::Session;
use crate::error::Result;
use crate::page::library::view;
use crate::page::{Base, Crumb};
use crate::widget::header::Header;
use crate::widget::library_nav::{Active, LibraryNav};

#[derive(Template)]
#[template(path = "page/library/tags.html")]
struct Tags {
    base: Base,
    nav: LibraryNav,
    library: LibraryId,
    tags: Vec<(Tag, usize)>,
}

pub async fn get(
    _session: Session,
    State(engine): State<Engine>,
    header: Header,
    Path(id): Path<String>,
) -> Result<Response> {
    let library = view::load(&engine, &id).await?;
    let mut tags = library.tags(false).await?;
    tags.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let parents = vec![
        Crumb::new("Libraries", "/"),
        Crumb::new(library.name(), format!("/library/{}", library.id())),
    ];
    let page = Tags {
        base: Base::new(header, "Tags", parents),
        nav: LibraryNav::load(&library, true, Active::Tags).await?,
        library: library.id(),
        tags,
    };
    Ok(Html(page.render()?).into_response())
}
