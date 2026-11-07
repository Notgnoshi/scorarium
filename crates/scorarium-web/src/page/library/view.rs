use std::cmp::Ordering;

use askama::Template;
use axum::extract::{Path, State};
use axum::http::{Method, Uri};
use axum::response::{Html, IntoResponse, Response};
use scorarium_engine::Engine;
use scorarium_engine::entity::Entity;
use scorarium_engine::library::LibraryId;
use scorarium_engine::publication::PublicationId;
use scorarium_engine::value::{Tag, Year};
use serde::Deserialize;

use crate::auth::{Viewer, login_redirect};
use crate::error::Result;
use crate::page::library::entity::{self, group_contributors};
use crate::page::{Base, Crumb};
use crate::widget::header::Header;
use crate::widget::stars::Stars;

#[derive(Template)]
#[template(path = "page/library/view.html")]
struct Library {
    base: Base,
    logged_in: bool,
    id: LibraryId,
    name: String,
    private: bool,
    publications: Vec<Summary>,
    sort: Sort,
}

struct Summary {
    id: PublicationId,
    title: String,
    contributors: Vec<entity::Contributor>,
    contributor_sort_name: Option<String>,
    year: Option<Year>,
    stars: Option<Stars>,
    tags: Vec<Tag>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Column {
    Title,
    Contributors,
    Year,
    Stars,
}

impl Column {
    fn name(self) -> &'static str {
        match self {
            Column::Title => "title",
            Column::Contributors => "contributors",
            Column::Year => "year",
            Column::Stars => "stars",
        }
    }
}

/// A column and direction, parsed from `[-]column`
#[derive(Clone, Copy, Deserialize)]
#[serde(try_from = "String")]
struct Sort {
    column: Column,
    descending: bool,
}

impl Default for Sort {
    fn default() -> Self {
        Sort {
            column: Column::Title,
            descending: false,
        }
    }
}

impl TryFrom<String> for Sort {
    type Error = String;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        let (descending, name) = match value.strip_prefix('-') {
            Some(name) => (true, name),
            None => (false, value.as_str()),
        };
        let column = [
            Column::Title,
            Column::Contributors,
            Column::Year,
            Column::Stars,
        ]
        .into_iter()
        .find(|column| column.name() == name)
        .ok_or_else(|| format!("unknown sort column {value:?}"))?;
        Ok(Sort { column, descending })
    }
}

impl Sort {
    fn href(&self, column: Column) -> String {
        let descending = self.is(column, false);
        format!(
            "?sort={}{}",
            if descending { "-" } else { "" },
            column.name()
        )
    }

    /// Check whether `column` is active in the given direction
    fn is(&self, column: Column, descending: bool) -> bool {
        self.column == column && self.descending == descending
    }
}

#[derive(Deserialize)]
pub(crate) struct Query {
    sort: Option<Sort>,
}

/// Load the library at `id`, or fail with 404 for an unparseable or unknown id
pub(super) async fn load(engine: &Engine, id: &str) -> Result<scorarium_engine::library::Library> {
    let id: LibraryId = id.parse().map_err(|_| scorarium_engine::Error::NotFound)?;
    Ok(engine
        .library(id)
        .await?
        .ok_or(scorarium_engine::Error::NotFound)?)
}

/// Sort summaries by the column, empties last in either direction, title breaking ties
fn sort_summaries(summaries: &mut [Summary], sort: Sort) {
    // An absent value sorts after any present one, whichever way the present ones are ordered.
    fn compare<T: Ord>(a: Option<T>, b: Option<T>, descending: bool) -> Ordering {
        match (a, b) {
            (Some(a), Some(b)) if descending => b.cmp(&a),
            (Some(a), Some(b)) => a.cmp(&b),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    }

    summaries.sort_by(|a, b| {
        let (a_title, b_title) = (a.title.to_lowercase(), b.title.to_lowercase());
        let by_column = match sort.column {
            Column::Title => compare(Some(&a_title), Some(&b_title), sort.descending),
            Column::Contributors => compare(
                a.contributor_sort_name.as_ref().map(|n| n.to_lowercase()),
                b.contributor_sort_name.as_ref().map(|n| n.to_lowercase()),
                sort.descending,
            ),
            Column::Year => compare(a.year, b.year, sort.descending),
            Column::Stars => compare(a.stars, b.stars, sort.descending),
        };
        by_column.then_with(|| a_title.cmp(&b_title))
    });
}

pub async fn get(
    viewer: Viewer,
    method: Method,
    uri: Uri,
    State(engine): State<Engine>,
    header: Header,
    Path(id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<Query>,
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
        .map(|publication| {
            let contributors = group_contributors(publication.contributors());
            Summary {
                id: publication.id(),
                title: publication.title().unwrap_or_default().to_owned(),
                contributor_sort_name: contributors.first().map(|c| c.sort_name.clone()),
                contributors,
                year: publication.year(),
                stars: publication.stars().map(Into::into),
                tags: publication.tags().to_vec(),
            }
        })
        .collect();
    // The order of a hidden column would reveal its values, so an anonymous viewer cannot sort by it.
    let sort = query
        .sort
        .filter(|sort| sort.column != Column::Stars || viewer.is_logged_in())
        .unwrap_or_default();
    sort_summaries(&mut publications, sort);

    let page = Library {
        base: Base::new(header, library.name(), vec![Crumb::new("Libraries", "/")]),
        logged_in: viewer.is_logged_in(),
        id: library.id(),
        name: library.name().to_owned(),
        private: library.is_private(),
        publications,
        sort,
    };
    Ok(Html(page.render()?).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_parses_direction_and_rejects_unknown_columns() {
        let year = Sort::try_from("-year".to_owned()).unwrap();
        assert!(year.is(Column::Year, true));
        assert!(
            Sort::try_from("title".to_owned())
                .unwrap()
                .is(Column::Title, false)
        );
        assert!(Sort::try_from("bogus".to_owned()).is_err());
        assert_eq!(year.href(Column::Year), "?sort=year");
        assert_eq!(year.href(Column::Title), "?sort=title");
        assert_eq!(Sort::default().href(Column::Title), "?sort=-title");
    }

    #[test]
    fn sort_puts_empty_values_last_in_either_direction() {
        let summary = |title: &str, year: Option<u16>| Summary {
            id: "1".parse().unwrap(),
            title: title.into(),
            contributors: Vec::new(),
            contributor_sort_name: None,
            year: year.map(|y| Year::try_from(y).unwrap()),
            stars: None,
            tags: Vec::new(),
        };
        let titles = |summaries: &[Summary]| {
            summaries
                .iter()
                .map(|s| s.title.clone())
                .collect::<Vec<_>>()
        };
        let mut summaries = vec![
            summary("a", Some(1722)),
            summary("b", None),
            summary("c", Some(1888)),
        ];
        sort_summaries(
            &mut summaries,
            Sort {
                column: Column::Year,
                descending: true,
            },
        );
        assert_eq!(titles(&summaries), ["c", "a", "b"]);
        sort_summaries(
            &mut summaries,
            Sort {
                column: Column::Year,
                descending: false,
            },
        );
        assert_eq!(titles(&summaries), ["a", "c", "b"]);
    }

    #[test]
    fn sort_compares_titles_case_insensitively() {
        let summary = |title: &str| Summary {
            id: "1".parse().unwrap(),
            title: title.into(),
            contributors: Vec::new(),
            contributor_sort_name: None,
            year: None,
            stars: None,
            tags: Vec::new(),
        };
        let mut summaries = vec![summary("Zebra"), summary("aardvark"), summary("Mouse")];
        sort_summaries(&mut summaries, Sort::default());
        let titles: Vec<_> = summaries.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["aardvark", "Mouse", "Zebra"]);
    }
}
