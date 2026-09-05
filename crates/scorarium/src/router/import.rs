use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use askama::Template;
use axum::Form;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;
use sqlx::SqlitePool;

use super::{AppError, BaseContext, Crumb, Session};
use crate::db::pending_import::{self, NewPendingImport, PendingImport};
use crate::db::publication::HoldingKind;
use crate::{AppState, db};

pub struct PendingRow {
    pub import: PendingImport,
    pub age: String,
}

/// Rows for one library's list, or for the cross-library queue.
async fn pending_rows(pool: &SqlitePool, library_id: Option<i64>) -> sqlx::Result<Vec<PendingRow>> {
    Ok(pending_import::list(pool, library_id)
        .await?
        .into_iter()
        .map(|import| PendingRow {
            age: age(import.created_at),
            import,
        })
        .collect())
}

fn age(created_at: i64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(created_at);
    let seconds = (now - created_at).max(0);
    let (count, unit) = match seconds {
        s if s < 60 => return "just now".to_string(),
        s if s < 3600 => (s / 60, "minute"),
        s if s < 86400 => (s / 3600, "hour"),
        s => (s / 86400, "day"),
    };
    let plural = if count == 1 { "" } else { "s" };
    format!("{count} {unit}{plural} ago")
}

#[derive(Template)]
#[template(path = "import.html")]
struct EntryPage {
    base: BaseContext,
    library: db::Library,
    more: bool,
    error: Option<&'static str>,
    pending: Vec<PendingRow>,
    /// The shared list fragment shows a library column only on the cross-library queue.
    show_library: bool,
}

#[derive(Deserialize)]
pub struct EntryQuery {
    more: Option<String>,
}

/// GET /library/{id}/import
pub async fn entry(
    _session: Session,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Query(query): Query<EntryQuery>,
) -> Result<Response, AppError> {
    render_entry(&state.pool, id, query.more.is_some(), None).await
}

async fn render_entry(
    pool: &SqlitePool,
    id: i64,
    more: bool,
    error: Option<&'static str>,
) -> Result<Response, AppError> {
    let Some(library) = db::get_library(pool, id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let page = EntryPage {
        base: BaseContext::new(
            "Import",
            format!("/library/{id}/import"),
            true,
            vec![Crumb::home(), Crumb::library(&library)],
        ),
        pending: pending_rows(pool, Some(id)).await?,
        show_library: false,
        library,
        more,
        error,
    };
    Ok(Html(page.render()?).into_response())
}

#[derive(Deserialize)]
pub struct StartForm {
    kind: String,
    location: Option<String>,
    file: Option<String>,
    /// Present when the "Import more" box is checked; browsers send "on".
    more: Option<String>,
}

/// POST /library/{id}/import
pub async fn start(
    _session: Session,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Form(form): Form<StartForm>,
) -> Result<Response, AppError> {
    let more = form.more.is_some();
    let Ok(kind) = form.kind.parse::<HoldingKind>() else {
        return Ok(StatusCode::BAD_REQUEST.into_response());
    };
    let location = match kind {
        HoldingKind::Physical => form.location,
        HoldingKind::Digital => form.file,
    }
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty());
    if kind == HoldingKind::Digital && location.is_none() {
        return render_entry(
            &state.pool,
            id,
            more,
            Some("Choose a file for a digital copy."),
        )
        .await;
    }
    if db::get_library(&state.pool, id).await?.is_none() {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    let pending_id = pending_import::create(
        &state.pool,
        &NewPendingImport {
            library_id: id,
            query: "",
            kind,
            location: location.as_deref(),
        },
    )
    .await?;
    let next = if more {
        format!("/library/{id}/import?more=1")
    } else {
        format!("/library/{id}/import/{pending_id}")
    };
    Ok(Redirect::to(&next).into_response())
}

#[derive(Template)]
#[template(path = "import_review.html")]
struct ReviewPage {
    base: BaseContext,
    library: db::Library,
    import: PendingImport,
    age: String,
}

/// GET /library/{library_id}/import/{id}
pub async fn review(
    _session: Session,
    State(state): State<Arc<AppState>>,
    Path((library_id, id)): Path<(i64, i64)>,
) -> Result<Response, AppError> {
    let Some(library) = db::get_library(&state.pool, library_id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let Some(import) = pending_import::get(&state.pool, library_id, id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let page = ReviewPage {
        base: BaseContext::new(
            "Untitled import",
            format!("/library/{library_id}/import/{id}"),
            true,
            vec![
                Crumb::home(),
                Crumb::library(&library),
                Crumb::import(&library),
            ],
        ),
        age: age(import.created_at),
        library,
        import,
    };
    Ok(Html(page.render()?).into_response())
}

/// POST /library/{library_id}/import/{id}/delete
pub async fn delete(
    _session: Session,
    State(state): State<Arc<AppState>>,
    Path((library_id, id)): Path<(i64, i64)>,
) -> Result<Response, AppError> {
    if !pending_import::delete(&state.pool, library_id, id).await? {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    Ok(Redirect::to(&format!("/library/{library_id}/import")).into_response())
}
