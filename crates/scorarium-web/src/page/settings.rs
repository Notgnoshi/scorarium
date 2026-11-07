use askama::Template;
use axum::response::Html;

use crate::auth::Session;
use crate::error::Result;
use crate::page::Base;
use crate::widget::header::Header;

#[derive(Template)]
#[template(path = "page/settings.html")]
struct Settings {
    base: Base,
}

pub async fn get(_session: Session, header: Header) -> Result<Html<String>> {
    let page = Settings {
        base: Base::new(header, "Settings", vec![]),
    };
    Ok(Html(page.render()?))
}
