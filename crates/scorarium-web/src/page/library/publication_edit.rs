use askama::Template;
use axum::Extension;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use scorarium_engine::Engine;
use scorarium_engine::asset::AssetSource;
use scorarium_engine::entity::{AnyEntityId, Contributed, Entity};
use scorarium_engine::library::{Library, LibraryId};
use scorarium_engine::link::{HttpUrl, LinkId};
use scorarium_engine::publication::{self, Identifier, Publication, PublicationId};
use scorarium_engine::value::{Classification, NonBlank, Stars, Tag, Year};
use serde::Deserialize;

use crate::Settings;
use crate::auth::{Session, Viewer};
use crate::error::Result;
use crate::forms::FormBody;
use crate::page::library::{entity, view};
use crate::page::{Base, Crumb, form_error};
use crate::proposal::{self, ProposalStates};
use crate::redirect::{Back, BackQuery};
use crate::widget::classification_input::ClassificationInput;
use crate::widget::confirm::Confirm;
use crate::widget::contents::{Contents, ContentsCandidates, ContentsEdits, ContentsForm};
use crate::widget::contributors_input::{
    ContributorCandidates, ContributorEdits, ContributorsForm, ContributorsInput, NewPeople,
};
use crate::widget::edit_actions::{EditActions, EditMode};
use crate::widget::edit_heading::EditHeading;
use crate::widget::header::Header;
use crate::widget::holdings::{HoldingEdits, Holdings, HoldingsForm};
use crate::widget::identifiers_input::{IdentifierCandidates, IdentifiersForm, IdentifiersInput};
use crate::widget::links_input::{LinkCandidates, LinksForm, LinksInput};
use crate::widget::list::Edits;
use crate::widget::picture_input::{PictureCandidates, PictureEdit, PictureForm, PictureInput};
use crate::widget::refusals::Refusals;
use crate::widget::review_queue::EnrichmentCell;
use crate::widget::stars_input::StarsInput;
use crate::widget::tags_input::TagsInput;
use crate::widget::typeahead::Typeahead;
use crate::widget::value_candidates::ValueCandidates;

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
    tag: Vec<String>,
}

struct Posted {
    form: Form,
    picture: PictureForm,
    identifiers: IdentifiersForm,
    links: LinksForm,
    contributors: ContributorsForm,
    holdings: HoldingsForm,
    contents: ContentsForm,
    states: ProposalStates,
}

impl Posted {
    fn read(body: &FormBody) -> Result<Posted> {
        Ok(Posted {
            form: body.read()?,
            picture: body.read()?,
            identifiers: body.read()?,
            links: body.read()?,
            contributors: body.read()?,
            holdings: body.read()?,
            contents: body.read()?,
            states: body.read()?,
        })
    }
}

/// The candidates that the edit page draws
#[derive(Template)]
#[template(path = "api/publication_candidates.html")]
pub(crate) struct Candidates {
    title: ValueCandidates,
    publisher: ValueCandidates,
    year: ValueCandidates,
    identifiers: IdentifierCandidates,
    links: LinkCandidates,
    contributors: ContributorCandidates,
    contents: ContentsCandidates,
    pictures: PictureCandidates,
}

impl Candidates {
    pub async fn of(
        publication: &Publication,
        states: &ProposalStates,
        back: &str,
    ) -> Result<Candidates> {
        let candidates = proposal::candidates(publication.proposals().await?, states);
        Ok(Candidates {
            title: ValueCandidates::new(
                "title",
                proposal::pick(&candidates, |value| match value {
                    publication::Proposal::Title(title) => Some(title.clone()),
                    _ => None,
                }),
            ),
            publisher: ValueCandidates::new(
                "publisher",
                proposal::pick(&candidates, |value| match value {
                    publication::Proposal::Publisher(publisher) => Some(publisher.clone()),
                    _ => None,
                }),
            ),
            year: ValueCandidates::new(
                "year",
                proposal::pick(&candidates, |value| match value {
                    publication::Proposal::Year(year) => Some(year.to_string()),
                    _ => None,
                }),
            ),
            identifiers: IdentifierCandidates::new(proposal::pick(
                &candidates,
                |value| match value {
                    publication::Proposal::Identifier(identifier) => Some(identifier.clone()),
                    _ => None,
                },
            )),
            links: LinkCandidates::new(proposal::pick(&candidates, |value| match value {
                publication::Proposal::Link(url) => Some(url.clone()),
                _ => None,
            })),
            contributors: ContributorCandidates::new(
                proposal::pick(&candidates, |value| match value {
                    publication::Proposal::Contributor { person, role } => {
                        Some((person.clone(), role.clone()))
                    }
                    _ => None,
                }),
                publication.library(),
                back,
            ),
            contents: ContentsCandidates::new(
                proposal::pick(&candidates, |value| match value {
                    publication::Proposal::Contents(work) => Some(work.clone()),
                    _ => None,
                }),
                publication.library(),
                back,
            ),
            pictures: PictureCandidates::new(proposal::pick(&candidates, |value| match value {
                publication::Proposal::Cover(url) => Some(url.clone()),
                _ => None,
            })),
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
    actions: EditActions,
    form: Form,
    title_candidates: ValueCandidates,
    publisher_candidates: ValueCandidates,
    year_candidates: ValueCandidates,
    identifiers: IdentifiersInput,
    tags: TagsInput,
    links: LinksInput,
    contributors: ContributorsInput,
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
    tags: Vec<Tag>,
    links: Edits<LinkId, HttpUrl>,
    contributors: ContributorEdits,
    picture: PictureEdit,
}

async fn parse(
    engine: &Engine,
    library: LibraryId,
    posted: &mut Posted,
    refusals: &mut Refusals,
) -> Result<Option<Parsed>> {
    let contributors = posted.contributors.parse(engine, library, refusals).await?;
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
    let tags = form
        .tag
        .iter()
        .filter(|tag| !tag.trim().is_empty())
        .filter_map(|tag| refusals.check(tag.parse()))
        .collect();
    let links = posted.links.parse(refusals);
    let picture = posted.picture.parse(refusals);
    if !refusals.is_empty() {
        return Ok(None);
    }
    Ok(title.zip(picture).map(|(title, picture)| Parsed {
        title,
        classification,
        year,
        stars,
        identifiers,
        tags,
        links,
        contributors,
        picture,
    }))
}

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
            refusal: None,
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
    let tags = form.tag.iter().filter_map(|tag| tag.parse().ok()).collect();
    let Candidates {
        title: title_candidates,
        publisher: publisher_candidates,
        year: year_candidates,
        identifiers: identifier_candidates,
        links: link_candidates,
        contributors: contributor_candidates,
        contents: contents_candidates,
        pictures,
    } = Candidates::of(publication, &posted.states, &here).await?;
    let page = Edit {
        picture: PictureInput::cover(publication, files_allowed, pictures).resubmitted(
            engine,
            &posted.picture,
            &refusals,
        ),
        base: Base::new(header, publication.title().unwrap_or("Untitled"), parents),
        heading: EditHeading {
            title: publication.title().unwrap_or("Untitled").to_owned(),
            draft,
            enrichment: Some(EnrichmentCell::new(
                AnyEntityId::Publication(publication.id()),
                publication.enrichment().map(|e| e.status()),
            )),
            enrich: Some(format!(
                "/library/{}/publication/{}/enrich{}",
                library.id(),
                publication.id(),
                back.query()
            )),
        },
        actions: EditActions {
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
            reject_remaining: Some(posted.states.rejects_remaining()),
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
        title_candidates,
        publisher_candidates,
        year_candidates,
        identifiers: IdentifiersInput::new(&posted.identifiers, &refusals, identifier_candidates),
        tags: TagsInput {
            library: library.id(),
            tags,
        },
        links: LinksInput::new(
            &posted.links,
            publication.links(),
            &refusals,
            link_candidates,
        ),
        contributors: ContributorsInput::new(
            &posted.contributors,
            publication.contributors(),
            library.id(),
            &here,
            &refusals,
            contributor_candidates,
        ),
        holdings: Holdings::of(
            engine,
            publication,
            &posted.holdings,
            &refusals,
            files_allowed,
        ),
        contents: Contents::of(
            engine,
            publication,
            &posted.contents,
            &here,
            &refusals,
            contents_candidates,
        )
        .await?,
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
        tag: publication.tags().iter().map(ToString::to_string).collect(),
    };
    let posted = Posted {
        form,
        picture: PictureForm::default(),
        identifiers: IdentifiersForm::from(publication.identifiers()),
        links: LinksForm::from(publication.links()),
        contributors: ContributorsForm::from(publication.contributors()),
        holdings: HoldingsForm::from(publication.holdings()),
        contents: ContentsForm::from(publication.works().await?.as_slice()),
        states: ProposalStates::default(),
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
    edits: HoldingEdits,
    contents: ContentsEdits,
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
    for tag in publication.tags().to_vec() {
        if !parsed.tags.contains(&tag) {
            publication.remove_tag(&tag).await?;
        }
    }
    for tag in parsed.tags {
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
    let mut people = NewPeople::default();
    for (pick, role) in parsed.contributors.add {
        people.attach(publication, pick, role).await?;
    }
    for work in contents.add {
        work.add_to(publication, &mut people).await?;
    }
    // Removals run last, because a removal deletes a person who is left with nothing.
    for (person, role) in &parsed.contributors.remove {
        publication.remove_contributor(*person, role).await?;
    }
    for work in contents.remove {
        publication.remove_work(work).await?;
    }
    posted.contributors = ContributorsForm::from(publication.contributors());
    posted.contents = ContentsForm::from(publication.works().await?.as_slice());
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
    for holding in &edits.add {
        publication.add_holding(holding.as_new()).await?;
    }
    for (id, location) in edits.relocate {
        publication
            .set_physical_holding_location(id, Some(&location))
            .await?;
    }
    for id in edits.remove {
        publication.remove_holding(id).await?;
    }
    posted.states.apply(publication).await?;
    Ok(())
}

enum Then {
    Return,
    Commit,
    Enrich,
}

/// Save the form, commit the draft or start enrichment if asked, and answer
///
/// A refusal from either step is shown in the form. A refused save blocks the commit and the
/// enrichment.
async fn submit(
    engine: Engine,
    settings: Settings,
    header: Header,
    (library, publication): (String, String),
    body: FormBody,
    query: BackQuery,
    then: Then,
) -> Result<Response> {
    let mut posted = Posted::read(&body)?;
    let library = view::load(&engine, &library).await?;
    let mut publication: Publication =
        entity::load(&engine, library.id(), &publication, &Viewer::LoggedIn).await?;
    let back = Back::new(query, publication_path(&library, &publication));
    let mut refusals = Refusals::default();
    let parsed = parse(&engine, library.id(), &mut posted, &mut refusals).await?;
    let contents = posted
        .contents
        .parse(&engine, &publication, &mut refusals)
        .await?;
    let edits = posted
        .holdings
        .parse(&engine, publication.holdings(), &mut refusals);
    if refusals.is_empty()
        && let Some(parsed) = parsed
    {
        let saved = match save(&mut publication, &mut posted, parsed, edits, contents).await {
            Ok(()) if matches!(then, Then::Commit) => publication.commit().await,
            saved => saved,
        };
        match saved {
            Ok(()) if matches!(then, Then::Enrich) => {
                publication.enrich().await?;
                let edit = format!(
                    "{}/edit{}",
                    publication_path(&library, &publication),
                    back.query()
                );
                return Ok(Redirect::to(&edit).into_response());
            }
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
    submit(engine, settings, header, path, body, query, Then::Return).await
}

pub async fn enrich(
    _session: Session,
    State(engine): State<Engine>,
    Extension(settings): Extension<Settings>,
    header: Header,
    Path(path): Path<(String, String)>,
    Query(query): Query<BackQuery>,
    body: FormBody,
) -> Result<Response> {
    submit(engine, settings, header, path, body, query, Then::Enrich).await
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
    submit(engine, settings, header, path, body, query, Then::Commit).await
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
