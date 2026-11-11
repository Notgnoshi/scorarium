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
use crate::page::library::entity::{self, parse_link_id};
use crate::page::library::view;
use crate::page::{Base, Crumb, form_error};
use crate::redirect::{BackQuery, same_site_path};
use crate::widget::confirm::{Confirm, count};
use crate::widget::edit_heading::{EditHeading, EditMode};
use crate::widget::header::Header;
use crate::widget::links_input::LinksInput;
use crate::widget::picture_input::PictureInput;
use crate::widget::refusals::Refusals;

#[derive(Deserialize)]
pub(crate) struct Form {
    name: String,
    sort_name: String,
    #[serde(default)]
    add_link: Vec<String>,
    #[serde(default)]
    remove_link: Vec<String>,
    #[serde(default)]
    picture: String,
    remove_picture: Option<String>,
}

struct Parsed {
    name: NonBlank,
    sort_name: NonBlank,
    add_links: Vec<HttpUrl>,
    remove_links: Vec<LinkId>,
}

fn parse(form: &Form, refusals: &mut Refusals) -> Option<Parsed> {
    let name = refusals.parse::<NonBlank>("name", &form.name);
    let sort_name = match form.sort_name.trim() {
        "" => name.as_ref().map(derive_sort_name),
        sort_name => refusals.check(sort_name.parse()),
    };
    let add_links = form
        .add_link
        .iter()
        .filter_map(|url| refusals.check(url.parse()))
        .collect();
    let remove_links = form
        .remove_link
        .iter()
        .filter_map(|id| refusals.check(parse_link_id(id)))
        .collect();
    if !refusals.is_empty() {
        return None;
    }
    Some(Parsed {
        name: name?,
        sort_name: sort_name?,
        add_links,
        remove_links,
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
    back: Option<String>,
    refusals: Refusals,
}

#[allow(clippy::too_many_arguments)]
async fn render(
    engine: &Engine,
    header: Header,
    library: &Library,
    person: &Person,
    form: Form,
    files_allowed: bool,
    back: Option<String>,
    refusals: Refusals,
) -> Result<Response> {
    let parents = vec![
        Crumb::new("Libraries", "/"),
        Crumb::new(library.name(), format!("/library/{}", library.id())),
    ];
    let draft = person.is_draft();
    let page = Edit {
        picture: PictureInput::profile(person, files_allowed).resubmitted(
            engine,
            &form.picture,
            form.remove_picture.is_some(),
        ),
        base: Base::new(header, person.name(), parents),
        heading: EditHeading {
            title: person.name().to_owned(),
            mode: if draft {
                EditMode::Draft
            } else {
                EditMode::Committed
            },
            cancel: back
                .clone()
                .unwrap_or_else(|| format!("/library/{}/person/{}", library.id(), person.id())),
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
                url: format!("/library/{}/person/{}/delete", library.id(), person.id()),
                target: None,
                fields: vec![],
            },
        },
        library: library.id(),
        id: person.id(),
        name: form.name,
        sort_name: form.sort_name,
        links: LinksInput {
            links: person.links().iter().map(Into::into).collect(),
        },
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
        add_link: vec![],
        remove_link: vec![],
        picture: String::new(),
        remove_picture: None,
    };
    render(
        &engine,
        header,
        &library,
        &person,
        form,
        settings.files_allowed(&engine),
        same_site_path(query.back),
        Refusals::default(),
    )
    .await
}

/// Apply the submitted form to `person`
async fn save(person: &mut Person, form: &Form, parsed: Parsed) -> scorarium_engine::Result<()> {
    person.rename(parsed.name, parsed.sort_name).await?;
    // Removals run first so that removing a link and adding it back leaves it present.
    for link in parsed.remove_links {
        person.remove_link(link).await?;
    }
    for url in parsed.add_links {
        person.add_link(url).await?;
    }
    // The picture is saved last, because the engine refuses a file that is not an image only as it
    // stores the file, and by then the name and links that the person typed are already saved.
    if !form.picture.is_empty() {
        let Ok(id) = form.picture.parse();
        person.set_profile(AssetSource::Staged(id)).await?;
    } else if form.remove_picture.is_some() {
        person.clear_profile().await?;
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
    axum_extra::extract::Form(form): axum_extra::extract::Form<Form>,
) -> Result<Response> {
    let back = same_site_path(query.back);
    let library = view::load(&engine, &library).await?;
    let mut person: Person =
        entity::load(&engine, library.id(), &person, &Viewer::LoggedIn).await?;
    let mut refusals = Refusals::default();
    if let Some(parsed) = parse(&form, &mut refusals) {
        match save(&mut person, &form, parsed).await {
            Ok(()) => {
                let destination = back
                    .unwrap_or_else(|| format!("/library/{}/person/{}", library.id(), person.id()));
                return Ok(Redirect::to(&destination).into_response());
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
        form,
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
) -> Result<Redirect> {
    let library = view::load(&engine, &library).await?;
    let person: Person = entity::load(&engine, library.id(), &person, &Viewer::LoggedIn).await?;
    person.delete().await?;
    Ok(Redirect::to(&format!("/library/{}", library.id())))
}
