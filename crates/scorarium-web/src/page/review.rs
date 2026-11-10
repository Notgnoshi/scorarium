use askama::Template;
use axum::extract::State;
use axum::response::Html;
use scorarium_engine::Engine;

use crate::auth::Session;
use crate::error::Result;
use crate::page::Base;
use crate::widget::breadcrumbs::Crumb;
use crate::widget::header::Header;
use crate::widget::review_queue::ReviewQueue;

#[derive(Template)]
#[template(path = "page/review.html")]
struct Review {
    base: Base,
    queue: ReviewQueue,
}

pub async fn get(
    _session: Session,
    State(engine): State<Engine>,
    header: Header,
) -> Result<Html<String>> {
    let page = Review {
        base: Base::new(header, "Review queue", vec![Crumb::new("Libraries", "/")]),
        queue: ReviewQueue::new(&engine).await?,
    };
    Ok(Html(page.render()?))
}
