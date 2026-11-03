use std::time::Duration;

use scorarium_engine::Engine;

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
