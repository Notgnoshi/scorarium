use askama::Template;
use axum::Extension;
use axum::extract::{Path, Query, State};
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
use crate::forms::FormBody;
use crate::page::library::{entity, view};
use crate::page::{Base, Crumb, form_error};
use crate::redirect::{Back, BackQuery};
use crate::widget::classification_input::ClassificationInput;
use crate::widget::confirm::Confirm;
use crate::widget::contents::Contents;
use crate::widget::contributors::Contributors;
use crate::widget::edit_heading::{EditHeading, EditMode};
use crate::widget::header::Header;
use crate::widget::holdings::{AddedHolding, Holdings, HoldingsForm};
use crate::widget::identifiers_input::{IdentifiersForm, IdentifiersInput};
use crate::widget::links_input::{LinksForm, LinksInput};
use crate::widget::list::Edits;
use crate::widget::picture_input::{PictureEdit, PictureForm, PictureInput};
use crate::widget::refusals::Refusals;
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
    add_tag: Vec<String>,
    #[serde(default)]
    remove_tag: Vec<String>,
}

struct Posted {
    form: Form,
    picture: PictureForm,
    identifiers: IdentifiersForm,
    links: LinksForm,
    holdings: HoldingsForm,
}

impl Posted {
    fn read(body: &FormBody) -> Result<Posted> {
        Ok(Posted {
            form: body.read()?,
            picture: body.read()?,
            identifiers: body.read()?,
            links: body.read()?,
            holdings: body.read()?,
        })
    }
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
    back: Back,
    refusals: Refusals,
}

struct Parsed {
    title: NonBlank,
    classification: Option<Classification>,
    year: Option<Year>,
    stars: Option<Stars>,
    identifiers: Edits<Identifier, Identifier>,
    add_tags: Vec<Tag>,
    remove_tags: Vec<Tag>,
    links: Edits<LinkId, HttpUrl>,
    picture: PictureEdit,
}

fn parse(posted: &Posted, refusals: &mut Refusals) -> Option<Parsed> {
    let form = &posted.form;
    let title = refusals.parse("title", &form.title);
    let classification = form
        .classification
        .as_deref()
        .and_then(|classification| refusals.check(classification.parse()));
    let year = match form.year.trim() {
        "" => None,
        year => refusals.parse::<Year>("year", year),
    };
    let stars = form
        .stars
        .and_then(|stars| refusals.check(Stars::try_from(stars)));
    let identifiers = posted.identifiers.parse(refusals);
    let add_tags = form
        .add_tag
        .iter()
        .filter_map(|tag| refusals.check(tag.parse()))
        .collect();
    let remove_tags = form
        .remove_tag
        .iter()
        .filter_map(|tag| refusals.check(tag.parse()))
        .collect();
    let links = posted.links.parse(refusals);
    let picture = posted.picture.parse(refusals);
    if !refusals.is_empty() {
        return None;
    }
    Some(Parsed {
        title: title?,
        classification,
        year,
        stars,
        identifiers,
        add_tags,
        remove_tags,
        links,
        picture: picture?,
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

/// Get the path of the page that shows `publication`
fn publication_path(library: &Library, publication: &Publication) -> String {
    format!("/library/{}/publication/{}", library.id(), publication.id())
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
    posted: Posted,
    back: Back,
    refusals: Refusals,
) -> Result<Response> {
    let status = if refusals.is_empty() {
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    };
    let here = format!(
        "{}/edit{}",
        publication_path(library, publication),
        back.query()
    );
    let parents = vec![
        Crumb::new("Libraries", "/"),
        Crumb::new(library.name(), format!("/library/{}", library.id())),
    ];
    let draft = publication.is_draft();
    let form = posted.form;
    let page = Edit {
        picture: PictureInput::cover(publication, files_allowed).resubmitted(
            engine,
            &posted.picture,
            &refusals,
        ),
        base: Base::new(header, publication.title().unwrap_or("Untitled"), parents),
        heading: EditHeading {
            title: publication.title().unwrap_or("Untitled").to_owned(),
            mode: if draft {
                EditMode::Import {
                    finish: format!(
                        "/library/{}/publication/{}/commit{}",
                        library.id(),
                        publication.id(),
                        back.query()
                    ),
                }
            } else {
                EditMode::Committed
            },
            cancel: back.path().to_owned(),
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
                    "/library/{}/publication/{}/delete{}",
                    library.id(),
                    publication.id(),
                    back.query()
                ),
                target: None,
                fields: vec![],
            },
        },
        library: library.id(),
        id: publication.id(),
        form,
        identifiers: IdentifiersInput::new(&posted.identifiers, &refusals),
        tags: TagsInput {
            library: library.id(),
            tags: publication.tags().to_vec(),
        },
        links: LinksInput::new(&posted.links, publication.links(), &refusals),
        contributors: Contributors::new(publication, None, here.clone()),
        holdings: Holdings::of(publication, files_allowed).resubmitted(engine, &posted.holdings),
        contents: Contents::new(publication, &publication.works().await?, here),
        back,
        refusals,
    };
    Ok((status, Html(page.render()?)).into_response())
}

pub async fn get(
    _session: Session,
    State(engine): State<Engine>,
    Extension(settings): Extension<Settings>,
    header: Header,
    Path((library, publication)): Path<(String, String)>,
    Query(query): Query<BackQuery>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let publication: Publication =
        entity::load(&engine, library.id(), &publication, &Viewer::LoggedIn).await?;
    let back = Back::new(query, publication_path(&library, &publication));
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
        add_tag: vec![],
        remove_tag: vec![],
    };
    let posted = Posted {
        form,
        picture: PictureForm::default(),
        identifiers: IdentifiersForm::from(publication.identifiers()),
        links: LinksForm::from(publication.links()),
        holdings: HoldingsForm::default(),
    };
    render(
        &engine,
        header,
        files_allowed,
        &library,
        &publication,
        posted,
        back,
        Refusals::default(),
    )
    .await
}

/// Apply the submitted form to `publication`
async fn save(
    publication: &mut Publication,
    posted: &mut Posted,
    parsed: Parsed,
    added: &[AddedHolding],
    locations: Vec<(HoldingId, String)>,
    removed: Vec<HoldingId>,
) -> scorarium_engine::Result<()> {
    let form = &posted.form;
    publication.set_title(parsed.title).await?;
    publication.set_publisher(Some(&form.publisher)).await?;
    publication.set_year(parsed.year).await?;
    publication.set_stars(parsed.stars).await?;
    publication.set_note(Some(&form.note)).await?;
    // Removals run first so that removing an entry and adding it back leaves it present.
    for identifier in &parsed.identifiers.remove {
        publication.remove_identifier(identifier).await?;
    }
    for identifier in parsed.identifiers.add {
        publication.add_identifier(identifier).await?;
    }
    posted.identifiers = IdentifiersForm::from(publication.identifiers());
    for tag in &parsed.remove_tags {
        publication.remove_tag(tag).await?;
    }
    for tag in parsed.add_tags {
        publication.add_tag(tag).await?;
    }
    for link in parsed.links.remove {
        publication.remove_link(link).await?;
    }
    for url in parsed.links.add {
        publication.add_link(url).await?;
    }
    posted.links = LinksForm::from(publication.links());
    if let Some(classification) = parsed.classification {
        publication.set_classification(classification).await?;
    }
    // The cover is saved after the other Details fields, because the engine refuses a file that is
    // not an image only as it stores the file, and by then the fields that the person typed are
    // already saved. It is saved before the holdings, so that a refused cover leaves them unsaved.
    match parsed.picture {
        PictureEdit::Store(id) => publication.set_cover(AssetSource::Staged(id)).await?,
        PictureEdit::Clear => publication.clear_cover().await?,
        PictureEdit::Keep => {}
    }
    // Additions run before removals so that the publication never passes through having no
    // holdings.
    for holding in added {
        publication.add_holding(holding.as_new()).await?;
    }
    for (id, location) in locations {
        if publication.holdings().iter().any(|h| h.id == id) {
            publication
                .set_physical_holding_location(id, Some(&location))
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
    body: FormBody,
    query: BackQuery,
    commit: bool,
) -> Result<Response> {
    let mut posted = Posted::read(&body)?;
    let library = view::load(&engine, &library).await?;
    let mut publication: Publication =
        entity::load(&engine, library.id(), &publication, &Viewer::LoggedIn).await?;
    let back = Back::new(query, publication_path(&library, &publication));
    let mut refusals = Refusals::default();
    let parsed = parse(&posted, &mut refusals);
    let added = posted.holdings.added();
    let locations = posted.holdings.locations()?;
    let removed = posted.holdings.removed()?;
    refusals.check(check_holdings(&engine, &publication, &added, &removed));
    if refusals.is_empty()
        && let Some(parsed) = parsed
    {
        let saved = match save(
            &mut publication,
            &mut posted,
            parsed,
            &added,
            locations,
            removed,
        )
        .await
        {
            Ok(()) if commit => publication.commit().await,
            saved => saved,
        };
        match saved {
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
        settings.files_allowed(&engine),
        &library,
        &publication,
        posted,
        back,
        refusals,
    )
    .await
}

pub async fn post(
    _session: Session,
    State(engine): State<Engine>,
    Extension(settings): Extension<Settings>,
    header: Header,
    Path(path): Path<(String, String)>,
    Query(query): Query<BackQuery>,
    body: FormBody,
) -> Result<Response> {
    submit(engine, settings, header, path, body, query, false).await
}

pub async fn commit(
    _session: Session,
    State(engine): State<Engine>,
    Extension(settings): Extension<Settings>,
    header: Header,
    Path(path): Path<(String, String)>,
    Query(query): Query<BackQuery>,
    body: FormBody,
) -> Result<Response> {
    submit(engine, settings, header, path, body, query, true).await
}

pub async fn delete(
    _session: Session,
    State(engine): State<Engine>,
    Path((library, publication)): Path<(String, String)>,
    Query(query): Query<BackQuery>,
) -> Result<Redirect> {
    let library = view::load(&engine, &library).await?;
    let publication: Publication =
        entity::load(&engine, library.id(), &publication, &Viewer::LoggedIn).await?;
    let gone = publication_path(&library, &publication);
    publication.delete().await?;
    let back = Back::new(query, format!("/library/{}", library.id()));
    Ok(Redirect::to(back.path_outside(&gone)))
}
