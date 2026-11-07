use askama::Template;
use axum::extract::State;
use axum::response::Html;
use scorarium_engine::Engine;
use scorarium_engine::library::LibraryId;

use crate::auth::Viewer;
use crate::error::Result;
use crate::page::Base;
use crate::widget::header::Header;

#[derive(Template)]
#[template(path = "page/index.html")]
struct Index {
    base: Base,
    libraries: Vec<Summary>,
}

struct Summary {
    id: LibraryId,
    name: String,
    private: bool,
    publications: usize,
}

pub async fn get(
    viewer: Viewer,
    State(engine): State<Engine>,
    header: Header,
) -> Result<Html<String>> {
    let mut libraries = Vec::new();
    for library in engine.libraries().await? {
        if library.is_private() && !viewer.is_logged_in() {
            continue;
        }
        libraries.push(Summary {
            id: library.id(),
            name: library.name().to_owned(),
            private: library.is_private(),
            publications: library.publication_count(false).await?,
        });
    }
    let page = Index {
        base: Base::new(header, "Libraries", vec![]),
        libraries,
    };
    Ok(Html(page.render()?))
}
