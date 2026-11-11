use askama::Template;
use axum::Extension;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use scorarium_engine::Engine;
use scorarium_engine::asset::AssetSource;
use scorarium_engine::entity::Entity;
use scorarium_engine::holding::HoldingId;
use scorarium_engine::library::{Library, LibraryId};
use scorarium_engine::link::{HttpUrl, LinkId};
use scorarium_engine::publication::{Identifier, Publication, PublicationId};
use scorarium_engine::value::{Classification, NonBlank, Stars, Tag, Year};
use serde::Deserialize;

use crate::Settings;
use crate::auth::{Session, Viewer};
use crate::error::Result;
use crate::forms::Forms;
use crate::page::library::entity::{self, parse_link_id};
use crate::page::library::view;
use crate::page::{Base, Crumb, form_error};
use crate::widget::classification_input::ClassificationInput;
use crate::widget::confirm::Confirm;
use crate::widget::contents::Contents;
use crate::widget::contributors::Contributors;
use crate::widget::edit_heading::{EditHeading, EditMode};
use crate::widget::header::Header;
use crate::widget::holdings::{AddedHolding, Holdings, HoldingsForm};
use crate::widget::identifiers_input::{self, IdentifiersInput};
use crate::widget::links_input::LinksInput;
use crate::widget::picture_input::PictureInput;
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
    #[serde(default)]
    picture: String,
    remove_picture: Option<String>,
}

#[derive(Template)]
#[template(path = "page/library/publication_edit.html")]
struct Edit {
    base: Base,
    picture: PictureInput,
    library: LibraryId,
    id: PublicationId,
    heading: EditHeading,
    form: Form,
    identifiers: IdentifiersInput,
    tags: TagsInput,
    links: LinksInput,
    contributors: Contributors,
    holdings: Holdings,
    contents: Contents,
    error: Option<String>,
}

struct Parsed {
    title: NonBlank,
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
        title: form.title.parse()?,
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

/// Refuse the holdings that would leave a missing file or a publication with no holdings
fn check_holdings(
    engine: &Engine,
    publication: &Publication,
    added: &[AddedHolding],
    removed: &[HoldingId],
) -> scorarium_engine::Result<()> {
    let gone = added
        .iter()
        .any(|holding| matches!(holding, AddedHolding::Digital(id) if engine.staged(id).is_none()));
    if gone {
        return Err(scorarium_engine::Error::InvalidValue(
            "A file is no longer available. Choose it again.".to_owned(),
        ));
    }
    let kept = publication
        .holdings()
        .iter()
        .filter(|holding| !removed.contains(&holding.id))
        .count();
    if kept + added.len() == 0 {
        return Err(scorarium_engine::Error::InvalidValue(
            "A publication needs at least one holding.".to_owned(),
        ));
    }
    Ok(())
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

#[allow(clippy::too_many_arguments)]
async fn render(
    engine: &Engine,
    header: Header,
    files_allowed: bool,
    library: &Library,
    publication: &Publication,
    form: Form,
    holdings: &HoldingsForm,
    status: StatusCode,
    error: Option<String>,
) -> Result<Response> {
    let parents = vec![
        Crumb::new("Libraries", "/"),
        Crumb::new(library.name(), format!("/library/{}", library.id())),
    ];
    let draft = publication.is_draft();
    let page = Edit {
        picture: PictureInput::cover(publication, files_allowed).resubmitted(
            engine,
            &form.picture,
            form.remove_picture.is_some(),
        ),
        base: Base::new(header, publication.title().unwrap_or("Untitled"), parents),
        heading: EditHeading {
            title: publication.title().unwrap_or("Untitled").to_owned(),
            mode: if draft {
                EditMode::Import {
                    finish: format!(
                        "/library/{}/publication/{}/commit",
                        library.id(),
                        publication.id()
                    ),
                }
            } else {
                EditMode::Committed
            },
            cancel: format!("/library/{}/publication/{}", library.id(), publication.id()),
            delete: Confirm {
                id: "delete".to_owned(),
                title: if draft {
                    format!("Discard {}?", publication.title().unwrap_or("Untitled"))
                } else {
                    format!("Delete {}?", publication.title().unwrap_or("Untitled"))
                },
                message: format!(
                    "Deleting {} also deletes:",
                    publication.title().unwrap_or("Untitled")
                ),
                also_deleted: vec![
                    "its digital assets".to_owned(),
                    "orphaned works".to_owned(),
                    "orphaned contributors".to_owned(),
                ],
                action: if draft {
                    "Discard draft"
                } else {
                    "Delete publication"
                },
                url: format!(
                    "/library/{}/publication/{}/delete",
                    library.id(),
                    publication.id()
                ),
                target: None,
                fields: vec![],
            },
        },
        library: library.id(),
        id: publication.id(),
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
        holdings: Holdings::of(publication, files_allowed).resubmitted(engine, holdings),
        contents: Contents::new(publication, &publication.works().await?),
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
        picture: String::new(),
        remove_picture: None,
    };
    render(
        &engine,
        header,
        files_allowed,
        &library,
        &publication,
        form,
        &HoldingsForm::default(),
        StatusCode::OK,
        None,
    )
    .await
}

/// Apply the submitted form to `publication`
async fn save(
    engine: &Engine,
    publication: &mut Publication,
    form: &Form,
    holdings: &HoldingsForm,
) -> scorarium_engine::Result<()> {
    let parsed = parse(form)?;
    let added = holdings.added();
    let locations = holdings.locations()?;
    let removed = holdings.removed()?;
    check_holdings(engine, publication, &added, &removed)?;
    publication.set_title(parsed.title).await?;
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
    // The cover is saved after the other Details fields, because the engine refuses a file that is
    // not an image only as it stores the file, and by then the fields that the person typed are
    // already saved. It is saved before the holdings, so that a refused cover leaves them unsaved.
    if !form.picture.is_empty() {
        let Ok(id) = form.picture.parse();
        publication.set_cover(AssetSource::Staged(id)).await?;
    } else if form.remove_picture.is_some() {
        publication.clear_cover().await?;
    }
    // Additions run before removals so that the publication never passes through having no
    // holdings.
    for holding in &added {
        publication.add_holding(holding.as_new()).await?;
    }
    for (id, location) in locations {
        if publication.holdings().iter().any(|h| h.id == id) {
            publication
                .set_physical_holding_location(id, Some(location))
                .await?;
        }
    }
    for id in removed {
        publication.remove_holding(id).await?;
    }
    Ok(())
}

/// Save the form, commit the draft if asked, and answer
///
/// A refusal from either step is shown in the form. A refused save blocks the commit.
async fn submit(
    engine: Engine,
    settings: Settings,
    header: Header,
    (library, publication): (String, String),
    (form, holdings): (Form, HoldingsForm),
    commit: bool,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let mut publication: Publication =
        entity::load(&engine, library.id(), &publication, &Viewer::LoggedIn).await?;
    let saved = match save(&engine, &mut publication, &form, &holdings).await {
        Ok(()) if commit => publication.commit().await,
        saved => saved,
    };
    match saved {
        Ok(()) => Ok(Redirect::to(&format!(
            "/library/{}/publication/{}",
            library.id(),
            publication.id()
        ))
        .into_response()),
        Err(e) => {
            let (status, error) = form_error(e)?;
            render(
                &engine,
                header,
                settings.files_allowed(&engine),
                &library,
                &publication,
                form,
                &holdings,
                status,
                Some(error),
            )
            .await
        }
    }
}

pub async fn post(
    _session: Session,
    State(engine): State<Engine>,
    Extension(settings): Extension<Settings>,
    header: Header,
    Path(path): Path<(String, String)>,
    Forms(form, holdings): Forms<Form, HoldingsForm>,
) -> Result<Response> {
    submit(engine, settings, header, path, (form, holdings), false).await
}

pub async fn commit(
    _session: Session,
    State(engine): State<Engine>,
    Extension(settings): Extension<Settings>,
    header: Header,
    Path(path): Path<(String, String)>,
    Forms(form, holdings): Forms<Form, HoldingsForm>,
) -> Result<Response> {
    submit(engine, settings, header, path, (form, holdings), true).await
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
