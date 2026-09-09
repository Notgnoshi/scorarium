use std::collections::HashMap;
use std::sync::Arc;

use askama::Template;
use axum::Form;
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::{Html, IntoResponse, Redirect, Response};
use scorarium_archive::{
    Action, AuditEntry, CatalogNumberEntry, EntityKind, EntityRef, NotFound, PasswordCheck,
};
use serde::Deserialize;

use super::{AppError, BaseContext, Crumb, Session, age};
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

#[derive(Deserialize)]
pub struct AuditQuery {
    /// The cursor the previous page handed back; absent on the first page
    before: Option<i64>,
}

/// GET /settings/audit
pub async fn audit_page(
    _session: Session,
    State(state): State<Arc<AppState>>,
    Query(query): Query<AuditQuery>,
    base: BaseContext,
) -> Result<Response, AppError> {
    let (entries, next) = state.archive.audit_page(query.before).await?;
    let hrefs = entity_hrefs(&state, &entries).await?;
    let page = AuditPage {
        base: base.page("Audit log", vec![Crumb::home(), Crumb::settings()]),
        entries: entries
            .iter()
            .map(|entry| shown_entry(entry, &hrefs))
            .collect(),
        next,
    };
    Ok(no_store(page.render()?))
}

/// One entry as the page shows it
struct ShownEntry {
    heads_a_group: bool,
    age: String,
    source: String,
    /// "Created publication", "Deleted work", "Changed the password"
    summary: String,
    label: String,
    /// None when the entity is gone, or when the event names no entity
    href: Option<String>,
    /// "title, publisher", empty when the action is not an update
    fields: String,
}

fn shown_entry(
    entry: &AuditEntry,
    hrefs: &HashMap<(EntityKind, i64), Option<String>>,
) -> ShownEntry {
    let entity = entry.event.entity.as_ref();
    let noun = entity
        .map(|entity| entity.kind.as_str())
        .unwrap_or_default();
    // Every wording lives here, so a new action does not compile until it reads as something
    let summary = match entry.event.action {
        Action::Created => format!("Created {noun}"),
        Action::Updated => format!("Updated {noun}"),
        Action::Deleted => format!("Deleted {noun}"),
        Action::Merged => format!("Merged into {noun}"),
        Action::Renamed => format!("Renamed {noun} to"),
        Action::ImportStarted => "Started import".to_string(),
        Action::ImportAccepted => format!("Accepted import as {noun}"),
        Action::ImportDiscarded => "Discarded import".to_string(),
        Action::PasswordClaimed => "Claimed the password".to_string(),
        Action::PasswordChanged => "Changed the password".to_string(),
    };
    ShownEntry {
        heads_a_group: entry.group_id.is_none(),
        age: age(entry.created_at),
        source: entry.source.as_str().replace('_', " "),
        summary,
        label: entity
            .map(|entity| entity.label.clone())
            .unwrap_or_default(),
        href: entity.and_then(|entity| hrefs.get(&(entity.kind, entity.id)).cloned().flatten()),
        fields: entry
            .event
            .fields
            .iter()
            .map(|field| field.as_str().replace('_', " "))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// Where each entity on the page links, or None for one that no longer exists.
///
/// Looked up once per entity rather than once per entry, since an entity can appear many times
/// on one page.
async fn entity_hrefs(
    state: &AppState,
    entries: &[AuditEntry],
) -> Result<HashMap<(EntityKind, i64), Option<String>>, AppError> {
    let mut hrefs = HashMap::new();
    for entity in entries
        .iter()
        .filter_map(|entry| entry.event.entity.as_ref())
    {
        let key = (entity.kind, entity.id);
        if hrefs.contains_key(&key) {
            continue;
        }
        hrefs.insert(key, entity_href(state, entity).await?);
    }
    Ok(hrefs)
}

async fn entity_href(state: &AppState, entity: &EntityRef) -> Result<Option<String>, AppError> {
    let Some(library_id) = entity.library_id else {
        return Ok(None);
    };
    let Some(library) = state.archive.library(library_id).await? else {
        return Ok(None);
    };
    let id = entity.id;
    Ok(match entity.kind {
        EntityKind::Library => Some(format!("/library/{library_id}")),
        EntityKind::Publication => library
            .publication(id)
            .await?
            .map(|_| format!("/library/{library_id}/publication/{id}")),
        EntityKind::Work => library
            .work(id)
            .await?
            .map(|_| format!("/library/{library_id}/work/{id}")),
        EntityKind::Person => library
            .person(id)
            .await?
            .map(|_| format!("/library/{library_id}/person/{id}")),
        EntityKind::Import => library
            .pending_import(id)
            .await?
            .map(|_| format!("/library/{library_id}/import/{id}")),
    })
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

#[derive(Template)]
#[template(path = "settings_audit.html")]
struct AuditPage {
    base: BaseContext,
    entries: Vec<ShownEntry>,
    /// The cursor for the next page, None on the last one
    next: Option<i64>,
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
