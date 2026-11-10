use askama::Template;
use axum::Extension;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use scorarium_engine::Engine;
use scorarium_engine::entity::Entity;
use scorarium_engine::library::{Library, LibraryId};
use scorarium_engine::link::{HttpUrl, LinkId};
use scorarium_engine::person::{Person, PersonId, derive_sort_name};
use serde::Deserialize;

use crate::Settings;
use crate::auth::{Session, Viewer};
use crate::error::Result;
use crate::page::library::entity::{self, parse_link_id};
use crate::page::library::view;
use crate::page::{Base, Crumb, form_error};
use crate::redirect::{BackQuery, same_site_path};
use crate::widget::confirm::{Confirm, count};
use crate::widget::edit_heading::EditHeading;
use crate::widget::header::Header;
use crate::widget::links_input::LinksInput;
use crate::widget::picture_box::PictureBox;

#[derive(Deserialize)]
pub(crate) struct Form {
    name: String,
    sort_name: String,
    #[serde(default)]
    add_link: Vec<String>,
    #[serde(default)]
    remove_link: Vec<String>,
}

struct Parsed {
    add_links: Vec<HttpUrl>,
    remove_links: Vec<LinkId>,
}

fn parse(form: &Form) -> scorarium_engine::Result<Parsed> {
    Ok(Parsed {
        add_links: form
            .add_link
            .iter()
            .map(|url| url.parse())
            .collect::<scorarium_engine::Result<_>>()?,
        remove_links: form
            .remove_link
            .iter()
            .map(|id| parse_link_id(id))
            .collect::<scorarium_engine::Result<_>>()?,
    })
}

#[derive(Template)]
#[template(path = "page/library/person_edit.html")]
struct Edit {
    base: Base,
    picture: PictureBox,
    library: LibraryId,
    id: PersonId,
    heading: EditHeading,
    name: String,
    sort_name: String,
    links: LinksInput,
    back: Option<String>,
    destination: String,
    error: Option<String>,
}

async fn render(
    header: Header,
    library: &Library,
    person: &Person,
    form: Form,
    files_allowed: bool,
    back: Option<String>,
    error: Option<(StatusCode, String)>,
) -> Result<Response> {
    let parents = vec![
        Crumb::new("Libraries", "/"),
        Crumb::new(library.name(), format!("/library/{}", library.id())),
    ];
    let draft = person.is_draft();
    let page = Edit {
        picture: PictureBox::profile(person, files_allowed),
        base: Base::new(header, person.name(), parents),
        heading: EditHeading {
            title: person.name().to_owned(),
            draft,
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
        destination: back
            .clone()
            .unwrap_or_else(|| format!("/library/{}/person/{}", library.id(), person.id())),
        back,
        error: error.as_ref().map(|(_, message)| message.clone()),
    };
    let status = error.map_or(StatusCode::OK, |(status, _)| status);
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
    };
    render(
        header,
        &library,
        &person,
        form,
        settings.files_allowed(&engine),
        same_site_path(query.back),
        None,
    )
    .await
}

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
    let files_allowed = settings.files_allowed(&engine);
    let library = view::load(&engine, &library).await?;
    let mut person: Person =
        entity::load(&engine, library.id(), &person, &Viewer::LoggedIn).await?;
    let parsed = match parse(&form) {
        Ok(parsed) => parsed,
        Err(e) => {
            let error = form_error(e)?;
            return render(
                header,
                &library,
                &person,
                form,
                files_allowed,
                back,
                Some(error),
            )
            .await;
        }
    };
    let sort_name = match form.sort_name.trim() {
        "" => derive_sort_name(&form.name),
        sort_name => sort_name.to_owned(),
    };
    if let Err(e) = person.rename(&form.name, &sort_name).await {
        let error = form_error(e)?;
        return render(
            header,
            &library,
            &person,
            form,
            files_allowed,
            back,
            Some(error),
        )
        .await;
    }
    // Removals run first so that removing a link and adding it back leaves it present.
    for link in parsed.remove_links {
        person.remove_link(link).await?;
    }
    for url in parsed.add_links {
        person.add_link(url).await?;
    }
    let destination =
        back.unwrap_or_else(|| format!("/library/{}/person/{}", library.id(), person.id()));
    Ok(Redirect::to(&destination).into_response())
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
