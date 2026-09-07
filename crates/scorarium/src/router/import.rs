use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum_extra::extract::Form as MultiForm;
use scorarium_archive::{
    ContributorInput, HoldingKind as ArchiveHoldingKind, HoldingRawInput, IdentifierRawInput,
    Library, PublicationErrors, PublicationRawInput, WorkRawInput,
};
use serde::Deserialize;

use super::{
    AppError, BaseContext, Crumb, FormFields, OrNotFound, Session, ShownHolding, WorkEdit,
    WorkFields,
};
use crate::db::pending_import::{self, NewPendingImport, PendingHolding, PendingImport};
use crate::db::publication::HoldingKind;
use crate::publication_form::{Errors, HoldingRow, PublicationForm, Submission};
use crate::{AppState, import, publication_form, work_form};

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
                .unwrap_or_else(|| import::Draft::seed(&import));
            PendingRow {
                title: label(&import, &draft.form),
                holdings: draft.form.holdings,
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
    library: Library,
    /// What to show in the form: blank on a visit, the rejected submission on an error
    query: String,
    more: bool,
    holdings: Vec<ShownHolding>,
    /// The message for having no copies at all, empty when there is one
    no_holdings: String,
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
    let library = state.archive.library(id).await?.or_not_found()?;
    let holdings = rows
        .iter()
        .enumerate()
        .map(|(i, row)| ShownHolding {
            id: row.id_value(),
            kind: row.kind.as_str(),
            location: row.location.clone(),
            message: errors
                .holdings
                .get(i)
                .cloned()
                .flatten()
                .unwrap_or_default(),
        })
        .collect();
    let page = EntryPage {
        base: base.page("Import", vec![Crumb::home(), Crumb::library(&library)]),
        pending: pending_rows(state, Some(id)).await?,
        show_library: false,
        no_holdings: errors.no_holdings.unwrap_or_default(),
        holdings,
        library,
        query,
        more,
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
    // The import needs the library to exist, but not the library itself
    state.archive.library(id).await?.or_not_found()?;
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
    library: Library,
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
    let library = state.archive.library(library_id).await?.or_not_found()?;
    let Some(import) = pending_import::get(&state.pool, library_id, id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let (draft, saved) = match state.drafts.get(id) {
        Some(draft) => (draft, true),
        None => (import::Draft::seed(&import), false),
    };
    let title = label(&import, &draft.form);
    let input = PublicationRawInput::from(&draft);
    // Errors show for saved drafts only; a fresh import should not open covered in warnings
    let errors = if saved {
        input.parse().err().unwrap_or_default()
    } else {
        PublicationErrors::default()
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
        fields: FormFields::build(&library, input, errors)
            .await?
            .edit_works(WorkEdit::Draft),
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
    let Some(pending) = pending_import::get(&state.pool, library_id, id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    // Read before the submission is consumed; the index names a row, since a row added just now
    // has no draft id to name
    let edit_work = submission.edit_work();
    let draft = load_draft(&state, &pending, submission);
    let next = match edit_work.and_then(|i| draft.works.get(i)) {
        Some(work) => format!("/library/{library_id}/import/{id}/work/{}", work.id),
        None => format!("/library/{library_id}/import/{id}"),
    };
    state.drafts.save(id, draft);
    Ok(Redirect::to(&next).into_response())
}

/// The stored draft with this submission merged into it, or a seeded one when nothing is stored
fn load_draft(state: &AppState, pending: &PendingImport, submission: Submission) -> import::Draft {
    let mut draft = state
        .drafts
        .get(pending.id)
        .unwrap_or_else(|| import::Draft::seed(pending));
    draft.merge(submission.into());
    draft
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
    let draft = load_draft(&state, &pending, submission);
    let (publication, works) = match draft.parse() {
        Ok(parsed) => parsed,
        // Keep the edits so the review page can show what is wrong with them
        Err(_) => {
            state.drafts.save(id, draft);
            return Ok(Redirect::to(&format!("/library/{library_id}/import/{id}")).into_response());
        }
    };
    let publication_id = import::accept(&state.pool, &pending, &publication, &works).await?;
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
#[template(path = "import_work.html")]
struct ImportWorkPage {
    base: BaseContext,
    library: Library,
    import: PendingImport,
    work_id: i64,
    fields: WorkFields,
}

const UNTITLED_WORK: &str = "Untitled work";

/// GET /library/{library_id}/import/{id}/work/{work_id}
///
/// A draft work lives only inside a saved draft, so a draft the store does not have, or an id it
/// does not know, is a 404 rather than a blank form.
pub async fn work(
    _session: Session,
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path((library_id, id, work_id)): Path<(i64, i64, i64)>,
) -> Result<Response, AppError> {
    let library = state.archive.library(library_id).await?.or_not_found()?;
    let Some(import) = pending_import::get(&state.pool, library_id, id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let Some(draft) = state.drafts.get(id) else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let Some(work) = draft.works.iter().find(|work| work.id == work_id) else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let input = WorkRawInput::from(&work.form);
    // As on the review page, a saved draft shows what is wrong with it
    let errors = input.parse().err().unwrap_or_default();
    let title = if input.title.is_empty() {
        UNTITLED_WORK.to_string()
    } else {
        input.title.clone()
    };
    let page = ImportWorkPage {
        base: base.page(
            title,
            vec![
                Crumb::home(),
                Crumb::library(&library),
                Crumb::import(&library),
                Crumb::import_review(&import, &label(&import, &draft.form)),
            ],
        ),
        fields: WorkFields::build(&library, input, errors).await?,
        work_id,
        library,
        import,
    };
    Ok(Html(page.render()?).into_response())
}

/// POST /library/{library_id}/import/{id}/work/{work_id}
pub async fn save_work(
    _session: Session,
    State(state): State<Arc<AppState>>,
    Path((library_id, id, work_id)): Path<(i64, i64, i64)>,
    MultiForm(submission): MultiForm<work_form::Submission>,
) -> Result<Response, AppError> {
    if pending_import::get(&state.pool, library_id, id)
        .await?
        .is_none()
    {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    let Some(mut draft) = state.drafts.get(id) else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    if !draft.set_work(work_id, submission.into()) {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    state.drafts.save(id, draft);
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

/// The old draft as the archive's raw input, so the review page and the draft work page render and
/// validate through the new form types while the draft itself is still built from the old ones.
///
/// Goes when the import routes move onto the archive's own draft.
impl From<&import::Draft> for PublicationRawInput {
    fn from(draft: &import::Draft) -> Self {
        PublicationRawInput {
            title: draft.form.title.clone(),
            publisher: draft.form.publisher.clone(),
            year: draft.form.year.clone(),
            holdings: draft
                .form
                .holdings
                .iter()
                .map(|holding| HoldingRawInput {
                    id: holding.id,
                    kind: match holding.kind {
                        HoldingKind::Physical => ArchiveHoldingKind::Physical,
                        HoldingKind::Digital => ArchiveHoldingKind::Digital,
                    },
                    location: holding.location.clone(),
                })
                .collect(),
            identifiers: draft
                .form
                .identifiers
                .iter()
                .map(|identifier| IdentifierRawInput {
                    kind: identifier.kind.clone(),
                    value: identifier.value.clone(),
                })
                .collect(),
            contributors: draft.form.contributors.iter().map(contributor).collect(),
            // The draft's works are whole; the rows on its form are only what the page shows
            contents: draft
                .works
                .iter()
                .map(|work| WorkRawInput {
                    id: Some(work.id),
                    ..WorkRawInput::from(&work.form)
                })
                .collect(),
        }
    }
}

impl From<&work_form::WorkForm> for WorkRawInput {
    fn from(form: &work_form::WorkForm) -> Self {
        WorkRawInput {
            // A draft work is named by the page it was opened from, not by its input
            id: None,
            title: form.title.clone(),
            key: form.key.clone(),
            time_signature: form.time_signature.clone(),
            instrumentation: form.instrumentation.clone(),
            contributors: form.contributors.iter().map(contributor).collect(),
        }
    }
}

fn contributor(row: &publication_form::ContributorRow) -> ContributorInput {
    ContributorInput {
        name: row.name.clone(),
        role: row.role.clone(),
    }
}
