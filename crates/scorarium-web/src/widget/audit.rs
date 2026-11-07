use askama::Template;
use scorarium_engine::audit::{AuditEvent, AuditSubject};

/// A table of audit log events
#[derive(Debug, Template)]
#[template(path = "widget/audit.html")]
pub struct AuditLog {
    events: Vec<AuditEvent>,
}

impl AuditLog {
    pub fn new(events: Vec<AuditEvent>) -> AuditLog {
        AuditLog { events }
    }
}

impl askama::filters::HtmlSafe for AuditLog {}
