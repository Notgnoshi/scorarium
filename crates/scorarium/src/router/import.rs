use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, Query, RawForm, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use scorarium_archive::{
    Draft, HoldingErrors, HoldingKind, HoldingRawInput, Library, PendingImport, PublicationErrors,
    PublicationRawInput, ValidationError, WorkRawInput, parse_holdings,
};
use serde::Deserialize;

use super::work::WorkPost;
use super::{
    AppError, BaseContext, Crumb, FormFields, OrNotFound, Session, ShownHolding, WorkEdit,
    WorkFields, age,
};
use crate::AppState;
use crate::publication_post::{self, PublicationPost};

const UNTITLED: &str = "Untitled import";
const UNTITLED_WORK: &str = "Untitled work";

/// One pending import as a list shows it.
pub struct ShownImport {
    pub import: PendingImport,
    pub title: String,
    /// The copies its draft holds, which are the ones the entry page entered until someone edits
    pub holdings: Vec<HoldingRawInput>,
    pub age: String,
}

fn shown(import: PendingImport) -> ShownImport {
    let draft = import.draft();
    ShownImport {
        title: label(&import, &draft.input),
        holdings: draft.input.holdings,
        age: age(import.created_at),
        import,
    }
}

/// What to call a pending import: its draft's title, else what was typed, else a placeholder.
fn label(import: &PendingImport, draft: &PublicationRawInput) -> String {
    [draft.title.as_str(), import.query.as_str(), UNTITLED]
        .into_iter()
        .find(|text| !text.is_empty())
        .unwrap_or(UNTITLED)
        .to_string()
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
    pending: Vec<ShownImport>,
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
    let library = state.archive.library(id).await?.or_not_found()?;
    let blank = vec![HoldingRawInput {
        id: None,
        kind: HoldingKind::Physical,
        location: String::new(),
    }];
    render_entry(
        &library,
        base,
        String::new(),
        query.more.is_some(),
        blank,
        HoldingErrors::default(),
    )
    .await
}

async fn render_entry(
    library: &Library,
    base: BaseContext,
    query: String,
    more: bool,
    raw: Vec<HoldingRawInput>,
    errors: HoldingErrors,
) -> Result<Response, AppError> {
    let holdings = raw
        .iter()
        .enumerate()
        .map(|(i, holding)| ShownHolding {
            id: holding.id.map(|id| id.to_string()).unwrap_or_default(),
            kind: holding.kind.as_str(),
            location: holding.location.clone(),
            message: message(errors.each.get(i).and_then(Option::as_ref)),
        })
        .collect();
    let page = EntryPage {
        base: base.page("Import", vec![Crumb::home(), Crumb::library(library)]),
        pending: library
            .pending_imports()
            .await?
            .into_iter()
            .map(shown)
            .collect(),
        show_library: false,
        no_holdings: message(errors.none.as_ref()),
        library: library.clone(),
        holdings,
        query,
        more,
    };
    Ok(Html(page.render()?).into_response())
}

/// Everything the entry page posts under a fixed key; its copies come from the raw pairs, as on
/// the review page.
#[derive(Deserialize)]
pub struct StartForm {
    #[serde(default)]
    query: String,
    /// Present when the "Import more" box is checked; browsers send "on".
    more: Option<String>,
}

/// POST /library/{id}/import
pub async fn start(
    _session: Session,
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path(id): Path<i64>,
    RawForm(body): RawForm,
) -> Result<Response, AppError> {
    let form: StartForm = publication_post::decode_form(&body)?;
    let pairs: Vec<(String, String)> = publication_post::decode_form(&body)?;
    let raw = publication_post::holdings(&pairs);
    let more = form.more.is_some();
    let library = state.archive.library(id).await?.or_not_found()?;
    let holdings = match parse_holdings(&raw) {
        Ok(holdings) => holdings,
        Err(errors) => return render_entry(&library, base, form.query, more, raw, errors).await,
    };
    let import = library.start_import(&form.query, &holdings).await?;
    let next = if more {
        format!("/library/{id}/import?more=1")
    } else {
        format!("/library/{id}/import/{}", import.id)
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
    let import = library.pending_import(id).await?.or_not_found()?;
    let draft = import.draft();
    let title = label(&import, &draft.input);
    // Errors show for saved drafts only; a fresh import should not open covered in warnings
    let errors = if draft.saved {
        draft.input.parse().err().unwrap_or_default()
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
        fields: FormFields::build(&library, draft.input, errors)
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
    RawForm(body): RawForm,
) -> Result<Response, AppError> {
    let post = PublicationPost::decode(&body)?;
    let library = state.archive.library(library_id).await?.or_not_found()?;
    let import = library.pending_import(id).await?.or_not_found()?;
    // Read before the submission is consumed. It names a work by position, since a work added just
    // now has no draft id to name it by.
    let edit_work = post.edit_work();
    let draft = import.save_draft(post.merge(import.draft().input.contents));
    let next = match edit_work.and_then(|i| draft.input.contents.get(i)) {
        Some(work) => {
            let work_id = work.id.expect("saving a draft names every work it holds");
            format!("/library/{library_id}/import/{id}/work/{work_id}")
        }
        None => format!("/library/{library_id}/import/{id}"),
    };
    Ok(Redirect::to(&next).into_response())
}

/// POST /library/{library_id}/import/{id}/submit
pub async fn submit(
    _session: Session,
    State(state): State<Arc<AppState>>,
    Path((library_id, id)): Path<(i64, i64)>,
    RawForm(body): RawForm,
) -> Result<Response, AppError> {
    let post = PublicationPost::decode(&body)?;
    let library = state.archive.library(library_id).await?.or_not_found()?;
    let import = library.pending_import(id).await?.or_not_found()?;
    let input = post.merge(import.draft().input.contents);
    match input.parse() {
        Ok(parsed) => {
            let publication = import.accept_into_publication(&parsed).await?;
            Ok(Redirect::to(&format!(
                "/library/{library_id}/publication/{}",
                publication.id
            ))
            .into_response())
        }
        // Keep the edits, so the review page can show what is wrong with them
        Err(_) => {
            import.save_draft(input);
            Ok(Redirect::to(&format!("/library/{library_id}/import/{id}")).into_response())
        }
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

/// GET /library/{library_id}/import/{id}/work/{work_id}
///
/// A draft work lives only inside a saved draft, so an unsaved draft, or an id it does not know,
/// is a 404 rather than a blank form.
pub async fn work(
    _session: Session,
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path((library_id, id, work_id)): Path<(i64, i64, i64)>,
) -> Result<Response, AppError> {
    let library = state.archive.library(library_id).await?.or_not_found()?;
    let import = library.pending_import(id).await?.or_not_found()?;
    let draft = import.draft();
    let Some(input) = draft_work(&draft, work_id).cloned() else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
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
                Crumb::import_review(&import, &label(&import, &draft.input)),
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
    RawForm(body): RawForm,
) -> Result<Response, AppError> {
    let post: WorkPost = publication_post::decode_form(&body)?;
    let library = state.archive.library(library_id).await?.or_not_found()?;
    let import = library.pending_import(id).await?.or_not_found()?;
    let mut draft = import.draft();
    if draft_work(&draft, work_id).is_none() {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    let edited = WorkRawInput {
        // The page names the work it edits, so what it posts need not
        id: Some(work_id),
        ..WorkRawInput::from(post)
    };
    for work in &mut draft.input.contents {
        if work.id == Some(work_id) {
            *work = edited;
            break;
        }
    }
    import.save_draft(draft.input);
    Ok(Redirect::to(&format!("/library/{library_id}/import/{id}")).into_response())
}

/// The work a saved draft holds under this id. An unsaved draft holds none: a draft work comes
/// into being only when the review page is saved.
fn draft_work(draft: &Draft, work_id: i64) -> Option<&WorkRawInput> {
    draft
        .saved
        .then(|| {
            draft
                .input
                .contents
                .iter()
                .find(|work| work.id == Some(work_id))
        })
        .flatten()
}

#[derive(Template)]
#[template(path = "review.html")]
struct QueuePage {
    base: BaseContext,
    pending: Vec<ShownImport>,
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
        pending: state
            .archive
            .pending_imports()
            .await?
            .into_iter()
            .map(shown)
            .collect(),
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
    let library = state.archive.library(library_id).await?.or_not_found()?;
    library
        .pending_import(id)
        .await?
        .or_not_found()?
        .discard()
        .await?;
    Ok(Redirect::to(&format!("/library/{library_id}/import")).into_response())
}

/// A message for the page, empty when there is nothing wrong.
fn message(error: Option<&ValidationError>) -> String {
    error.map(ToString::to_string).unwrap_or_default()
}
