mod assets;
mod import;
mod index;
mod library;
mod login;
mod person;
mod publication;
mod settings;
mod suggest;
mod work;

use std::sync::Arc;

use axum::Router;
use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum_extra::extract::CookieJar;
use scorarium_archive::{
    CatalogNumber, ContributorInput, HoldingRawInput, IdentifierRawInput, Library, NotFound,
    PendingImport, Publication, PublicationErrors, PublicationRawInput, ValidationError, Work,
    WorkErrors, WorkRawInput,
};
use tower_http::trace::TraceLayer;

use crate::AppState;
use crate::publication_post::{self, BadForm};

/// The name of the cookie holding the login session token.
const SESSION_COOKIE: &str = "session";

pub struct Crumb {
    pub label: String,
    pub href: String,
}

impl Crumb {
    pub fn home() -> Self {
        Self {
            label: "Home".to_string(),
            href: "/".to_string(),
        }
    }

    pub fn library(library: &Library) -> Self {
        Self {
            label: library.name.clone(),
            href: format!("/library/{}", library.id),
        }
    }

    pub fn import(library: &Library) -> Self {
        Self {
            label: "Import".to_string(),
            href: format!("/library/{}/import", library.id),
        }
    }

    pub fn publication(publication: &Publication) -> Self {
        Self {
            label: publication.title.clone(),
            href: format!(
                "/library/{}/publication/{}",
                publication.library_id, publication.id
            ),
        }
    }

    /// The import under review, by the label its page shows.
    pub fn import_review(import: &PendingImport, label: &str) -> Self {
        Self {
            label: label.to_string(),
            href: format!("/library/{}/import/{}", import.library_id, import.id),
        }
    }

    pub fn settings() -> Self {
        Self {
            label: "Settings".to_string(),
            href: "/settings".to_string(),
        }
    }

    pub fn work(work: &Work) -> Self {
        Self {
            label: work.title.clone(),
            href: format!("/library/{}/work/{}", work.library_id, work.id),
        }
    }
}

pub struct BaseContext {
    pub title: String,
    /// The request path, so header links to the current page can be hidden.
    pub path: String,
    pub logged_in: bool,
    pub demo: bool,
    /// Imports awaiting review, for the header badge. Zero when logged out.
    pub pending_import_count: i64,
    pub breadcrumbs: Vec<Crumb>,
    pub bootstrap_css: String,
    pub bootstrap_icons_css: String,
    pub bootstrap_js: String,
}

/// The request fills in everything the header needs; the handler adds the title and breadcrumbs with [BaseContext::page]
impl FromRequestParts<Arc<AppState>> for BaseContext {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_headers(&parts.headers);
        let logged_in = state.demo
            || jar
                .get(SESSION_COOKIE)
                .is_some_and(|cookie| state.sessions.validate(cookie.value()));
        let pending_import_count = if logged_in {
            state.archive.pending_import_count().await?
        } else {
            0
        };
        Ok(Self {
            title: String::new(),
            path: parts.uri.path().to_string(),
            logged_in,
            demo: state.demo,
            pending_import_count,
            breadcrumbs: Vec::new(),
            bootstrap_css: assets::url("bootstrap.min.css"),
            bootstrap_icons_css: assets::url("bootstrap-icons.min.css"),
            bootstrap_js: assets::url("bootstrap.bundle.min.js"),
        })
    }
}

impl BaseContext {
    /// Finish the context with what only the handler knows.
    pub fn page(mut self, title: impl Into<String>, breadcrumbs: Vec<Crumb>) -> Self {
        self.title = title.into();
        self.breadcrumbs = breadcrumbs;
        self
    }
}

/// Suggested alongside the library's existing roles, so a new library still gets a datalist.
const CONVENTIONAL_ROLES: [&str; 5] = ["arranger", "author", "composer", "editor", "translator"];

/// What a work shows when its only problem is a field the publication form does not reach.
const HIDDEN_WORK_PROBLEM: &str = "A hidden field is incomplete. Open the work to fix it.";

/// What a work's edit control does, which is a property of the page rather than the work.
#[derive(Default)]
pub enum WorkEdit {
    /// Link to the stored work's edit page, which returns to `back` when it is done
    Stored { back: String },
    /// Save the draft and open the draft work, since a draft work has no stable link of its own
    #[default]
    Draft,
}

impl WorkEdit {
    pub fn is_draft(&self) -> bool {
        matches!(self, WorkEdit::Draft)
    }

    /// Where a stored work's edit link returns to. Empty for a draft work, which has no link.
    pub fn back(&self) -> &str {
        match self {
            WorkEdit::Stored { back } => back,
            WorkEdit::Draft => "",
        }
    }
}

/// One copy as the form shows it. The macros take plain strings, so that a filled copy and the
/// blank template copy render through the same code.
pub struct ShownHolding {
    /// The hidden field's value: the stored copy's id, empty for one being added
    pub id: String,
    pub kind: &'static str,
    pub location: String,
    pub message: String,
}

/// One work as the publication form shows it: its title, and the one catalog number and the one
/// contributor the page picks.
pub struct ShownWork {
    /// The hidden field's value: the work's id, empty for one being added
    pub id: String,
    pub title: String,
    pub catalog_number: String,
    pub recognized: bool,
    /// How many catalog numbers the form does not show, empty when it shows them all
    pub more_numbers: String,
    pub name: String,
    pub role: String,
    /// How many contributors the form does not show, empty when it shows them all
    pub more: String,
    pub message: String,
}

/// Everything the shared publication form fragment renders. The import review page and the
/// publication edit page show the same fields, so they build the same context for them.
pub struct FormFields {
    pub input: PublicationRawInput,
    pub errors: PublicationErrors,
    pub holdings: Vec<ShownHolding>,
    /// The message for having no copies at all, empty when there is one
    pub no_holdings: String,
    pub identifiers: Vec<(IdentifierRawInput, String)>,
    pub contributors: Vec<(ContributorInput, String)>,
    pub works: Vec<ShownWork>,
    // Datalist suggestions for the role and name inputs
    pub roles: Vec<String>,
    pub names: Vec<String>,
    pub no_copies_warning: String,
    pub work_edit: WorkEdit,
}

impl FormFields {
    pub async fn build(
        library: &Library,
        input: PublicationRawInput,
        errors: PublicationErrors,
    ) -> Result<Self, AppError> {
        let (roles, names) = suggestions(library).await?;
        Ok(Self {
            holdings: shown_holdings(&input.holdings, &errors.holdings.each),
            no_holdings: message(&errors.holdings.none),
            identifiers: pair_messages(&input.identifiers, &errors.identifiers),
            contributors: pair_messages(&input.contributors, &errors.contributors),
            works: shown_works(&input.contents, &errors.contents),
            no_copies_warning: String::new(),
            work_edit: WorkEdit::default(),
            roles,
            names,
            input,
            errors,
        })
    }

    /// What to warn when the last copy is removed, on the page that can act on it.
    pub fn warn_when_empty(mut self, warning: &str) -> Self {
        self.no_copies_warning = warning.to_string();
        self
    }

    /// What a work's edit control does on this page.
    pub fn edit_works(mut self, work_edit: WorkEdit) -> Self {
        self.work_edit = work_edit;
        self
    }
}

/// One catalog number as the work form shows it, with what the parser made of it
pub struct ShownCatalogNumber {
    pub value: String,
    pub recognized: bool,
    pub message: String,
}

/// Everything the shared work form fragment renders. The stored work edit page and the draft work
/// page show the same fields, so they build the same context for them.
pub struct WorkFields {
    pub input: WorkRawInput,
    pub errors: WorkErrors,
    pub contributors: Vec<(ContributorInput, String)>,
    pub catalog_numbers: Vec<ShownCatalogNumber>,
    pub roles: Vec<String>,
    pub names: Vec<String>,
}

impl WorkFields {
    pub async fn build(
        library: &Library,
        input: WorkRawInput,
        errors: WorkErrors,
    ) -> Result<Self, AppError> {
        let (roles, names) = suggestions(library).await?;
        Ok(Self {
            contributors: pair_messages(&input.contributors, &errors.contributors),
            catalog_numbers: shown_catalog_numbers(&input.catalog_numbers, &errors.catalog_numbers),
            roles,
            names,
            input,
            errors,
        })
    }
}

fn shown_catalog_numbers(
    values: &[String],
    errors: &[Option<ValidationError>],
) -> Vec<ShownCatalogNumber> {
    values
        .iter()
        .enumerate()
        .map(|(i, value)| ShownCatalogNumber {
            value: value.clone(),
            recognized: CatalogNumber::parse(value).is_recognized(),
            message: message(errors.get(i).unwrap_or(&None)),
        })
        .collect()
}

/// Datalist suggestions for the role and name inputs, as (roles, names).
async fn suggestions(library: &Library) -> Result<(Vec<String>, Vec<String>), AppError> {
    let roles = library
        .roles()
        .await?
        .into_iter()
        .chain(CONVENTIONAL_ROLES.iter().map(|role| role.to_string()))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok((roles, library.person_names().await?))
}

fn shown_holdings(
    holdings: &[HoldingRawInput],
    errors: &[Option<ValidationError>],
) -> Vec<ShownHolding> {
    holdings
        .iter()
        .enumerate()
        .map(|(i, holding)| ShownHolding {
            id: holding.id.map(|id| id.to_string()).unwrap_or_default(),
            kind: holding.kind.as_str(),
            location: holding.location.clone(),
            message: message(errors.get(i).unwrap_or(&None)),
        })
        .collect()
}

fn shown_works(contents: &[WorkRawInput], errors: &[WorkErrors]) -> Vec<ShownWork> {
    let no_errors = WorkErrors::default();
    contents
        .iter()
        .enumerate()
        .map(|(i, work)| {
            let errors = errors.get(i).unwrap_or(&no_errors);
            let lead = publication_post::lead_contributor(&work.contributors);
            let shown = lead
                .map(|i| work.contributors[i].clone())
                .unwrap_or_default();
            let lead_number = publication_post::lead_catalog_number(&work.catalog_numbers);
            let number = lead_number
                .map(|i| work.catalog_numbers[i].clone())
                .unwrap_or_default();
            ShownWork {
                id: work.id.map(|id| id.to_string()).unwrap_or_default(),
                title: work.title.clone(),
                recognized: CatalogNumber::parse(&number).is_recognized(),
                catalog_number: number,
                // A work may carry no number at all, so say how many are hidden only when any are
                more_numbers: match work.catalog_numbers.len() {
                    0 | 1 => String::new(),
                    numbered => (numbered - 1).to_string(),
                },
                name: shown.name,
                role: shown.role,
                // A work may credit nobody at all, so say how many are hidden only when any are
                more: match work.contributors.len() {
                    0 | 1 => String::new(),
                    credited => (credited - 1).to_string(),
                },
                message: work_message(errors, lead, lead_number),
            }
        })
        .collect()
}

/// What a work says on the publication form: the problem with a field it shows, else a note that
/// the problem is one the form cannot reach, so a refused submit is always explainable.
fn work_message(errors: &WorkErrors, lead: Option<usize>, lead_number: Option<usize>) -> String {
    if let Some(error) = &errors.title {
        return error.to_string();
    }
    if let Some(error) = lead_number
        .and_then(|i| errors.catalog_numbers.get(i))
        .and_then(Option::as_ref)
    {
        return error.to_string();
    }
    if let Some(error) = lead
        .and_then(|i| errors.contributors.get(i))
        .and_then(Option::as_ref)
    {
        return error.to_string();
    }
    if errors
        .contributors
        .iter()
        .chain(errors.catalog_numbers.iter())
        .any(Option::is_some)
    {
        return HIDDEN_WORK_PROBLEM.to_string();
    }
    String::new()
}

/// Pair each value with its message. An input that parsed clean has no message slots at all, so
/// the values cannot simply be zipped with the errors.
fn pair_messages<T: Clone>(values: &[T], errors: &[Option<ValidationError>]) -> Vec<(T, String)> {
    values
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, value)| (value, message(errors.get(i).unwrap_or(&None))))
        .collect()
}

/// A message for the page, empty when there is nothing wrong.
fn message(error: &Option<ValidationError>) -> String {
    error
        .as_ref()
        .map(ValidationError::to_string)
        .unwrap_or_default()
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(index::index))
        .route("/assets/{*name}", get(assets::asset))
        .route("/login", get(login::login_form).post(login::login))
        .route("/logout", post(login::logout))
        .route("/settings", get(settings::settings_page))
        .route("/settings/password", post(settings::change_password))
        .route(
            "/settings/catalog-numbers",
            get(settings::catalog_numbers_page),
        )
        .route("/review", get(import::queue))
        .route("/library", post(library::create))
        .route("/library/{id}", get(library::library))
        .route("/library/{id}/rename", post(library::rename))
        .route("/library/{id}/delete", post(library::delete))
        .route(
            "/library/{id}/import",
            get(import::entry).post(import::start),
        )
        .route("/library/{library_id}/import/{id}", get(import::review))
        .route("/library/{library_id}/import/{id}/save", post(import::save))
        .route(
            "/library/{library_id}/import/{id}/work/{work_id}",
            get(import::work).post(import::save_work),
        )
        .route(
            "/library/{library_id}/import/{id}/submit",
            post(import::submit),
        )
        .route(
            "/library/{library_id}/import/{id}/delete",
            post(import::delete),
        )
        .route(
            "/library/{library_id}/publication/{id}",
            get(publication::publication),
        )
        .route(
            "/library/{library_id}/publication/{id}/edit",
            get(publication::edit).post(publication::save),
        )
        .route(
            "/library/{library_id}/publication/{id}/delete",
            post(publication::delete),
        )
        .route(
            "/library/{id}/suggest/catalog-numbers",
            get(suggest::catalog_numbers),
        )
        .route("/library/{library_id}/work/{id}", get(work::work))
        .route(
            "/library/{library_id}/work/{id}/edit",
            get(work::edit).post(work::save),
        )
        .route("/library/{library_id}/person/{id}", get(person::person))
        .route("/library/{id}/composers", get(person::composers))
        .route("/library/{id}/authors", get(person::authors))
        .with_state(state)
        // Applies only to the routes added above it, so keep this last.
        .layer(TraceLayer::new_for_http())
}

/// The session token of a logged-in request.
struct Session(String);

impl FromRequestParts<Arc<AppState>> for Session {
    type Rejection = Redirect;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        if state.demo {
            return Ok(Session(String::new()));
        }
        let jar = CookieJar::from_headers(&parts.headers);
        match jar.get(SESSION_COOKIE) {
            Some(cookie) if state.sessions.validate(cookie.value()) => {
                Ok(Session(cookie.value().to_string()))
            }
            _ => Err(Redirect::to("/login")),
        }
    }
}

pub struct AppError(color_eyre::Report);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        if self.0.downcast_ref::<NotFound>().is_some() {
            return StatusCode::NOT_FOUND.into_response();
        }
        if self.0.downcast_ref::<BadForm>().is_some() {
            return StatusCode::UNPROCESSABLE_ENTITY.into_response();
        }
        tracing::error!(error = ?self.0, "handler error");
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }
}

impl<E: Into<color_eyre::Report>> From<E> for AppError {
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

/// Return 404 for a lookup that found nothing
pub trait OrNotFound<T> {
    fn or_not_found(self) -> Result<T, AppError>;
}

impl<T> OrNotFound<T> for Option<T> {
    fn or_not_found(self) -> Result<T, AppError> {
        self.ok_or_else(|| AppError(NotFound.into()))
    }
}
