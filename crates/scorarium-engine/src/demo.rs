use crate::{Engine, Result};

/// Populate a demo database intended to exercise several different aspects of Scorarium's data model
pub(crate) async fn populate(engine: &Engine) -> Result<()> {
    engine.create_library("Books", true).await?;
    engine.create_library("Sheet music", false).await?;
    Ok(())
}
