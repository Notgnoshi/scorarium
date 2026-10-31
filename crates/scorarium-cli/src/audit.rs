use std::io::Write;

use scorarium_engine::Engine;
use scorarium_engine::audit::{AuditEvent, AuditSubject};
use scorarium_engine::entity::Entity;

use crate::library::write_table;

/// Print the full global audit log of every library
pub(crate) async fn log(engine: &Engine, limit: usize, out: &mut impl Write) -> eyre::Result<()> {
    let rows: Vec<[String; 7]> = engine
        .audit_log(None, limit)
        .await?
        .iter()
        .map(|event| {
            let (kind, id, label) = match &event.subject {
                None => (String::new(), String::new(), String::new()),
                Some(AuditSubject::Library { id, label }) => {
                    ("library".to_string(), id.to_string(), label.clone())
                }
                Some(AuditSubject::Entity {
                    kind, id, label, ..
                }) => (kind.to_string(), id.to_string(), label.clone()),
            };
            [
                timestamp(event),
                event.source.to_string(),
                event.action.to_string(),
                kind,
                id,
                label,
                event.field.clone().unwrap_or_default(),
            ]
        })
        .collect();
    write_table(out, &rows)
}

/// Print a specific entity's audit log
pub(crate) async fn entity_log<E: Entity>(
    entity: &E,
    limit: usize,
    out: &mut impl Write,
) -> eyre::Result<()> {
    let rows: Vec<[String; 4]> = entity
        .audit_log(None, limit)
        .await?
        .iter()
        .map(|event| {
            [
                timestamp(event),
                event.source.to_string(),
                event.action.to_string(),
                event.field.clone().unwrap_or_default(),
            ]
        })
        .collect();
    write_table(out, &rows)
}

fn timestamp(event: &AuditEvent) -> String {
    event.created_at.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}
