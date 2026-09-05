use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use askama::Template;
use axum::Form;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;

use super::{AppError, BaseContext, Crumb, Session};
use crate::db::pending_import::{self, NewPendingImport, PendingImport};
use crate::db::publication::HoldingKind;
use crate::import::Draft;
use crate::{AppState, db};

const UNTITLED: &str = "Untitled import";

pub struct PendingRow {
    pub import: PendingImport,
    /// The draft's title when one has been saved, else a placeholder
    pub title: String,
    pub age: String,
}

/// Rows for one library's list, or for the cross-library queue.
async fn pending_rows(state: &AppState, library_id: Option<i64>) -> sqlx::Result<Vec<PendingRow>> {
    Ok(pending_import::list(&state.pool, library_id)
        .await?
        .into_iter()
        .map(|import| PendingRow {
            title: state
                .drafts
                .get(import.id)
                .filter(|d| !d.title.is_empty())
                .map(|d| d.title)
                .unwrap_or_else(|| UNTITLED.to_string()),
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
    base: BaseContext,
    Path(id): Path<i64>,
    Query(query): Query<EntryQuery>,
) -> Result<Response, AppError> {
    render_entry(&state, id, base, query.more.is_some(), None).await
}

async fn render_entry(
    state: &AppState,
    id: i64,
    base: BaseContext,
    more: bool,
    error: Option<&'static str>,
) -> Result<Response, AppError> {
    let Some(library) = db::get_library(&state.pool, id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let page = EntryPage {
        base: base.page("Import", vec![Crumb::home(), Crumb::library(&library)]),
        pending: pending_rows(state, Some(id)).await?,
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
    base: BaseContext,
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
            &state,
            id,
            base,
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
    draft: Draft,
}

/// GET /library/{library_id}/import/{id}
pub async fn review(
    _session: Session,
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path((library_id, id)): Path<(i64, i64)>,
) -> Result<Response, AppError> {
    let Some(library) = db::get_library(&state.pool, library_id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let Some(import) = pending_import::get(&state.pool, library_id, id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let draft = state.drafts.get(id).unwrap_or_default();
    let title = if draft.title.is_empty() {
        UNTITLED
    } else {
        &draft.title
    };
    let page = ReviewPage {
        base: base.page(
            title,
            vec![
                Crumb::home(),
                Crumb::library(&library),
                Crumb::import(&library),
            ],
        ),
        age: age(import.created_at),
        library,
        import,
        draft,
    };
    Ok(Html(page.render()?).into_response())
}

#[derive(Deserialize)]
pub struct ReviewForm {
    title: String,
    publisher: String,
    year: String,
}

impl From<ReviewForm> for Draft {
    fn from(form: ReviewForm) -> Self {
        Draft {
            title: form.title.trim().to_string(),
            publisher: form.publisher.trim().to_string(),
            year: form.year.trim().to_string(),
        }
    }
}

/// POST /library/{library_id}/import/{id}/save
pub async fn save(
    _session: Session,
    State(state): State<Arc<AppState>>,
    Path((library_id, id)): Path<(i64, i64)>,
    Form(form): Form<ReviewForm>,
) -> Result<Response, AppError> {
    if pending_import::get(&state.pool, library_id, id)
        .await?
        .is_none()
    {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    state.drafts.save(id, form.into());
    Ok(Redirect::to(&format!("/library/{library_id}/import/{id}")).into_response())
}

#[derive(Template)]
#[template(path = "review.html")]
struct QueuePage {
    base: BaseContext,
    pending: Vec<PendingRow>,
    show_library: bool,
}

/// GET /review
pub async fn queue(
    _session: Session,
    State(state): State<Arc<AppState>>,
    base: BaseContext,
) -> Result<Response, AppError> {
    let page = QueuePage {
        base: base.page("Review queue", vec![Crumb::home()]),
        pending: pending_rows(&state, None).await?,
        show_library: true,
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
    state.drafts.remove(id);
    Ok(Redirect::to(&format!("/library/{library_id}/import")).into_response())
}
