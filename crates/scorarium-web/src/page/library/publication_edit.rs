use askama::Template;
use axum::Extension;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use scorarium_engine::Engine;
use scorarium_engine::entity::Entity;
use scorarium_engine::library::{Library, LibraryId};
use scorarium_engine::link::{HttpUrl, LinkId};
use scorarium_engine::publication::{Identifier, Publication, PublicationId};
use scorarium_engine::value::{Classification, Stars, Tag, Year};
use serde::Deserialize;

use crate::Settings;
use crate::auth::{Session, Viewer};
use crate::error::Result;
use crate::page::library::entity::{self, parse_link_id};
use crate::page::library::view;
use crate::page::{Base, Crumb, form_error};
use crate::widget::classification_input::ClassificationInput;
use crate::widget::confirm::Confirm;
use crate::widget::contents::Contents;
use crate::widget::contributors::Contributors;
use crate::widget::header::Header;
use crate::widget::identifiers_input::{self, IdentifiersInput};
use crate::widget::links_input::LinksInput;
use crate::widget::picture_box::PictureBox;
use crate::widget::stars_input::StarsInput;
use crate::widget::tags_input::TagsInput;
use crate::widget::typeahead::Typeahead;

#[derive(Deserialize)]
pub(crate) struct Form {
    title: String,
    classification: Option<String>,
    publisher: String,
    /// Blank for none; a string because a cleared number input posts an empty value
    year: String,
    stars: Option<u8>,
    note: String,
    #[serde(default)]
    add_identifier: Vec<String>,
    #[serde(default)]
    remove_identifier: Vec<String>,
    #[serde(default)]
    add_tag: Vec<String>,
    #[serde(default)]
    remove_tag: Vec<String>,
    #[serde(default)]
    add_link: Vec<String>,
    #[serde(default)]
    remove_link: Vec<String>,
}

#[derive(Template)]
#[template(path = "page/library/publication_edit.html")]
struct Edit {
    base: Base,
    picture: PictureBox,
    library: LibraryId,
    id: PublicationId,
    draft: bool,
    form: Form,
    identifiers: IdentifiersInput,
    tags: TagsInput,
    links: LinksInput,
    contributors: Contributors,
    contents: Contents,
    delete: Confirm,
    destination: String,
    error: Option<String>,
}

struct Parsed {
    classification: Option<Classification>,
    year: Option<Year>,
    stars: Option<Stars>,
    add_identifiers: Vec<Identifier>,
    remove_identifiers: Vec<Identifier>,
    add_tags: Vec<Tag>,
    remove_tags: Vec<Tag>,
    add_links: Vec<HttpUrl>,
    remove_links: Vec<LinkId>,
}

fn parse(form: &Form) -> scorarium_engine::Result<Parsed> {
    let year = form.year.trim();
    Ok(Parsed {
        classification: form.classification.as_deref().map(str::parse).transpose()?,
        year: (!year.is_empty()).then(|| year.parse()).transpose()?,
        stars: form.stars.map(Stars::try_from).transpose()?,
        add_identifiers: form
            .add_identifier
            .iter()
            .map(|identifier| identifiers_input::parse(identifier))
            .collect::<scorarium_engine::Result<_>>()?,
        remove_identifiers: form
            .remove_identifier
            .iter()
            .map(|identifier| identifiers_input::parse(identifier))
            .collect::<scorarium_engine::Result<_>>()?,
        add_tags: form
            .add_tag
            .iter()
            .map(|tag| tag.parse())
            .collect::<scorarium_engine::Result<_>>()?,
        remove_tags: form
            .remove_tag
            .iter()
            .map(|tag| tag.parse())
            .collect::<scorarium_engine::Result<_>>()?,
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

impl Edit {
    fn classification(&self) -> ClassificationInput {
        ClassificationInput {
            value: self
                .form
                .classification
                .as_deref()
                .and_then(|c| c.parse().ok()),
        }
    }

    fn stars(&self) -> StarsInput {
        StarsInput {
            value: self.form.stars,
        }
    }

    /// Build a vocabulary field with suggestions from the library
    fn typeahead(&self, field: &str, value: &str) -> Typeahead {
        Typeahead {
            id: field.to_owned(),
            name: field.to_owned(),
            url: format!("/api/library/{}/suggest/{field}", self.library),
            placeholder: String::new(),
            value: value.to_owned(),
            picked: None,
            pick_required: None,
            attrs: vec![],
        }
    }
}

async fn render(
    header: Header,
    files_allowed: bool,
    library: &Library,
    publication: &Publication,
    form: Form,
    status: StatusCode,
    error: Option<String>,
) -> Result<Response> {
    let parents = vec![
        Crumb::new("Libraries", "/"),
        Crumb::new(library.name(), format!("/library/{}", library.id())),
    ];
    let page = Edit {
        picture: PictureBox::cover(publication, files_allowed),
        base: Base::new(header, publication.title().unwrap_or("Untitled"), parents),
        library: library.id(),
        id: publication.id(),
        draft: publication.is_draft(),
        form,
        identifiers: IdentifiersInput {
            identifiers: publication.identifiers().to_vec(),
        },
        tags: TagsInput {
            library: library.id(),
            tags: publication.tags().to_vec(),
        },
        links: LinksInput {
            links: publication.links().iter().map(Into::into).collect(),
        },
        contributors: Contributors::new(publication, None),
        contents: Contents::new(publication, &publication.works().await?),
        delete: Confirm {
            id: "delete".to_owned(),
            title: format!("Delete {}?", publication.title().unwrap_or("Untitled")),
            message: format!(
                "Deleting {} also deletes:",
                publication.title().unwrap_or("Untitled")
            ),
            also_deleted: vec![
                "its digital assets".to_owned(),
                "orphaned works".to_owned(),
                "orphaned contributors".to_owned(),
            ],
            action: "Delete publication",
            url: format!(
                "/library/{}/publication/{}/delete",
                library.id(),
                publication.id()
            ),
        },
        destination: format!("/library/{}/publication/{}", library.id(), publication.id()),
        error,
    };
    Ok((status, Html(page.render()?)).into_response())
}

pub async fn get(
    _session: Session,
    State(engine): State<Engine>,
    Extension(settings): Extension<Settings>,
    header: Header,
    Path((library, publication)): Path<(String, String)>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let publication: Publication =
        entity::load(&engine, library.id(), &publication, &Viewer::LoggedIn).await?;
    let files_allowed = settings.files_allowed(&engine);
    let text = |value: Option<&str>| value.unwrap_or_default().to_owned();
    let form = Form {
        title: text(publication.title()),
        classification: publication.classification().map(|c| c.to_string()),
        publisher: text(publication.publisher()),
        year: publication
            .year()
            .map(|year| year.to_string())
            .unwrap_or_default(),
        stars: publication.stars().map(|stars| stars.get()),
        note: text(publication.note()),
        add_identifier: vec![],
        remove_identifier: vec![],
        add_tag: vec![],
        remove_tag: vec![],
        add_link: vec![],
        remove_link: vec![],
    };
    render(
        header,
        files_allowed,
        &library,
        &publication,
        form,
        StatusCode::OK,
        None,
    )
    .await
}

pub async fn post(
    _session: Session,
    State(engine): State<Engine>,
    Extension(settings): Extension<Settings>,
    header: Header,
    Path((library, publication)): Path<(String, String)>,
    axum_extra::extract::Form(form): axum_extra::extract::Form<Form>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let mut publication: Publication =
        entity::load(&engine, library.id(), &publication, &Viewer::LoggedIn).await?;
    let files_allowed = settings.files_allowed(&engine);
    let parsed = match parse(&form) {
        Ok(parsed) => parsed,
        Err(e) => {
            let (status, error) = form_error(e)?;
            return render(
                header,
                files_allowed,
                &library,
                &publication,
                form,
                status,
                Some(error),
            )
            .await;
        }
    };
    // The title is the only field here the engine can reject, so setting it first leaves the
    // publication untouched when it is.
    if let Err(e) = publication.set_title(&form.title).await {
        let (status, error) = form_error(e)?;
        return render(
            header,
            files_allowed,
            &library,
            &publication,
            form,
            status,
            Some(error),
        )
        .await;
    }
    publication.set_publisher(Some(&form.publisher)).await?;
    publication.set_year(parsed.year).await?;
    publication.set_stars(parsed.stars).await?;
    publication.set_note(Some(&form.note)).await?;
    // Removals run first so that removing an entry and adding it back leaves it present.
    for identifier in &parsed.remove_identifiers {
        publication.remove_identifier(identifier).await?;
    }
    for identifier in parsed.add_identifiers {
        publication.add_identifier(identifier).await?;
    }
    for tag in &parsed.remove_tags {
        publication.remove_tag(tag).await?;
    }
    for tag in parsed.add_tags {
        publication.add_tag(tag).await?;
    }
    for link in parsed.remove_links {
        publication.remove_link(link).await?;
    }
    for url in parsed.add_links {
        publication.add_link(url).await?;
    }
    if let Some(classification) = parsed.classification {
        publication.set_classification(classification).await?;
    }
    Ok(Redirect::to(&format!(
        "/library/{}/publication/{}",
        library.id(),
        publication.id()
    ))
    .into_response())
}

pub async fn delete(
    _session: Session,
    State(engine): State<Engine>,
    Path((library, publication)): Path<(String, String)>,
) -> Result<Redirect> {
    let library = view::load(&engine, &library).await?;
    let publication: Publication =
        entity::load(&engine, library.id(), &publication, &Viewer::LoggedIn).await?;
    publication.delete().await?;
    Ok(Redirect::to(&format!("/library/{}", library.id())))
}
