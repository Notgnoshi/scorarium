use std::io::Write;
use std::time::Duration;

use eyre::bail;
use scorarium_engine::Engine;
use scorarium_engine::enrich::{EnrichmentState, EnrichmentStatus};
use scorarium_engine::entity::Entity;

/// Wait for background enrichment to finish, giving up after the timeout if one is given.
pub(crate) async fn wait(engine: &Engine, timeout: Option<Duration>) {
    let Some(timeout) = timeout else {
        engine.wait_for_enrichment().await;
        return;
    };
    if tokio::time::timeout(timeout, engine.wait_for_enrichment())
        .await
        .is_err()
    {
        tracing::warn!("Background enrichment is still in progress and will be abandoned");
    }
}

/// Format an enrichment status as a human readable string
pub(crate) fn status(status: EnrichmentStatus) -> String {
    let state = match status.state() {
        EnrichmentState::InProgress => "in progress",
        EnrichmentState::Failed => "failed",
        EnrichmentState::Completed => "completed",
        EnrichmentState::Cancelled => "cancelled",
    };
    let done = status.successful + status.failed;
    let total = done + status.pending;
    let mut description = format!("{state} {done}/{total}");
    if status.failed > 0 {
        description.push_str(&format!(", {} failed", status.failed));
    }
    description
}

/// Start enrichment, or print the status of a run already in progress.
pub(crate) async fn enrich<E: Entity>(entity: &E, out: &mut impl Write) -> eyre::Result<()> {
    if let Some(enrichment) = entity.enrichment() {
        let status = enrichment.status();
        if status.state() == EnrichmentState::InProgress {
            writeln!(out, "{}", self::status(status))?;
            return Ok(());
        }
    }
    entity.enrich().await?;
    Ok(())
}

/// Cancel the entity's enrichment task whatever its state, failing when it has none.
pub(crate) fn cancel_enrichment<E: Entity>(
    entity: &E,
    kind: &str,
    reference: &str,
) -> eyre::Result<()> {
    let Some(enrichment) = entity.enrichment() else {
        bail!("{kind} '{reference}' has no enrichment task");
    };
    enrichment.cancel();
    Ok(())
}
