use std::str::FromStr;

use askama::Template;
use axum::extract::{Path, State};
use axum::http::{Method, Uri};
use axum::response::{Html, IntoResponse, Response};
use scorarium_engine::Engine;
use scorarium_engine::contributor::Role;
use scorarium_engine::library::LibraryId;
use scorarium_engine::person::PersonSummary;
use serde::Deserialize;

use crate::auth::{Viewer, login_redirect};
use crate::error::Result;
use crate::page::library::entity::plural_role_label;
use crate::page::library::view;
use crate::page::{Base, Crumb};
use crate::sort;
use crate::widget::header::Header;
use crate::widget::library_nav::{Active, LibraryNav};

#[derive(Template)]
#[template(path = "page/library/people.html")]
struct People {
    base: Base,
    library: LibraryId,
    nav: LibraryNav,
    people: Vec<PersonSummary>,
    sort: Sort,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Column {
    #[default]
    Name,
    Publications,
    Works,
}

impl sort::Column for Column {
    const ALL: &'static [Self] = &[Column::Name, Column::Publications, Column::Works];

    fn name(self) -> &'static str {
        match self {
            Column::Name => "name",
            Column::Publications => "publications",
            Column::Works => "works",
        }
    }

    fn descending_first(self) -> bool {
        self != Column::Name
    }
}

type Sort = sort::Sort<Column>;

#[derive(Deserialize)]
pub(crate) struct Query {
    sort: Option<Sort>,
}

#[derive(Deserialize)]
pub(crate) struct Params {
    id: String,
    role: Option<String>,
}

fn sort_people(people: &mut [PersonSummary], sort: &Sort) {
    people.sort_by(|a, b| {
        let by_name =
            || (a.sort_name.to_lowercase(), &a.name).cmp(&(b.sort_name.to_lowercase(), &b.name));
        let by_column = match sort.column {
            Column::Name => by_name(),
            Column::Publications => a.publications.cmp(&b.publications),
            Column::Works => a.works.cmp(&b.works),
        };
        let by_column = if sort.descending {
            by_column.reverse()
        } else {
            by_column
        };
        by_column.then_with(by_name)
    });
}

pub async fn get(
    viewer: Viewer,
    method: Method,
    uri: Uri,
    State(engine): State<Engine>,
    header: Header,
    Path(params): Path<Params>,
    axum::extract::Query(query): axum::extract::Query<Query>,
) -> Result<Response> {
    let library = view::load(&engine, &params.id).await?;
    if library.is_private() && !viewer.is_logged_in() {
        return Ok(login_redirect(&method, &uri).into_response());
    }
    let role = params
        .role
        .as_deref()
        .map(Role::from_str)
        .transpose()
        .map_err(|_| scorarium_engine::Error::NotFound)?;
    let mut people = library.people_summary(role.as_ref(), false).await?;
    let sort = query.sort.unwrap_or_default();
    sort_people(&mut people, &sort);

    let library_href = format!("/library/{}", library.id());
    let mut parents = vec![
        Crumb::new("Libraries", "/"),
        Crumb::new(library.name(), &library_href),
    ];
    let heading = match &role {
        Some(role) => {
            parents.push(Crumb::new("People", format!("{library_href}/people")));
            plural_role_label(role)
        }
        None => "People".to_owned(),
    };
    let active = match &role {
        Some(role) => Active::Role(role.clone()),
        None => Active::People,
    };
    let page = People {
        base: Base::new(header, heading, parents),
        library: library.id(),
        nav: LibraryNav::load(&library, viewer.is_logged_in(), active).await?,
        people,
        sort,
    };
    Ok(Html(page.render()?).into_response())
}
