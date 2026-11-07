use askama::Template;
use axum::extract::State;
use axum::response::Html;
use scorarium_engine::Engine;
use serde::Deserialize;

use crate::auth::Session;
use crate::error::Result;
use crate::page::Base;
use crate::widget::audit::AuditLog;
use crate::widget::breadcrumbs::Crumb;
use crate::widget::header::Header;

const PAGE_SIZE: usize = 100;

#[derive(Deserialize)]
pub(crate) struct Query {
    page: Option<usize>,
}

#[derive(Template)]
#[template(path = "page/audit.html")]
struct Audit {
    base: Base,
    log: AuditLog,
}

pub async fn get(
    _session: Session,
    State(engine): State<Engine>,
    header: Header,
    axum::extract::Query(query): axum::extract::Query<Query>,
) -> Result<Html<String>> {
    let page = query.page.unwrap_or(1).max(1);
    // The engine clamps an offset past the end.
    let offset = (page - 1).saturating_mul(PAGE_SIZE);
    let log = engine.audit_log(offset, PAGE_SIZE).await?;
    let page = Audit {
        base: Base::new(
            header,
            "Audit log",
            vec![
                Crumb::new("Libraries", "/"),
                Crumb::new("Settings", "/settings"),
            ],
        ),
        log: AuditLog::new("/settings/audit", page, PAGE_SIZE, log),
    };
    Ok(Html(page.render()?))
}
