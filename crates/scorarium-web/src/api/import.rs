use askama::Template;
use axum::extract::State;
use axum::response::Html;
use scorarium_engine::Engine;

use crate::api::{Query, SUGGESTION_LIMIT};
use crate::auth::ApiSession;
use crate::error::Result;
use crate::widget::candidate::Candidate;

#[derive(Template)]
#[template(path = "api/candidates.html")]
struct Fragment {
    candidates: Vec<Candidate>,
}

/// Suggest external publications whose titles match the typed text
pub async fn candidates(
    _session: ApiSession,
    State(engine): State<Engine>,
    axum::extract::Query(query): axum::extract::Query<Query>,
) -> Result<Html<String>> {
    let text = query.q.as_deref().unwrap_or_default();
    let candidates = match engine.suggest_external(text, SUGGESTION_LIMIT).await {
        Ok(candidates) => candidates,
        Err(e) => {
            tracing::warn!("Failed to suggest external candidates for {text:?}: {e}");
            Vec::new()
        }
    };
    let candidates = candidates.iter().filter_map(Candidate::new).collect();
    Ok(Html(Fragment { candidates }.render()?))
}
