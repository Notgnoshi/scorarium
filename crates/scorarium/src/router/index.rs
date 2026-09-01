use std::sync::Arc;

use axum::extract::State;

use super::AppError;
use crate::{AppState, db};

pub async fn index(State(state): State<Arc<AppState>>) -> Result<String, AppError> {
    let libraries = db::list_libraries(&state.pool).await?;
    Ok(libraries.into_iter().fold(String::new(), |mut body, lib| {
        body.push_str(&lib.name);
        body.push('\n');
        body
    }))
}
