use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse, Response};
use scorarium_archive::{Contributor, Library};

use super::{AppError, BaseContext, Crumb, OrNotFound, Session};
use crate::AppState;

/// How many sizes the cloud renders a tag in
const SIZES: i64 = 5;

/// One tag in the cloud, sized by how often it is used
struct ShownTag {
    tag: String,
    uses: i64,
    /// 1 through [SIZES], for the class the cloud sizes it with
    size: i64,
}

#[derive(Template)]
#[template(path = "tag_cloud.html")]
struct CloudPage {
    base: BaseContext,
    library: Library,
    tags: Vec<ShownTag>,
}

/// GET /library/{id}/tags
pub async fn cloud(
    _session: Session,
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let library = state.archive.library(id).await?.or_not_found()?;
    let counts = library.tag_counts().await?;
    let most = counts.iter().map(|count| count.uses).max().unwrap_or(1);
    let page = CloudPage {
        base: base.page("Tags", vec![Crumb::home(), Crumb::library(&library)]),
        tags: counts
            .into_iter()
            .map(|count| ShownTag {
                // Every tag used the same number of times makes them all the same size
                size: match most {
                    1 => 1,
                    most => 1 + (count.uses - 1) * (SIZES - 1) / (most - 1),
                },
                tag: count.tag,
                uses: count.uses,
            })
            .collect(),
        library,
    };
    Ok(Html(page.render()?).into_response())
}

/// A publication or a work as one entry of the tag page, so the template has one row type
struct TaggedEntity {
    kind: &'static str,
    href: String,
    title: String,
    contributors: Vec<Contributor>,
    stars: Option<i64>,
    tags: Vec<String>,
}

#[derive(Template)]
#[template(path = "tag.html")]
struct TagPage {
    base: BaseContext,
    library: Library,
    tag: String,
    entities: Vec<TaggedEntity>,
}

/// GET /library/{library_id}/tags/{tag}
pub async fn tagged(
    _session: Session,
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path((library_id, tag)): Path<(i64, String)>,
) -> Result<Response, AppError> {
    let library = state.archive.library(library_id).await?.or_not_found()?;
    let (publications, works) = library.tagged(&tag).await?;
    let mut entities: Vec<TaggedEntity> = publications
        .into_iter()
        .map(|publication| TaggedEntity {
            kind: "publication",
            href: format!(
                "/library/{}/publication/{}",
                publication.library_id, publication.id
            ),
            title: publication.title,
            contributors: publication.contributors,
            stars: publication.stars,
            tags: publication.tags,
        })
        .chain(works.into_iter().map(|work| TaggedEntity {
            kind: "work",
            href: format!("/library/{}/work/{}", work.library_id, work.id),
            title: work.title,
            contributors: work.contributors,
            stars: work.stars,
            tags: work.tags,
        }))
        .collect();
    // A tag nothing carries is not a page, which is also what an invalid slug gets
    if entities.is_empty() {
        return Err(AppError::from(scorarium_archive::NotFound));
    }
    entities.sort_by(|a, b| a.title.cmp(&b.title));
    let page = TagPage {
        base: base.page(
            tag.clone(),
            vec![
                Crumb::home(),
                Crumb::library(&library),
                Crumb::tags(&library),
            ],
        ),
        tag,
        entities,
        library,
    };
    Ok(Html(page.render()?).into_response())
}
