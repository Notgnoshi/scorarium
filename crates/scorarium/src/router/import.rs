use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use askama::Template;
use axum::Form;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum_extra::extract::Form as MultiForm;
use serde::Deserialize;

use super::{AppError, BaseContext, Crumb, Session};
use crate::db::pending_import::{self, NewPendingImport, PendingImport};
use crate::db::publication::HoldingKind;
use crate::import::{ContributorRow, Draft, Errors, IdentifierRow};
use crate::{AppState, db, import};

const UNTITLED: &str = "Untitled import";

/// Suggested alongside the library's existing roles, so a new library still gets a datalist.
const CONVENTIONAL_ROLES: [&str; 5] = ["arranger", "author", "composer", "editor", "translator"];

pub struct PendingRow {
    pub import: PendingImport,
    pub title: String,
    pub kind: HoldingKind,
    pub location: String,
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
                .unwrap_or_else(|| Draft::seed(&import));
            PendingRow {
                title: label(&import, &draft),
                kind: draft.kind,
                location: draft.location,
                age: age(import.created_at),
                import,
            }
        })
        .collect())
}

/// What to call a pending import: its draft's title, else what was typed, else a placeholder.
fn label(import: &PendingImport, draft: &Draft) -> String {
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
    form: StartForm,
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
    let form = StartForm {
        more: query.more,
        ..StartForm::default()
    };
    render_entry(&state, id, base, form, None).await
}

async fn render_entry(
    state: &AppState,
    id: i64,
    base: BaseContext,
    form: StartForm,
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
        form,
        error,
    };
    Ok(Html(page.render()?).into_response())
}

#[derive(Deserialize)]
pub struct StartForm {
    #[serde(default)]
    query: String,
    kind: HoldingKind,
    // Both inputs submit and the kind picks which one counts
    #[serde(default)]
    location: String,
    #[serde(default)]
    file: String,
    /// Present when the "Import more" box is checked; browsers send "on".
    more: Option<String>,
}

impl Default for StartForm {
    fn default() -> Self {
        Self {
            query: String::new(),
            kind: HoldingKind::Physical,
            location: String::new(),
            file: String::new(),
            more: None,
        }
    }
}

/// POST /library/{id}/import
pub async fn start(
    _session: Session,
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path(id): Path<i64>,
    Form(form): Form<StartForm>,
) -> Result<Response, AppError> {
    let location = match form.kind {
        HoldingKind::Physical => form.location.trim(),
        HoldingKind::Digital => form.file.trim(),
    }
    .to_string();
    if form.kind == HoldingKind::Digital && location.is_empty() {
        return render_entry(
            &state,
            id,
            base,
            form,
            Some("Choose a file for a digital copy."),
        )
        .await;
    }
    if db::get_library(&state.pool, id).await?.is_none() {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    let more = form.more.is_some();
    let pending_id = pending_import::create(
        &state.pool,
        &NewPendingImport {
            library_id: id,
            query: form.query.trim(),
            kind: form.kind,
            location: (!location.is_empty()).then_some(location.as_str()),
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
    errors: Errors,
    // Draft rows paired with their error, empty when there is none, so the row macro takes plain
    // strings for both the saved rows and the blank template row.
    identifier_rows: Vec<(IdentifierRow, String)>,
    contributor_rows: Vec<(ContributorRow, String)>,
    // Datalist suggestions for the role input
    roles: Vec<String>,
}

/// The message for row `i`. A draft that parsed clean has no error slots at all, so the rows
/// cannot simply be zipped with the errors.
fn row_error(errors: &[Option<String>], i: usize) -> String {
    errors.get(i).cloned().flatten().unwrap_or_default()
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
        None => (Draft::seed(&import), Errors::default()),
    };
    let title = label(&import, &draft);
    let identifier_rows = draft
        .identifiers
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, row)| (row, row_error(&errors.identifiers, i)))
        .collect();
    let contributor_rows = draft
        .contributors
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, row)| (row, row_error(&errors.contributors, i)))
        .collect();
    let roles = db::person::list_roles(&state.pool, library_id)
        .await?
        .into_iter()
        .chain(CONVENTIONAL_ROLES.iter().map(|r| r.to_string()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
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
        errors,
        identifier_rows,
        contributor_rows,
        roles,
    };
    Ok(Html(page.render()?).into_response())
}

#[derive(Deserialize)]
pub struct ReviewForm {
    title: String,
    publisher: String,
    year: String,
    kind: HoldingKind,
    // Like the entry page, both inputs submit and the kind picks which one counts
    #[serde(default)]
    location: String,
    #[serde(default)]
    file: String,
    // Parallel repeated keys, one entry per row; `default` covers a submission with no rows
    #[serde(default)]
    identifier_kind: Vec<String>,
    #[serde(default)]
    identifier_value: Vec<String>,
    #[serde(default)]
    contributor_name: Vec<String>,
    #[serde(default)]
    contributor_role: Vec<String>,
}

impl From<ReviewForm> for Draft {
    fn from(form: ReviewForm) -> Self {
        let identifiers = form
            .identifier_kind
            .into_iter()
            .zip(form.identifier_value)
            .map(|(kind, value)| IdentifierRow {
                kind: kind.trim().to_string(),
                value: value.trim().to_string(),
            })
            .collect();
        let contributors = form
            .contributor_name
            .into_iter()
            .zip(form.contributor_role)
            .map(|(name, role)| ContributorRow {
                name: name.trim().to_string(),
                role: role.trim().to_string(),
            })
            .collect();
        Draft {
            title: form.title.trim().to_string(),
            publisher: form.publisher.trim().to_string(),
            year: form.year.trim().to_string(),
            kind: form.kind,
            location: match form.kind {
                HoldingKind::Physical => form.location,
                HoldingKind::Digital => form.file,
            }
            .trim()
            .to_string(),
            identifiers,
            contributors,
        }
    }
}

/// POST /library/{library_id}/import/{id}/save
pub async fn save(
    _session: Session,
    State(state): State<Arc<AppState>>,
    Path((library_id, id)): Path<(i64, i64)>,
    MultiForm(form): MultiForm<ReviewForm>,
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

/// POST /library/{library_id}/import/{id}/submit
pub async fn submit(
    _session: Session,
    State(state): State<Arc<AppState>>,
    Path((library_id, id)): Path<(i64, i64)>,
    MultiForm(form): MultiForm<ReviewForm>,
) -> Result<Response, AppError> {
    let Some(pending) = pending_import::get(&state.pool, library_id, id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let draft: Draft = form.into();
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
