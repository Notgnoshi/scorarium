use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum_extra::extract::Form as MultiForm;
use serde::Deserialize;

use super::{AppError, BaseContext, Crumb, FormFields, Session, pair_errors};
use crate::db::pending_import::{self, NewPendingImport, PendingHolding, PendingImport};
use crate::db::publication::HoldingKind;
use crate::publication_form::{Errors, HoldingRow, PublicationForm, Submission};
use crate::{AppState, db, import, publication_form};

const UNTITLED: &str = "Untitled import";

pub struct PendingRow {
    pub import: PendingImport,
    pub title: String,
    pub holdings: Vec<HoldingRow>,
    pub age: String,
}

/// Rows for one library's list, or for the cross-library queue.
async fn pending_rows(state: &AppState, library_id: Option<i64>) -> sqlx::Result<Vec<PendingRow>> {
    Ok(pending_import::list(&state.pool, library_id)
        .await?
        .into_iter()
        .map(|import| {
            let draft = state
                .drafts
                .get(import.id)
                .unwrap_or_else(|| PublicationForm::seed(&import));
            PendingRow {
                title: label(&import, &draft),
                holdings: draft.holdings,
                age: age(import.created_at),
                import,
            }
        })
        .collect())
}

/// What to call a pending import: its draft's title, else what was typed, else a placeholder.
fn label(import: &PendingImport, draft: &PublicationForm) -> String {
    [draft.title.as_str(), import.query.as_str(), UNTITLED]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or(UNTITLED)
        .to_string()
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
    /// What to show in the form: blank on a visit, the rejected submission on an error
    query: String,
    more: bool,
    holding_rows: Vec<(HoldingRow, String)>,
    errors: Errors,
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
    let rows = vec![HoldingRow {
        id: None,
        kind: HoldingKind::Physical,
        location: String::new(),
    }];
    render_entry(
        &state,
        id,
        base,
        String::new(),
        query.more.is_some(),
        rows,
        Errors::default(),
    )
    .await
}

async fn render_entry(
    state: &AppState,
    id: i64,
    base: BaseContext,
    query: String,
    more: bool,
    rows: Vec<HoldingRow>,
    errors: Errors,
) -> Result<Response, AppError> {
    let Some(library) = db::get_library(&state.pool, id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let holding_rows = pair_errors(&rows, &errors.holdings);
    let page = EntryPage {
        base: base.page("Import", vec![Crumb::home(), Crumb::library(&library)]),
        pending: pending_rows(state, Some(id)).await?,
        show_library: false,
        library,
        query,
        more,
        holding_rows,
        errors,
    };
    Ok(Html(page.render()?).into_response())
}

#[derive(Deserialize)]
pub struct StartForm {
    #[serde(default)]
    query: String,
    // Copy rows as parallel repeated keys, decoded as on the review page
    #[serde(default)]
    holding_id: Vec<String>,
    #[serde(default)]
    holding_kind: Vec<HoldingKind>,
    #[serde(default)]
    holding_location: Vec<String>,
    #[serde(default)]
    holding_file: Vec<String>,
    /// Present when the "Import more" box is checked; browsers send "on".
    more: Option<String>,
}

/// POST /library/{id}/import
pub async fn start(
    _session: Session,
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path(id): Path<i64>,
    MultiForm(form): MultiForm<StartForm>,
) -> Result<Response, AppError> {
    let rows = publication_form::holding_rows(
        form.holding_id,
        form.holding_kind,
        form.holding_location,
        form.holding_file,
    );
    let mut errors = Errors::default();
    let holdings = publication_form::parse_holdings(&rows, &mut errors);
    let more = form.more.is_some();
    if !errors.is_empty() {
        return render_entry(&state, id, base, form.query, more, rows, errors).await;
    }
    if db::get_library(&state.pool, id).await?.is_none() {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    let holdings: Vec<PendingHolding> = holdings
        .into_iter()
        .map(|h| PendingHolding {
            kind: h.kind,
            location: h.location,
        })
        .collect();
    let pending_id = pending_import::create(
        &state.pool,
        &NewPendingImport {
            library_id: id,
            query: form.query.trim(),
            holdings: &holdings,
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
    fields: FormFields,
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
    let (draft, errors) = match state.drafts.get(id) {
        // Errors show for saved drafts only; a fresh import should not open covered in warnings
        Some(draft) => {
            let errors = draft.parse().err().unwrap_or_default();
            (draft, errors)
        }
        None => (PublicationForm::seed(&import), Errors::default()),
    };
    let title = label(&import, &draft);
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
        // Nothing is stored until the import is accepted, so no work row can name one
        fields: FormFields::build(&state.pool, library_id, draft, errors, &[]).await?,
        library,
        import,
    };
    Ok(Html(page.render()?).into_response())
}

/// POST /library/{library_id}/import/{id}/save
pub async fn save(
    _session: Session,
    State(state): State<Arc<AppState>>,
    Path((library_id, id)): Path<(i64, i64)>,
    MultiForm(submission): MultiForm<Submission>,
) -> Result<Response, AppError> {
    if pending_import::get(&state.pool, library_id, id)
        .await?
        .is_none()
    {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    state.drafts.save(id, submission.into());
    Ok(Redirect::to(&format!("/library/{library_id}/import/{id}")).into_response())
}

/// POST /library/{library_id}/import/{id}/submit
pub async fn submit(
    _session: Session,
    State(state): State<Arc<AppState>>,
    Path((library_id, id)): Path<(i64, i64)>,
    MultiForm(submission): MultiForm<Submission>,
) -> Result<Response, AppError> {
    let Some(pending) = pending_import::get(&state.pool, library_id, id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let draft = PublicationForm::from(submission);
    let validated = match draft.parse() {
        Ok(validated) => validated,
        // Keep the edits so the review page can show what is wrong with them
        Err(_) => {
            state.drafts.save(id, draft);
            return Ok(Redirect::to(&format!("/library/{library_id}/import/{id}")).into_response());
        }
    };
    let publication_id = import::accept(&state.pool, &pending, &validated).await?;
    // Accepted here or already gone from another tab: either way the draft is finished with
    state.drafts.remove(id);
    match publication_id {
        Some(publication_id) => Ok(Redirect::to(&format!(
            "/library/{library_id}/publication/{publication_id}"
        ))
        .into_response()),
        None => Ok(StatusCode::NOT_FOUND.into_response()),
    }
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
