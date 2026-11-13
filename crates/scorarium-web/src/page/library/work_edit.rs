use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use scorarium_engine::Engine;
use scorarium_engine::entity::{Contributed, Entity};
use scorarium_engine::library::{Library, LibraryId};
use scorarium_engine::link::{HttpUrl, LinkId};
use scorarium_engine::value::{Classification, NonBlank, Stars, Tag};
use scorarium_engine::work::{CatalogNumber, Work, WorkId};
use serde::Deserialize;

use crate::auth::{Session, Viewer};
use crate::error::Result;
use crate::forms::FormBody;
use crate::page::library::{entity, view};
use crate::page::{Base, Crumb, form_error};
use crate::redirect::{Back, BackQuery};
use crate::widget::catalog_numbers_input::{CatalogNumbersForm, CatalogNumbersInput};
use crate::widget::classification_input::ClassificationInput;
use crate::widget::confirm::{Confirm, count};
use crate::widget::contributors_input::{ContributorEdits, ContributorsForm, ContributorsInput};
use crate::widget::edit_heading::{EditHeading, EditMode};
use crate::widget::header::Header;
use crate::widget::links_input::{LinksForm, LinksInput};
use crate::widget::list::Edits;
use crate::widget::picture::Picture;
use crate::widget::refusals::Refusals;
use crate::widget::stars_input::StarsInput;
use crate::widget::tags_input::TagsInput;
use crate::widget::typeahead::Typeahead;

#[derive(Deserialize)]
pub(crate) struct Form {
    title: String,
    classification: Option<String>,
    key: String,
    time_signature: String,
    instrumentation: String,
    stars: Option<u8>,
    note: String,
    #[serde(default)]
    tag: Vec<String>,
}

struct Posted {
    form: Form,
    catalog_numbers: CatalogNumbersForm,
    links: LinksForm,
    contributors: ContributorsForm,
}

impl Posted {
    fn read(body: &FormBody) -> Result<Posted> {
        Ok(Posted {
            form: body.read()?,
            catalog_numbers: body.read()?,
            links: body.read()?,
            contributors: body.read()?,
        })
    }
}

#[derive(Template)]
#[template(path = "page/library/work_edit.html")]
struct Edit {
    base: Base,
    picture: Picture,
    library: LibraryId,
    id: WorkId,
    heading: EditHeading,
    form: Form,
    catalog_numbers: CatalogNumbersInput,
    tags: TagsInput,
    links: LinksInput,
    contributors: ContributorsInput,
    back: Back,
    refusals: Refusals,
}

struct Parsed {
    title: NonBlank,
    classification: Option<Classification>,
    stars: Option<Stars>,
    catalog_numbers: Edits<CatalogNumber, CatalogNumber>,
    tags: Vec<Tag>,
    links: Edits<LinkId, HttpUrl>,
    contributors: ContributorEdits,
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
    let stars = form
        .stars
        .and_then(|stars| refusals.check(Stars::try_from(stars)));
    let catalog_numbers = posted.catalog_numbers.parse(refusals);
    let tags = form
        .tag
        .iter()
        .filter(|tag| !tag.trim().is_empty())
        .filter_map(|tag| refusals.check(tag.parse()))
        .collect();
    let links = posted.links.parse(refusals);
    if !refusals.is_empty() {
        return Ok(None);
    }
    Ok(title.map(|title| Parsed {
        title,
        classification,
        stars,
        catalog_numbers,
        tags,
        links,
        contributors,
    }))
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

async fn render(
    header: Header,
    library: &Library,
    work: &Work,
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
        "/library/{}/work/{}/edit{}",
        library.id(),
        work.id(),
        back.query()
    );
    let parents = vec![
        Crumb::new("Libraries", "/"),
        Crumb::new(library.name(), format!("/library/{}", library.id())),
    ];
    let draft = work.is_draft();
    let tags = posted
        .form
        .tag
        .iter()
        .filter_map(|tag| tag.parse().ok())
        .collect();
    let page = Edit {
        picture: Picture::from(work),
        base: Base::new(header, work.title(), parents),
        heading: EditHeading {
            title: work.title().to_owned(),
            mode: if draft {
                EditMode::Draft
            } else {
                EditMode::Committed
            },
            cancel: back.path().to_owned(),
            delete: Confirm {
                id: "delete".to_owned(),
                title: if draft {
                    format!("Discard {}?", work.title())
                } else {
                    format!("Delete {}?", work.title())
                },
                message: format!(
                    "It is removed from {}.",
                    count(work.publications().await?.len(), "publication")
                ),
                also_deleted: vec![],
                action: if draft {
                    "Discard draft"
                } else {
                    "Delete work"
                },
                url: format!(
                    "/library/{}/work/{}/delete{}",
                    library.id(),
                    work.id(),
                    back.query()
                ),
                target: None,
                fields: vec![],
            },
        },
        library: library.id(),
        id: work.id(),
        catalog_numbers: CatalogNumbersInput::new(
            &posted.catalog_numbers,
            work.catalog_numbers(),
            &refusals,
        ),
        form: posted.form,
        tags: TagsInput {
            library: library.id(),
            tags,
        },
        links: LinksInput::new(&posted.links, work.links(), &refusals),
        contributors: ContributorsInput::new(
            &posted.contributors,
            work.contributors(),
            library.id(),
            &here,
            &refusals,
        ),
        back,
        refusals,
    };
    Ok((status, Html(page.render()?)).into_response())
}

pub async fn get(
    _session: Session,
    State(engine): State<Engine>,
    header: Header,
    Path((library, work)): Path<(String, String)>,
    Query(query): Query<BackQuery>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let work: Work = entity::load(&engine, library.id(), &work, &Viewer::LoggedIn).await?;
    let text = |value: Option<&str>| value.unwrap_or_default().to_owned();
    let form = Form {
        title: work.title().to_owned(),
        classification: work.classification().map(|c| c.to_string()),
        key: text(work.key()),
        time_signature: text(work.time_signature()),
        instrumentation: text(work.instrumentation()),
        stars: work.stars().map(|stars| stars.get()),
        note: text(work.note()),
        tag: work.tags().iter().map(ToString::to_string).collect(),
    };
    let posted = Posted {
        form,
        catalog_numbers: CatalogNumbersForm::from(work.catalog_numbers()),
        links: LinksForm::from(work.links()),
        contributors: ContributorsForm::from(work.contributors()),
    };
    let back = Back::new(
        query,
        format!("/library/{}/work/{}", library.id(), work.id()),
    );
    render(header, &library, &work, posted, back, Refusals::default()).await
}

/// Apply the submitted form to `work`
async fn save(
    work: &mut Work,
    posted: &mut Posted,
    parsed: Parsed,
) -> scorarium_engine::Result<()> {
    let form = &posted.form;
    work.set_title(parsed.title).await?;
    work.set_key(Some(&form.key)).await?;
    work.set_time_signature(Some(&form.time_signature)).await?;
    work.set_instrumentation(Some(&form.instrumentation))
        .await?;
    work.set_stars(parsed.stars).await?;
    work.set_note(Some(&form.note)).await?;
    // Removals run first so that removing an entry and adding it back leaves it present.
    for number in &parsed.catalog_numbers.remove {
        work.remove_catalog_number(number).await?;
    }
    for number in parsed.catalog_numbers.add {
        work.add_catalog_number(number).await?;
    }
    posted.catalog_numbers = CatalogNumbersForm::from(work.catalog_numbers());
    for tag in work.tags().to_vec() {
        if !parsed.tags.contains(&tag) {
            work.remove_tag(&tag).await?;
        }
    }
    for tag in parsed.tags {
        work.add_tag(tag).await?;
    }
    for link in parsed.links.remove {
        work.remove_link(link).await?;
    }
    for url in parsed.links.add {
        work.add_link(url).await?;
    }
    posted.links = LinksForm::from(work.links());
    parsed.contributors.apply(work).await?;
    posted.contributors = ContributorsForm::from(work.contributors());
    if let Some(classification) = parsed.classification {
        work.set_classification(classification).await?;
    }
    Ok(())
}

/// Save the form and answer
///
/// A refusal is shown in the form.
pub async fn post(
    _session: Session,
    State(engine): State<Engine>,
    header: Header,
    Path((library, work)): Path<(String, String)>,
    Query(query): Query<BackQuery>,
    body: FormBody,
) -> Result<Response> {
    let mut posted = Posted::read(&body)?;
    let library = view::load(&engine, &library).await?;
    let mut work: Work = entity::load(&engine, library.id(), &work, &Viewer::LoggedIn).await?;
    let back = Back::new(
        query,
        format!("/library/{}/work/{}", library.id(), work.id()),
    );
    let mut refusals = Refusals::default();
    if let Some(parsed) = parse(&engine, library.id(), &mut posted, &mut refusals).await? {
        match save(&mut work, &mut posted, parsed).await {
            Ok(()) => {
                return Ok(Redirect::to(back.path_after_save()).into_response());
            }
            Err(e) => {
                let (_, message) = form_error(e)?;
                refusals.refuse(message);
            }
        }
    }
    render(header, &library, &work, posted, back, refusals).await
}

pub async fn delete(
    _session: Session,
    State(engine): State<Engine>,
    Path((library, work)): Path<(String, String)>,
    Query(query): Query<BackQuery>,
) -> Result<Redirect> {
    let library = view::load(&engine, &library).await?;
    let work: Work = entity::load(&engine, library.id(), &work, &Viewer::LoggedIn).await?;
    let gone = format!("/library/{}/work/{}", library.id(), work.id());
    work.delete().await?;
    let back = Back::new(query, format!("/library/{}", library.id()));
    Ok(Redirect::to(back.path_outside(&gone)))
}
