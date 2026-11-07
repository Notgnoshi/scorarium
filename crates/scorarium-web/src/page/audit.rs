use askama::Template;
use axum::extract::State;
use axum::response::Html;
use scorarium_engine::Engine;

use crate::auth::Session;
use crate::error::Result;
use crate::page::Base;
use crate::widget::audit::AuditLog;
use crate::widget::breadcrumbs::Crumb;
use crate::widget::header::Header;

const PAGE_SIZE: usize = 100;

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
) -> Result<Html<String>> {
    let page = engine.audit_log(0, PAGE_SIZE).await?;
    let page = Audit {
        base: Base::new(
            header,
            "Audit log",
            vec![
                Crumb::new("Libraries", "/"),
                Crumb::new("Settings", "/settings"),
            ],
        ),
        log: AuditLog::new(page.events),
    };
    Ok(Html(page.render()?))
}
