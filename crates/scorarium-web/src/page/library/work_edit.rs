use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use scorarium_engine::Engine;
use scorarium_engine::entity::Entity;
use scorarium_engine::library::{Library, LibraryId};
use scorarium_engine::link::{HttpUrl, LinkId};
use scorarium_engine::value::{Classification, Stars, Tag};
use scorarium_engine::work::{CatalogNumber, Work, WorkId};
use serde::Deserialize;

use crate::auth::{Session, Viewer};
use crate::error::Result;
use crate::page::library::entity::{self, parse_link_id};
use crate::page::library::view;
use crate::page::{Base, Crumb, form_error};
use crate::redirect::{BackQuery, same_site_path};
use crate::widget::catalog_numbers_input::CatalogNumbersInput;
use crate::widget::classification_input::ClassificationInput;
use crate::widget::contributors::Contributors;
use crate::widget::header::Header;
use crate::widget::links_input::LinksInput;
use crate::widget::picture::Picture;
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
    add_catalog_number: Vec<String>,
    #[serde(default)]
    remove_catalog_number: Vec<String>,
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
#[template(path = "page/library/work_edit.html")]
struct Edit {
    base: Base,
    picture: Picture,
    library: LibraryId,
    id: WorkId,
    draft: bool,
    publications: usize,
    form: Form,
    catalog_numbers: CatalogNumbersInput,
    tags: TagsInput,
    links: LinksInput,
    contributors: Contributors,
    back: Option<String>,
    destination: String,
    error: Option<String>,
}

struct Parsed {
    classification: Option<Classification>,
    stars: Option<Stars>,
    add_catalog_numbers: Vec<CatalogNumber>,
    remove_catalog_numbers: Vec<CatalogNumber>,
    add_tags: Vec<Tag>,
    remove_tags: Vec<Tag>,
    add_links: Vec<HttpUrl>,
    remove_links: Vec<LinkId>,
}

fn parse(form: &Form) -> scorarium_engine::Result<Parsed> {
    Ok(Parsed {
        classification: form.classification.as_deref().map(str::parse).transpose()?,
        stars: form.stars.map(Stars::try_from).transpose()?,
        add_catalog_numbers: form
            .add_catalog_number
            .iter()
            .map(|number| number.parse())
            .collect::<scorarium_engine::Result<_>>()?,
        remove_catalog_numbers: form
            .remove_catalog_number
            .iter()
            .map(|number| number.parse())
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
    library: &Library,
    work: &Work,
    form: Form,
    status: StatusCode,
    back: Option<String>,
    error: Option<String>,
) -> Result<Response> {
    let parents = vec![
        Crumb::new("Libraries", "/"),
        Crumb::new(library.name(), format!("/library/{}", library.id())),
    ];
    let page = Edit {
        picture: Picture::from(work),
        base: Base::new(header, work.title(), parents),
        library: library.id(),
        id: work.id(),
        draft: work.is_draft(),
        publications: work.publications().await?.len(),
        form,
        catalog_numbers: CatalogNumbersInput {
            numbers: work.catalog_numbers().to_vec(),
        },
        tags: TagsInput {
            library: library.id(),
            tags: work.tags().to_vec(),
        },
        links: LinksInput {
            links: work.links().iter().map(Into::into).collect(),
        },
        contributors: Contributors::new(work, None),
        destination: back
            .clone()
            .unwrap_or_else(|| format!("/library/{}/work/{}", library.id(), work.id())),
        back,
        error,
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
        add_catalog_number: vec![],
        remove_catalog_number: vec![],
        add_tag: vec![],
        remove_tag: vec![],
        add_link: vec![],
        remove_link: vec![],
    };
    render(
        header,
        &library,
        &work,
        form,
        StatusCode::OK,
        same_site_path(query.back),
        None,
    )
    .await
}

pub async fn post(
    _session: Session,
    State(engine): State<Engine>,
    header: Header,
    Path((library, work)): Path<(String, String)>,
    Query(query): Query<BackQuery>,
    axum_extra::extract::Form(form): axum_extra::extract::Form<Form>,
) -> Result<Response> {
    let back = same_site_path(query.back);
    let library = view::load(&engine, &library).await?;
    let mut work: Work = entity::load(&engine, library.id(), &work, &Viewer::LoggedIn).await?;
    let parsed = match parse(&form) {
        Ok(parsed) => parsed,
        Err(e) => {
            let (status, error) = form_error(e)?;
            return render(header, &library, &work, form, status, back, Some(error)).await;
        }
    };
    // The title is the only field here the engine can reject, so setting it first leaves the work
    // untouched when it is.
    if let Err(e) = work.set_title(&form.title).await {
        let (status, error) = form_error(e)?;
        return render(header, &library, &work, form, status, back, Some(error)).await;
    }
    work.set_key(Some(&form.key)).await?;
    work.set_time_signature(Some(&form.time_signature)).await?;
    work.set_instrumentation(Some(&form.instrumentation))
        .await?;
    work.set_stars(parsed.stars).await?;
    work.set_note(Some(&form.note)).await?;
    // Removals run first so that removing an entry and adding it back leaves it present.
    for number in &parsed.remove_catalog_numbers {
        work.remove_catalog_number(number).await?;
    }
    for number in parsed.add_catalog_numbers {
        work.add_catalog_number(number).await?;
    }
    for tag in &parsed.remove_tags {
        work.remove_tag(tag).await?;
    }
    for tag in parsed.add_tags {
        work.add_tag(tag).await?;
    }
    for link in parsed.remove_links {
        work.remove_link(link).await?;
    }
    for url in parsed.add_links {
        work.add_link(url).await?;
    }
    if let Some(classification) = parsed.classification {
        work.set_classification(classification).await?;
    }
    let destination =
        back.unwrap_or_else(|| format!("/library/{}/work/{}", library.id(), work.id()));
    Ok(Redirect::to(&destination).into_response())
}

pub async fn delete(
    _session: Session,
    State(engine): State<Engine>,
    Path((library, work)): Path<(String, String)>,
) -> Result<Redirect> {
    let library = view::load(&engine, &library).await?;
    let work: Work = entity::load(&engine, library.id(), &work, &Viewer::LoggedIn).await?;
    work.delete().await?;
    Ok(Redirect::to(&format!("/library/{}", library.id())))
}
