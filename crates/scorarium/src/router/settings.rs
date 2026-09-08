use std::sync::Arc;

use askama::Template;
use axum::Form;
use axum::extract::State;
use axum::http::header;
use axum::response::{Html, IntoResponse, Redirect, Response};
use scorarium_archive::{CatalogNumberEntry, NotFound, PasswordCheck};
use serde::Deserialize;

use super::{AppError, BaseContext, Crumb, Session};
use crate::AppState;

/// GET /settings
pub async fn settings_page(
    _session: Session,
    State(state): State<Arc<AppState>>,
    base: BaseContext,
) -> Result<Response, AppError> {
    let unrecognized = unrecognized_catalog_numbers(&state).await?.len();
    Ok(no_store(settings(base, None, unrecognized)?))
}

/// GET /settings/catalog-numbers
pub async fn catalog_numbers_page(
    _session: Session,
    State(state): State<Arc<AppState>>,
    base: BaseContext,
) -> Result<Response, AppError> {
    let page = CatalogNumbersPage {
        base: base.page(
            "Unrecognized catalog numbers",
            vec![Crumb::home(), Crumb::settings()],
        ),
        entries: unrecognized_catalog_numbers(&state).await?,
    };
    Ok(no_store(page.render()?))
}

/// The numbers the parser does not recognize, in the order the archive lists them
///
/// A work with two composers is listed once per composer, so the entries are deduplicated by the
/// number they name rather than shown twice.
async fn unrecognized_catalog_numbers(
    state: &AppState,
) -> Result<Vec<CatalogNumberEntry>, AppError> {
    let mut seen = Vec::new();
    let mut entries = Vec::new();
    for entry in state.archive.all_catalog_numbers().await? {
        if entry.number.is_recognized() {
            continue;
        }
        let key = (entry.work_id, entry.number.as_str().to_string());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        entries.push(entry);
    }
    Ok(entries)
}

#[derive(Deserialize)]
pub struct PasswordForm {
    current: String,
    new: String,
    confirm: String,
}

/// POST /settings/password - handle results of the change-password form
pub async fn change_password(
    Session(token): Session,
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Form(form): Form<PasswordForm>,
) -> Result<Response, AppError> {
    // The demo hands out a session to everyone, so it must not hand out the password with it
    if state.demo {
        return Err(NotFound.into());
    }
    // A refusal renders the whole settings page again, which says how many numbers need attention
    let unrecognized = unrecognized_catalog_numbers(&state).await?.len();
    let refused = |reason| Ok(no_store(settings(base, Some(reason), unrecognized)?));
    if form.new != form.confirm {
        return refused("The new passwords did not match.");
    }
    if form.new.is_empty() {
        return refused("The new password must not be empty.");
    }
    match state.archive.verify_password(&form.current).await? {
        PasswordCheck::Correct => {}
        PasswordCheck::Wrong | PasswordCheck::Unclaimed => {
            return refused("Wrong current password.");
        }
    }
    state.archive.change_password(&form.new).await?;
    // Changing the password is how a possibly-compromised session gets locked out, so end every
    // session other than the one that made the change.
    state.sessions.revoke_all_except(&token);
    Ok(Redirect::to("/").into_response())
}

/// Keep pages shown only to logged-in users out of browser caches
fn no_store(page: String) -> Response {
    ([(header::CACHE_CONTROL, "no-store")], Html(page)).into_response()
}

#[derive(Template)]
#[template(path = "settings.html")]
struct SettingsPage {
    base: BaseContext,
    /// What went wrong with the password change, if anything
    error: Option<&'static str>,
    /// How many stored catalog numbers the parser does not recognize
    unrecognized: usize,
}

#[derive(Template)]
#[template(path = "settings_catalog_numbers.html")]
struct CatalogNumbersPage {
    base: BaseContext,
    entries: Vec<CatalogNumberEntry>,
}

fn settings(
    base: BaseContext,
    error: Option<&'static str>,
    unrecognized: usize,
) -> askama::Result<String> {
    SettingsPage {
        base: base.page("Settings", vec![Crumb::home()]),
        error,
        unrecognized,
    }
    .render()
}
