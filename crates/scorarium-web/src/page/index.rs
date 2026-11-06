use askama::Template;
use axum::response::Html;

use crate::error::Result;
use crate::page::Base;

#[derive(Template)]
#[template(path = "page/index.html")]
struct Index {
    base: Base,
}

pub async fn get() -> Result<Html<String>> {
    let page = Index {
        base: Base {
            title: "Libraries".to_owned(),
        },
    };
    Ok(Html(page.render()?))
}
