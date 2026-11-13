use askama::Template;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use scorarium_engine::Engine;
use scorarium_engine::entity::Entity;
use scorarium_engine::publication::Publication;

use crate::auth::{ApiSession, Viewer};
use crate::error::Result;
use crate::forms::FormBody;
use crate::page::library::{entity, view};
use crate::redirect::{Back, CurrentPage};
use crate::widget::contents::{Contents, ContentsForm, SequenceDialog, SequenceForm};
use crate::widget::refusals::Refusals;

async fn load(engine: &Engine, library: &str, id: &str) -> Result<Publication> {
    let library = view::load(engine, library).await?;
    entity::load(engine, library.id(), id, &Viewer::LoggedIn).await
}

pub async fn sequence_dialog(
    _session: ApiSession,
    State(engine): State<Engine>,
    Path((library, id)): Path<(String, String)>,
    body: FormBody,
) -> Result<Response> {
    let publication = load(&engine, &library, &id).await?;
    let contents: ContentsForm = body.read()?;
    let works = publication.works().await?;
    let form = contents.prefill(&engine, &publication, &works).await?;
    let dialog = SequenceDialog::new(&publication, form, Refusals::default());
    Ok(Html(dialog.render()?).into_response())
}

pub async fn sequence(
    _session: ApiSession,
    page: CurrentPage,
    State(engine): State<Engine>,
    Path((library, id)): Path<(String, String)>,
    body: FormBody,
) -> Result<Response> {
    let publication = load(&engine, &library, &id).await?;
    let mut contents: ContentsForm = body.read()?;
    let form: SequenceForm = body.read()?;
    let mut refusals = Refusals::default();
    let Some(sequence) = contents
        .append(&engine, &publication, &form, &mut refusals)
        .await?
    else {
        let dialog = SequenceDialog::new(&publication, form, refusals);
        return Ok((StatusCode::BAD_REQUEST, Html(dialog.render()?)).into_response());
    };
    let edit = format!(
        "/library/{}/publication/{}/edit",
        publication.library(),
        publication.id()
    );
    let back = Back::new(page.query(), edit.clone());
    let here = format!("{edit}{}", back.query());
    let contents = Contents::of(
        &engine,
        &publication,
        &contents,
        &here,
        &Refusals::default(),
    )
    .await?
    .with_sequence(sequence);
    Ok(([("HX-Retarget", "#contents")], Html(contents.render()?)).into_response())
}
