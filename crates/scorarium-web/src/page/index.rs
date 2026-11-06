use askama::Template;
use axum::response::Html;

use crate::error::Result;
use crate::page::Base;
use crate::widget::header::Header;

#[derive(Template)]
#[template(path = "page/index.html")]
struct Index {
    base: Base,
}

pub async fn get(header: Header) -> Result<Html<String>> {
    let page = Index {
        base: Base::new(header, "Libraries"),
    };
    Ok(Html(page.render()?))
}
