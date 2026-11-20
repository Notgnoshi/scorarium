use askama::Template;
use axum::extract::State;
use axum::response::Html;
use scorarium_engine::Engine;
use scorarium_engine::library::LibraryId;

use crate::auth::Session;
use crate::error::Result;
use crate::page::Base;
use crate::widget::breadcrumbs::Crumb;
use crate::widget::header::Header;

#[derive(Template)]
#[template(path = "page/export.html")]
struct Export {
    base: Base,
    libraries: Vec<Exportable>,
}

struct Exportable {
    id: LibraryId,
    name: String,
    /// How many committed publications the script recreates
    publications: usize,
    /// How many draft publications the script leaves out
    drafts: usize,
}

pub async fn get(
    _session: Session,
    State(engine): State<Engine>,
    header: Header,
) -> Result<Html<String>> {
    let mut libraries = Vec::new();
    for library in engine.libraries().await? {
        let publications = library.publication_count(false).await?;
        let all = library.publication_count(true).await?;
        libraries.push(Exportable {
            id: library.id(),
            name: library.name().to_owned(),
            publications,
            drafts: all - publications,
        });
    }
    let page = Export {
        base: Base::new(
            header,
            "Export",
            vec![
                Crumb::new("Libraries", "/"),
                Crumb::new("Settings", "/settings"),
            ],
        ),
        libraries,
    };
    Ok(Html(page.render()?))
}
