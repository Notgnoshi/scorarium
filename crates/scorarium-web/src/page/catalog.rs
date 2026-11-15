use askama::Template;
use axum::extract::State;
use axum::response::Html;
use scorarium_engine::Engine;
use scorarium_engine::library::LibraryId;
use scorarium_engine::value::title_sort_key;
use scorarium_engine::work::CatalogNumber;
use serde::Deserialize;

use crate::auth::Session;
use crate::error::Result;
use crate::page::library::entity::ContainedWork;
use crate::page::{Base, Crumb};
use crate::sort;
use crate::widget::header::Header;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Column {
    #[default]
    Number,
    Work,
    Contributors,
    Library,
}

impl sort::Column for Column {
    const ALL: &'static [Self] = &[
        Column::Number,
        Column::Work,
        Column::Contributors,
        Column::Library,
    ];

    fn name(self) -> &'static str {
        match self {
            Column::Number => "number",
            Column::Work => "work",
            Column::Contributors => "contributors",
            Column::Library => "library",
        }
    }
}

type Sort = sort::Sort<Column>;

#[derive(Deserialize)]
pub(crate) struct Query {
    sort: Option<Sort>,
}

/// An unrecognized catalog number and the work that carries it
struct Entry {
    number: CatalogNumber,
    library: LibraryId,
    library_name: String,
    work: ContainedWork,
}

impl Entry {
    /// Derive the key that orders the catalog number, ignoring case, punctuation, and whitespace
    fn number_key(&self) -> (String, &str) {
        (self.number.squashed(), self.number.as_str())
    }
}

/// Order entries by the column, the catalog number breaking ties
fn sort_entries(entries: &mut [Entry], sort: &Sort) {
    entries.sort_by(|a, b| {
        let (a_number, b_number) = (a.number_key(), b.number_key());
        let by_column = match sort.column {
            Column::Number => sort::compare(Some(&a_number), Some(&b_number), sort.descending),
            Column::Work => sort::compare(
                Some(title_sort_key(&a.work.title)),
                Some(title_sort_key(&b.work.title)),
                sort.descending,
            ),
            Column::Contributors => sort::compare(
                a.work.primary_sort_name(),
                b.work.primary_sort_name(),
                sort.descending,
            ),
            Column::Library => sort::compare(
                Some(a.library_name.to_lowercase()),
                Some(b.library_name.to_lowercase()),
                sort.descending,
            ),
        };
        by_column.then_with(|| a_number.cmp(&b_number))
    });
}

#[derive(Template)]
#[template(path = "page/catalog.html")]
struct Catalog {
    base: Base,
    entries: Vec<Entry>,
    sort: Sort,
}

pub async fn get(
    _session: Session,
    State(engine): State<Engine>,
    header: Header,
    axum::extract::Query(query): axum::extract::Query<Query>,
) -> Result<Html<String>> {
    let mut entries = Vec::new();
    for library in engine.libraries().await? {
        for work in library.works().await? {
            for number in work.catalog_numbers() {
                if !number.is_recognized() {
                    entries.push(Entry {
                        number: number.clone(),
                        library: library.id(),
                        library_name: library.name().to_owned(),
                        work: ContainedWork::from(&work),
                    });
                }
            }
        }
    }
    let sort = query.sort.unwrap_or_default();
    sort_entries(&mut entries, &sort);

    let page = Catalog {
        base: Base::new(
            header,
            "Unrecognized catalog numbers",
            vec![
                Crumb::new("Libraries", "/"),
                Crumb::new("Settings", "/settings"),
            ],
        ),
        entries,
        sort,
    };
    Ok(Html(page.render()?))
}
