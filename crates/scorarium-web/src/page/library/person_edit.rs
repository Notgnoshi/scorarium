use askama::Template;
use axum::Extension;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use scorarium_engine::Engine;
use scorarium_engine::asset::AssetSource;
use scorarium_engine::entity::Entity;
use scorarium_engine::library::{Library, LibraryId};
use scorarium_engine::link::{HttpUrl, LinkId};
use scorarium_engine::person::{Person, PersonId, derive_sort_name};
use scorarium_engine::value::NonBlank;
use serde::Deserialize;

use crate::Settings;
use crate::auth::{Session, Viewer};
use crate::error::Result;
use crate::forms::FormBody;
use crate::page::library::{entity, view};
use crate::page::{Base, Crumb, form_error};
use crate::redirect::{Back, BackQuery};
use crate::widget::confirm::{Confirm, count};
use crate::widget::edit_heading::{EditHeading, EditMode};
use crate::widget::header::Header;
use crate::widget::links_input::{LinksForm, LinksInput};
use crate::widget::list::Edits;
use crate::widget::picture_input::{PictureEdit, PictureForm, PictureInput};
use crate::widget::refusals::Refusals;

#[derive(Deserialize)]
pub(crate) struct Form {
    name: String,
    sort_name: String,
}

struct Posted {
    form: Form,
    picture: PictureForm,
    links: LinksForm,
}

impl Posted {
    fn read(body: &FormBody) -> Result<Posted> {
        Ok(Posted {
            form: body.read()?,
            picture: body.read()?,
            links: body.read()?,
        })
    }
}

struct Parsed {
    name: NonBlank,
    sort_name: NonBlank,
    links: Edits<LinkId, HttpUrl>,
    picture: PictureEdit,
}

fn parse(posted: &Posted, refusals: &mut Refusals) -> Option<Parsed> {
    let form = &posted.form;
    let name = refusals.parse::<NonBlank>("name", &form.name);
    let sort_name = match form.sort_name.trim() {
        "" => name.as_ref().map(derive_sort_name),
        sort_name => refusals.check(sort_name.parse()),
    };
    let links = posted.links.parse(refusals);
    if !refusals.is_empty() {
        return None;
    }
    Some(Parsed {
        name: name?,
        sort_name: sort_name?,
        links,
        picture: posted.picture.parse(),
    })
}

#[derive(Template)]
#[template(path = "page/library/person_edit.html")]
struct Edit {
    base: Base,
    picture: PictureInput,
    library: LibraryId,
    id: PersonId,
    heading: EditHeading,
    name: String,
    sort_name: String,
    links: LinksInput,
    back: Back,
    refusals: Refusals,
}

#[allow(clippy::too_many_arguments)]
async fn render(
    engine: &Engine,
    header: Header,
    library: &Library,
    person: &Person,
    posted: Posted,
    files_allowed: bool,
    back: Back,
    refusals: Refusals,
) -> Result<Response> {
    let parents = vec![
        Crumb::new("Libraries", "/"),
        Crumb::new(library.name(), format!("/library/{}", library.id())),
    ];
    let draft = person.is_draft();
    let form = posted.form;
    let page = Edit {
        picture: PictureInput::profile(person, files_allowed).resubmitted(engine, &posted.picture),
        base: Base::new(header, person.name(), parents),
        heading: EditHeading {
            title: person.name().to_owned(),
            mode: if draft {
                EditMode::Draft
            } else {
                EditMode::Committed
            },
            cancel: back.path().to_owned(),
            delete: Confirm {
                id: "delete".to_owned(),
                title: if draft {
                    format!("Discard {}?", person.name())
                } else {
                    format!("Delete {}?", person.name())
                },
                message: format!(
                    "They will be removed from {} and {}.",
                    count(person.publications().await?.len(), "publication"),
                    count(person.works().await?.len(), "work")
                ),
                also_deleted: vec![],
                action: if draft {
                    "Discard draft"
                } else {
                    "Delete person"
                },
                url: format!(
                    "/library/{}/person/{}/delete{}",
                    library.id(),
                    person.id(),
                    back.query()
                ),
                target: None,
                fields: vec![],
            },
        },
        library: library.id(),
        id: person.id(),
        name: form.name,
        sort_name: form.sort_name,
        links: LinksInput::new(&posted.links, person.links(), &refusals),
        back,
        refusals,
    };
    let status = if page.refusals.is_empty() {
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    };
    Ok((status, Html(page.render()?)).into_response())
}

pub async fn get(
    _session: Session,
    State(engine): State<Engine>,
    Extension(settings): Extension<Settings>,
    header: Header,
    Path((library, person)): Path<(String, String)>,
    Query(query): Query<BackQuery>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let person: Person = entity::load(&engine, library.id(), &person, &Viewer::LoggedIn).await?;
    let form = Form {
        name: person.name().to_owned(),
        sort_name: person.sort_name().to_owned(),
    };
    let posted = Posted {
        form,
        picture: PictureForm::default(),
        links: LinksForm::from(person.links()),
    };
    let back = Back::new(
        query,
        format!("/library/{}/person/{}", library.id(), person.id()),
    );
    render(
        &engine,
        header,
        &library,
        &person,
        posted,
        settings.files_allowed(&engine),
        back,
        Refusals::default(),
    )
    .await
}

/// Apply the submitted form to `person`
async fn save(
    person: &mut Person,
    posted: &mut Posted,
    parsed: Parsed,
) -> scorarium_engine::Result<()> {
    person.rename(parsed.name, parsed.sort_name).await?;
    // Removals run first so that removing a link and adding it back leaves it present.
    for link in parsed.links.remove {
        person.remove_link(link).await?;
    }
    for url in parsed.links.add {
        person.add_link(url).await?;
    }
    posted.links = LinksForm::from(person.links());
    // The picture is saved last, because the engine refuses a file that is not an image only as it
    // stores the file, and by then the name and links that the person typed are already saved.
    match parsed.picture {
        PictureEdit::Store(id) => person.set_profile(AssetSource::Staged(id)).await?,
        PictureEdit::Clear => person.clear_profile().await?,
        PictureEdit::Keep => {}
    }
    Ok(())
}

/// Save the form and answer
///
/// A refusal is shown in the form.
pub async fn post(
    _session: Session,
    State(engine): State<Engine>,
    Extension(settings): Extension<Settings>,
    header: Header,
    Path((library, person)): Path<(String, String)>,
    Query(query): Query<BackQuery>,
    body: FormBody,
) -> Result<Response> {
    let mut posted = Posted::read(&body)?;
    let library = view::load(&engine, &library).await?;
    let mut person: Person =
        entity::load(&engine, library.id(), &person, &Viewer::LoggedIn).await?;
    let back = Back::new(
        query,
        format!("/library/{}/person/{}", library.id(), person.id()),
    );
    let mut refusals = Refusals::default();
    if let Some(parsed) = parse(&posted, &mut refusals) {
        match save(&mut person, &mut posted, parsed).await {
            Ok(()) => {
                return Ok(Redirect::to(back.path_after_save()).into_response());
            }
            Err(e) => {
                let (_, message) = form_error(e)?;
                refusals.refuse(message);
            }
        }
    }
    render(
        &engine,
        header,
        &library,
        &person,
        posted,
        settings.files_allowed(&engine),
        back,
        refusals,
    )
    .await
}

pub async fn delete(
    _session: Session,
    State(engine): State<Engine>,
    Path((library, person)): Path<(String, String)>,
    Query(query): Query<BackQuery>,
) -> Result<Redirect> {
    let library = view::load(&engine, &library).await?;
    let person: Person = entity::load(&engine, library.id(), &person, &Viewer::LoggedIn).await?;
    let gone = format!("/library/{}/person/{}", library.id(), person.id());
    person.delete().await?;
    let back = Back::new(query, format!("/library/{}", library.id()));
    Ok(Redirect::to(back.path_outside(&gone)))
}
