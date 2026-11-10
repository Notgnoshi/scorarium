use askama::Template;
use axum::extract::{Query, State};
use axum::response::Html;
use scorarium_engine::Engine;

use crate::api::review::ReviewQuery;
use crate::auth::Session;
use crate::error::Result;
use crate::page::Base;
use crate::widget::breadcrumbs::Crumb;
use crate::widget::header::Header;
use crate::widget::review_queue::{ReviewQueue, Size};

#[derive(Template)]
#[template(path = "page/review.html")]
struct Review {
    base: Base,
    queue: ReviewQueue,
}

pub async fn get(
    _session: Session,
    State(engine): State<Engine>,
    Query(query): Query<ReviewQuery>,
    header: Header,
) -> Result<Html<String>> {
    let page = Review {
        base: Base::new(header, "Review queue", vec![Crumb::new("Libraries", "/")]),
        queue: ReviewQueue::new(
            &engine,
            Size::Full {
                sort: query.sort.unwrap_or_default(),
            },
            None,
        )
        .await?,
    };
    Ok(Html(page.render()?))
}
