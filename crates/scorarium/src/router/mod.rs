mod assets;
mod import;
mod index;
mod library;
mod login;
mod password;
mod person;
mod publication;
mod work;

use std::collections::HashMap;
use std::sync::Arc;

use axum::Router;
use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum_extra::extract::CookieJar;
use tower_http::trace::TraceLayer;

use crate::{AppState, db, publication_form, work_form};

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

    pub fn library(library: &db::Library) -> Self {
        Self {
            label: library.name.clone(),
            href: format!("/library/{}", library.id),
        }
    }

    pub fn import(library: &db::Library) -> Self {
        Self {
            label: "Import".to_string(),
            href: format!("/library/{}/import", library.id),
        }
    }

    pub fn publication(publication: &db::publication::Publication) -> Self {
        Self {
            label: publication.title.clone(),
            href: format!(
                "/library/{}/publication/{}",
                publication.library_id, publication.id
            ),
        }
    }

    /// The import under review, by the label its page shows.
    pub fn import_review(import: &db::pending_import::PendingImport, label: &str) -> Self {
        Self {
            label: label.to_string(),
            href: format!("/library/{}/import/{}", import.library_id, import.id),
        }
    }

    pub fn work(work: &db::work::Work) -> Self {
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
            db::pending_import::count(&state.pool).await?
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

/// What a work row's edit control does, which is a property of the page rather than the row.
#[derive(Default)]
pub enum RowEdit {
    /// Link to the stored work's edit page, which returns to `back` when it is done
    Stored { back: String },
    /// Save the draft and open the draft work, since a draft work has no stable link of its own
    #[default]
    Draft,
}

impl RowEdit {
    pub fn is_draft(&self) -> bool {
        matches!(self, RowEdit::Draft)
    }

    /// Where a stored row's edit link returns to. Empty for a draft row, which has no link.
    pub fn back(&self) -> &str {
        match self {
            RowEdit::Stored { back } => back,
            RowEdit::Draft => "",
        }
    }
}

/// Everything the shared publication form fragment renders. The import review page and the
/// publication edit page show the same fields, so they build the same context for them.
pub struct FormFields {
    pub form: publication_form::PublicationForm,
    pub errors: publication_form::Errors,
    // Rows paired with their error, empty when there is none, so the row macros take plain strings
    // for both the filled rows and the blank template row.
    pub holding_rows: Vec<(publication_form::HoldingRow, String)>,
    pub identifier_rows: Vec<(publication_form::IdentifierRow, String)>,
    pub contributor_rows: Vec<(publication_form::ContributorRow, String)>,
    // A work row shows one contributor, so it also carries how many more the work credits, empty
    // when it credits no others
    pub work_rows: Vec<(publication_form::WorkRow, String, String)>,
    // Datalist suggestions for the role and name inputs
    pub roles: Vec<String>,
    pub names: Vec<String>,
    pub no_copies_warning: String,
    pub row_edit: RowEdit,
}

impl FormFields {
    /// `contributors` is how many people each work credits, by the id its rows carry: the stored
    /// works' ids on the publication edit page, the draft's own on the review page.
    pub async fn build(
        pool: &sqlx::SqlitePool,
        library_id: i64,
        form: publication_form::PublicationForm,
        errors: publication_form::Errors,
        contributors: &HashMap<i64, usize>,
    ) -> sqlx::Result<Self> {
        let (roles, names) = suggestions(pool, library_id).await?;
        Ok(Self {
            holding_rows: pair_errors(&form.holdings, &errors.holdings),
            identifier_rows: pair_errors(&form.identifiers, &errors.identifiers),
            contributor_rows: pair_errors(&form.contributors, &errors.contributors),
            work_rows: work_rows(&form.works, &errors.works, contributors),
            no_copies_warning: String::new(),
            row_edit: RowEdit::default(),
            roles,
            names,
            form,
            errors,
        })
    }

    /// What to warn when the last copy row is removed, on the page that can act on it.
    pub fn warn_when_empty(mut self, warning: &str) -> Self {
        self.no_copies_warning = warning.to_string();
        self
    }

    /// What a work row's edit control does on this page.
    pub fn edit_works(mut self, row_edit: RowEdit) -> Self {
        self.row_edit = row_edit;
        self
    }
}

/// Everything the shared work form fragment renders. The stored work edit page and the draft work
/// page show the same fields, so they build the same context for them.
pub struct WorkFields {
    pub form: work_form::WorkForm,
    pub errors: work_form::Errors,
    pub contributor_rows: Vec<(publication_form::ContributorRow, String)>,
    pub roles: Vec<String>,
    pub names: Vec<String>,
}

impl WorkFields {
    pub async fn build(
        pool: &sqlx::SqlitePool,
        library_id: i64,
        form: work_form::WorkForm,
        errors: work_form::Errors,
    ) -> sqlx::Result<Self> {
        let (roles, names) = suggestions(pool, library_id).await?;
        Ok(Self {
            contributor_rows: pair_errors(&form.contributors, &errors.contributors),
            roles,
            names,
            form,
            errors,
        })
    }
}

/// Datalist suggestions for the role and name inputs, as (roles, names).
async fn suggestions(
    pool: &sqlx::SqlitePool,
    library_id: i64,
) -> sqlx::Result<(Vec<String>, Vec<String>)> {
    let roles = db::person::list_roles(pool, library_id)
        .await?
        .into_iter()
        .chain(CONVENTIONAL_ROLES.iter().map(|r| r.to_string()))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok((roles, db::person::list_names(pool, library_id).await?))
}

/// Pair each work row with the number of contributors the row does not show and its message. A row
/// naming no work yet shows none.
fn work_rows(
    rows: &[publication_form::WorkRow],
    errors: &[Option<String>],
    contributors: &HashMap<i64, usize>,
) -> Vec<(publication_form::WorkRow, String, String)> {
    pair_errors(rows, errors)
        .into_iter()
        .map(|(row, error)| {
            let more = row
                .id
                .and_then(|id| contributors.get(&id))
                // A work may credit nobody at all, so the row shows one contributor fewer than the
                // work has only when it has any
                .map(|count| count.saturating_sub(1))
                .filter(|more| *more > 0)
                .map(|more| more.to_string())
                .unwrap_or_default();
            (row, more, error)
        })
        .collect()
}

/// Pair each row with its message. A form that parsed clean has no error slots at all, so the rows
/// cannot simply be zipped with the errors.
fn pair_errors<T: Clone>(rows: &[T], errors: &[Option<String>]) -> Vec<(T, String)> {
    rows.iter()
        .cloned()
        .enumerate()
        .map(|(i, row)| (row, errors.get(i).cloned().flatten().unwrap_or_default()))
        .collect()
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(index::index))
        .route("/assets/{*name}", get(assets::asset))
        .route("/login", get(login::login_form).post(login::login))
        .route("/logout", post(login::logout))
        .route(
            "/password",
            get(password::password_form).post(password::change_password),
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
        tracing::error!(error = ?self.0, "handler error");
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }
}

impl<E: Into<color_eyre::Report>> From<E> for AppError {
    fn from(err: E) -> Self {
        Self(err.into())
    }
}
