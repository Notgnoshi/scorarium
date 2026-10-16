use std::collections::HashMap;
use std::sync::Arc;

use askama::Template;
use axum::Form;
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::{Html, IntoResponse, Redirect, Response};
use scorarium_engine::archive::{
    Action, AuditEntry, AuditSubject, CatalogNumberEntry, EntityRef, Field, NotFound, PasswordCheck,
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
async fn unrecognized_catalog_numbers(
    state: &AppState,
) -> Result<Vec<CatalogNumberEntry>, AppError> {
    Ok(state
        .archive
        .all_catalog_numbers()
        .await?
        .into_iter()
        .filter(|entry| !entry.number.is_recognized())
        .collect())
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
    // The demo logs anyone in without a password, so it must not let them set one
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

const ENTRIES_PER_PAGE: usize = 100;

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
    // One entry past the page tells whether there is a page after this one
    let mut entries = state
        .archive
        .audit_log(query.before, Some(ENTRIES_PER_PAGE + 1))
        .await?;
    let has_older = entries.len() > ENTRIES_PER_PAGE;
    entries.truncate(ENTRIES_PER_PAGE);
    let next = has_older.then(|| entries[ENTRIES_PER_PAGE - 1].id);
    let hrefs = entity_hrefs(&state, &entries).await?;
    let page = AuditPage {
        base: base.page("Audit log", vec![Crumb::home(), Crumb::settings()]),
        entries: fold_saves(&entries)
            .iter()
            .map(|(entry, fields)| shown_entry(entry, fields, &hrefs))
            .collect(),
        next,
    };
    Ok(no_store(page.render()?))
}

/// One entry as the page shows it
struct ShownEntry {
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

/// Fold audit log events for a bulk edit of one entity into a group
fn fold_saves(entries: &[AuditEntry]) -> Vec<(&AuditEntry, Vec<Field>)> {
    let key = |entry: &AuditEntry| entry.entity.as_ref().map(|e| (e.kind, e.id));
    let mut shown: Vec<(&AuditEntry, Vec<Field>)> = Vec::new();
    for entry in entries {
        if let Some((head, fields)) = shown.last_mut()
            && key(head).is_some()
            && key(head) == key(entry)
            && head.action == entry.action
            && head.source == entry.source
            && head.created_at - entry.created_at <= 1
        {
            if let Some(field) = entry.field
                && !fields.contains(&field)
            {
                fields.push(field);
            }
            continue;
        }
        shown.push((entry, entry.field.into_iter().collect()));
    }
    // Entries are newest first, but the fields read in the order the save wrote them.
    for (_, fields) in &mut shown {
        fields.reverse();
    }
    shown
}

fn shown_entry(
    entry: &AuditEntry,
    fields: &[Field],
    hrefs: &HashMap<(AuditSubject, i64), Option<String>>,
) -> ShownEntry {
    let entity = entry.entity.as_ref();
    let noun = entity
        .map(|entity| entity.kind.as_str())
        .unwrap_or_default();
    // Every wording lives here, so a new action does not compile until it reads as something
    let summary = match entry.action {
        Action::Created => format!("Created {noun}"),
        Action::Updated => format!("Updated {noun}"),
        Action::Deleted => format!("Deleted {noun}"),
        Action::Merged => format!("Merged into {noun}"),
        Action::ImportAccepted => format!("Accepted import of {noun}"),
        Action::PasswordClaimed => "Claimed the password".to_string(),
        Action::PasswordChanged => "Changed the password".to_string(),
    };
    ShownEntry {
        age: age(entry.created_at),
        source: entry.source.as_str().replace('_', " "),
        summary,
        label: entity
            .map(|entity| entity.label.clone())
            .unwrap_or_default(),
        href: entity.and_then(|entity| hrefs.get(&(entity.kind, entity.id)).cloned().flatten()),
        fields: fields
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
) -> Result<HashMap<(AuditSubject, i64), Option<String>>, AppError> {
    let mut hrefs = HashMap::new();
    for entity in entries.iter().filter_map(|entry| entry.entity.as_ref()) {
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
        AuditSubject::Library => Some(format!("/library/{library_id}")),
        AuditSubject::Publication => library
            .publication(id)
            .await?
            .map(|_| format!("/library/{library_id}/publication/{id}")),
        AuditSubject::Work => library
            .work(id)
            .await?
            .map(|_| format!("/library/{library_id}/work/{id}")),
        AuditSubject::Person => library
            .person(id)
            .await?
            .map(|_| format!("/library/{library_id}/person/{id}")),
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

#[cfg(test)]
mod tests {
    use scorarium_engine::archive::Source;

    use super::*;

    fn entry(id: i64, created_at: i64, entity_id: i64, field: Option<Field>) -> AuditEntry {
        AuditEntry {
            id,
            created_at,
            source: Source::User,
            action: Action::Updated,
            entity: Some(EntityRef {
                kind: AuditSubject::Work,
                id: entity_id,
                library_id: Some(1),
                label: "Aria".into(),
            }),
            field,
        }
    }

    /// Adjacent entries on one entity within a second fold into one shown entry listing each
    /// field once; a different entity or an older save is its own shown entry.
    #[test]
    fn a_save_folds_into_one_shown_entry() {
        let entries = vec![
            entry(6, 101, 1, Some(Field::Links)),
            entry(5, 100, 1, Some(Field::Links)),
            entry(4, 100, 1, Some(Field::Title)),
            entry(3, 100, 2, Some(Field::Key)),
            entry(2, 98, 1, Some(Field::Stars)),
            entry(1, 98, 1, None),
        ];
        let shown: Vec<(i64, Vec<Field>)> = fold_saves(&entries)
            .into_iter()
            .map(|(head, fields)| (head.id, fields))
            .collect();
        assert_eq!(
            shown,
            vec![
                (6, vec![Field::Title, Field::Links]),
                (3, vec![Field::Key]),
                (2, vec![Field::Stars]),
            ]
        );
    }

    /// Entries that name no entity, such as password changes, are never folded together.
    #[test]
    fn entries_without_an_entity_stay_separate() {
        let nameless = |id| AuditEntry {
            entity: None,
            action: Action::PasswordChanged,
            ..entry(id, 100, 1, None)
        };
        let entries = vec![nameless(2), nameless(1)];
        assert_eq!(fold_saves(&entries).len(), 2);
    }
}
