use askama::Template;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use scorarium_engine::Engine;
use scorarium_engine::entity::Entity;
use scorarium_engine::library::{Library, LibraryId};
use scorarium_engine::work::{Work, WorkId};
use serde::Deserialize;

use crate::auth::{Session, Viewer};
use crate::error::Result;
use crate::page::library::{entity, view};
use crate::page::{Base, Crumb, form_error};
use crate::widget::header::Header;
use crate::widget::typeahead::Typeahead;

#[derive(Deserialize)]
pub(crate) struct Form {
    title: String,
    key: String,
    time_signature: String,
    instrumentation: String,
    note: String,
}

#[derive(Template)]
#[template(path = "page/library/work_edit.html")]
struct Edit {
    base: Base,
    library: LibraryId,
    id: WorkId,
    draft: bool,
    form: Form,
    error: Option<String>,
}

impl Edit {
    /// Build a vocabulary field with suggestions from the library
    fn typeahead(&self, field: &str, value: &str) -> Typeahead {
        Typeahead {
            name: field.to_owned(),
            url: format!("/api/library/{}/suggest/{field}", self.library),
            placeholder: String::new(),
            value: value.to_owned(),
            picked_id: None,
            pick_required: None,
            attrs: vec![],
        }
    }
}

fn render(
    header: Header,
    library: &Library,
    work: &Work,
    form: Form,
    status: StatusCode,
    error: Option<String>,
) -> Result<Response> {
    let parents = vec![
        Crumb::new("Libraries", "/"),
        Crumb::new(library.name(), format!("/library/{}", library.id())),
    ];
    let page = Edit {
        base: Base::new(header, work.title(), parents),
        library: library.id(),
        id: work.id(),
        draft: work.is_draft(),
        form,
        error,
    };
    Ok((status, Html(page.render()?)).into_response())
}

pub async fn get(
    _session: Session,
    State(engine): State<Engine>,
    header: Header,
    Path((library, work)): Path<(String, String)>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let work: Work = entity::load(&engine, library.id(), &work, &Viewer::LoggedIn).await?;
    let text = |value: Option<&str>| value.unwrap_or_default().to_owned();
    let form = Form {
        title: work.title().to_owned(),
        key: text(work.key()),
        time_signature: text(work.time_signature()),
        instrumentation: text(work.instrumentation()),
        note: text(work.note()),
    };
    render(header, &library, &work, form, StatusCode::OK, None)
}

pub async fn post(
    _session: Session,
    State(engine): State<Engine>,
    header: Header,
    Path((library, work)): Path<(String, String)>,
    axum::Form(form): axum::Form<Form>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let mut work: Work = entity::load(&engine, library.id(), &work, &Viewer::LoggedIn).await?;
    // The title is the only field here the engine can reject, so setting it first leaves the work
    // untouched when it is.
    if let Err(e) = work.set_title(&form.title).await {
        let (status, error) = form_error(e)?;
        return render(header, &library, &work, form, status, Some(error));
    }
    work.set_key(Some(&form.key)).await?;
    work.set_time_signature(Some(&form.time_signature)).await?;
    work.set_instrumentation(Some(&form.instrumentation))
        .await?;
    work.set_note(Some(&form.note)).await?;
    Ok(Redirect::to(&format!("/library/{}/work/{}", library.id(), work.id())).into_response())
}
