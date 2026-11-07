use askama::Template;
use axum::extract::{Path, State};
use axum::http::{Method, Uri};
use axum::response::{Html, IntoResponse, Response};
use scorarium_engine::Engine;
use scorarium_engine::contributor::Contributor;
use scorarium_engine::entity::Entity;
use scorarium_engine::library::LibraryId;
use scorarium_engine::value::{Tag, Year};

use crate::auth::{Viewer, login_redirect};
use crate::error::Result;
use crate::page::{Base, Crumb};
use crate::widget::header::Header;
use crate::widget::stars::Stars;

#[derive(Template)]
#[template(path = "page/library.html")]
struct Library {
    base: Base,
    logged_in: bool,
    id: LibraryId,
    name: String,
    private: bool,
    publications: Vec<Summary>,
}

struct Summary {
    title: String,
    contributors: Vec<String>,
    year: Option<Year>,
    stars: Option<Stars>,
    tags: Vec<Tag>,
}

/// Load the library at `id`, or fail with 404 for an unparseable or unknown id
async fn load(engine: &Engine, id: &str) -> Result<scorarium_engine::library::Library> {
    let id: LibraryId = id.parse().map_err(|_| scorarium_engine::Error::NotFound)?;
    Ok(engine
        .library(id)
        .await?
        .ok_or(scorarium_engine::Error::NotFound)?)
}

/// Order contributors for display: authors and composers first, then the rest
fn order_contributors(contributors: &[Contributor]) -> Vec<&Contributor> {
    let mut ordered: Vec<&Contributor> = contributors.iter().collect();
    ordered.sort_by_key(|c| {
        let primary = matches!(c.role.as_str(), "author" | "composer");
        (!primary, c.sort_name.as_str())
    });
    let mut seen = Vec::new();
    ordered.retain(|c| {
        let first = !seen.contains(&c.person);
        seen.push(c.person);
        first
    });
    ordered
}

pub async fn get(
    viewer: Viewer,
    method: Method,
    uri: Uri,
    State(engine): State<Engine>,
    header: Header,
    Path(id): Path<String>,
) -> Result<Response> {
    let library = load(&engine, &id).await?;
    if library.is_private() && !viewer.is_logged_in() {
        return Ok(login_redirect(&method, &uri).into_response());
    }

    let mut publications: Vec<Summary> = library
        .publications()
        .await?
        .iter()
        .filter(|publication| !publication.is_draft())
        .map(|publication| Summary {
            title: publication.title().unwrap_or_default().to_owned(),
            contributors: order_contributors(publication.contributors())
                .into_iter()
                .map(|c| c.name.clone())
                .collect(),
            year: publication.year(),
            stars: publication.stars().map(Into::into),
            tags: publication.tags().to_vec(),
        })
        .collect();
    publications.sort_by(|a, b| a.title.cmp(&b.title));

    let page = Library {
        base: Base::new(header, library.name(), vec![Crumb::new("Libraries", "/")]),
        logged_in: viewer.is_logged_in(),
        id: library.id(),
        name: library.name().to_owned(),
        private: library.is_private(),
        publications,
    };
    Ok(Html(page.render()?).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_contributors_puts_primary_roles_first_and_lists_a_person_once() {
        let contributor = |id: &str, name: &str, sort_name: &str, role: &str| Contributor {
            person: id.parse().unwrap(),
            name: name.into(),
            sort_name: sort_name.into(),
            role: role.parse().unwrap(),
        };
        let satie_editor = contributor("1", "Erik Satie", "Satie, Erik", "editor");
        let satie_composer = contributor("1", "Erik Satie", "Satie, Erik", "composer");
        let debussy = contributor("2", "Claude Debussy", "Debussy, Claude", "author");
        let all = [satie_editor, satie_composer.clone(), debussy.clone()];
        let ordered = order_contributors(&all);
        assert_eq!(ordered, [&debussy, &satie_composer]);
    }
}
