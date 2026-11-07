use askama::Template;
use scorarium_engine::audit::{AuditEvent, AuditPage, AuditSubject};

use crate::widget::pagination::Pagination;

/// A table of audit log events
#[derive(Debug, Template)]
#[template(path = "widget/audit.html")]
pub struct AuditLog {
    events: Vec<AuditEvent>,
    pagination: Pagination,
}

impl AuditLog {
    /// Show one page of `log`, with links to the others under `path`
    pub fn new(path: &str, page: usize, page_size: usize, log: AuditPage) -> AuditLog {
        AuditLog {
            pagination: Pagination::new(path, page, page_size, log.total),
            events: log.events,
        }
    }
}

impl askama::filters::HtmlSafe for AuditLog {}
