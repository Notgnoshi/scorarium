use std::collections::HashSet;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use scorarium_engine::Engine;
use scorarium_engine::contributor::Role;
use scorarium_engine::entity::{Contributed, Entity};
use scorarium_engine::person::Person;
use scorarium_engine::publication::Publication;
use scorarium_engine::work::{CatalogNumber, Work, WorkId};
use serde::Deserialize;

use crate::auth::{ApiSession, Viewer};
use crate::error::Result;
use crate::page::form_error;
use crate::page::library::{entity, view};
use crate::widget::contents::{AddFields, Contents, Marks};

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct AddForm {
    name: String,
    /// The picked person's id, `new` or blank for a person named `name`
    person: String,
    title: String,
    /// The picked work's id, or `new` or blank for a work titled `title`
    work: String,
    catalog_number: String,
    role: String,
}

#[derive(Deserialize)]
pub(crate) struct RemoveForm {
    work: String,
}

impl From<AddForm> for AddFields {
    fn from(form: AddForm) -> AddFields {
        AddFields {
            name: form.name,
            person: form.person,
            title: form.title,
            work: form.work,
            catalog_number: form.catalog_number,
            role: form.role,
        }
    }
}

/// Find the work among `works` that carries `number`, the lowest id breaking ties
pub(crate) fn matching_work(works: &[Work], number: &CatalogNumber) -> Option<WorkId> {
    works
        .iter()
        .filter(|work| work.catalog_numbers().contains(number))
        .map(Work::id)
        .min_by_key(|id| id.get())
}

/// Check whether a picker's value names an existing entity rather than a new or absent one
fn is_id(value: &str) -> bool {
    !value.is_empty() && value != "new"
}

async fn apply_add(
    engine: &Engine,
    publication: &mut Publication,
    form: &AddForm,
) -> scorarium_engine::Result<Marks> {
    let contained: HashSet<WorkId> = publication.works().await?.iter().map(Work::id).collect();
    let mut marks = Marks::default();

    // A picked work is linked as it is, so the inputs describing a new work are ignored.
    if is_id(&form.work) {
        let work: Work =
            entity::in_library(engine, publication.library(), &form.work, "work").await?;
        publication.add_work(work.id()).await?;
        if !contained.contains(&work.id()) {
            marks.linked.insert(work.id());
        }
        return Ok(marks);
    }

    // Everything that can be rejected is checked before the first write, so a rejected add writes
    // nothing.
    let name = form.name.trim();
    let role: Option<Role> = (!name.is_empty() || !form.person.is_empty())
        .then(|| form.role.parse())
        .transpose()?;
    let number = form.catalog_number.trim();
    let number: Option<CatalogNumber> = (!number.is_empty()).then(|| number.parse()).transpose()?;
    if form.title.trim().is_empty() {
        return Err(scorarium_engine::Error::InvalidValue(
            "A work title cannot be blank.".to_owned(),
        ));
    }
    let person: Option<Person> = if is_id(&form.person) {
        Some(entity::in_library(engine, publication.library(), &form.person, "person").await?)
    } else {
        None
    };

    if let (Some(person), Some(number)) = (&person, &number)
        && let Some(id) = matching_work(&person.works().await?, number)
    {
        publication.add_work(id).await?;
        if !contained.contains(&id) {
            marks.linked.insert(id);
        }
        return Ok(marks);
    }

    let mut work = publication.create_work(&form.title).await?;
    if let Some(number) = number {
        work.add_catalog_number(number).await?;
    }
    if let Some(role) = role {
        match person {
            Some(person) => work.add_contributor(person.id(), role).await?,
            None if !name.is_empty() => drop(work.create_contributor(name, role).await?),
            None => {}
        }
    }
    marks.created.insert(work.id());
    Ok(marks)
}

async fn apply_remove(
    engine: &Engine,
    publication: &mut Publication,
    form: &RemoveForm,
) -> scorarium_engine::Result<()> {
    let work: Work = entity::in_library(engine, publication.library(), &form.work, "work").await?;
    publication.remove_work(work.id()).await
}

/// Render the box with the outcome of an action, keeping the submitted add form when it is rejected
async fn render(
    mut publication: Publication,
    outcome: scorarium_engine::Result<Marks>,
    submitted: Option<AddFields>,
) -> Result<Response> {
    let (status, marks, error) = match outcome {
        Ok(marks) => (StatusCode::OK, marks, None),
        Err(e) => {
            let (status, error) = form_error(e)?;
            (status, Marks::default(), Some(error))
        }
    };
    // Reloading picks up works changed from another tab as well as this action's.
    publication.reload().await?;
    let works = publication.works().await?;
    let mut contents = Contents::new(&publication, &works);
    contents.marks = marks;
    if let (Some(_), Some(submitted)) = (&error, submitted) {
        contents.add = submitted;
    }
    contents.error = error;
    Ok((status, Html(askama::Template::render(&contents)?)).into_response())
}

pub async fn add(
    _session: ApiSession,
    State(engine): State<Engine>,
    Path((library, id)): Path<(String, String)>,
    axum::Form(form): axum::Form<AddForm>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let mut publication: Publication =
        entity::load(&engine, library.id(), &id, &Viewer::LoggedIn).await?;
    let outcome = apply_add(&engine, &mut publication, &form).await;
    render(publication, outcome, Some(form.into())).await
}

pub async fn remove(
    _session: ApiSession,
    State(engine): State<Engine>,
    Path((library, id)): Path<(String, String)>,
    axum::Form(form): axum::Form<RemoveForm>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let mut publication: Publication =
        entity::load(&engine, library.id(), &id, &Viewer::LoggedIn).await?;
    let outcome = apply_remove(&engine, &mut publication, &form)
        .await
        .map(|()| Marks::default());
    render(publication, outcome, None).await
}
