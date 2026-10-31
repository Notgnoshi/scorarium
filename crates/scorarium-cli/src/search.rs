use std::io::Write;

use scorarium_engine::Engine;
use scorarium_engine::entity::{AnyEntity, Entity};
use scorarium_engine::person::Person;
use scorarium_engine::work::Work;

use crate::library::write_table;

pub(crate) async fn run(engine: &Engine, query: &str, out: &mut impl Write) -> eyre::Result<()> {
    let rows: Vec<[String; 6]> = engine
        .search(query, true, 20)
        .await?
        .into_iter()
        .map(|hit| {
            let (kind, id, title) = describe(&hit.entity);
            [
                kind.to_string(),
                hit.library.to_string(),
                hit.library_name.clone(),
                id,
                title,
                context(&hit.entity),
            ]
        })
        .collect();
    write_table(out, &rows)
}

/// Return the kind, id, and label of an entity.
pub(crate) fn describe(entity: &AnyEntity) -> (&'static str, String, String) {
    match entity {
        AnyEntity::Publication(p) => (
            "publication",
            p.id().to_string(),
            p.title().unwrap_or_default().to_string(),
        ),
        AnyEntity::Work(w) => ("work", w.id().to_string(), w.title().to_string()),
        AnyEntity::Person(p) => ("person", p.id().to_string(), p.name().to_string()),
    }
}

pub(crate) fn context(entity: &AnyEntity) -> String {
    match entity {
        AnyEntity::Publication(publication) => {
            let names: Vec<&str> = publication
                .contributors()
                .iter()
                .map(|c| c.name.as_str())
                .collect();
            names.join(", ")
        }
        AnyEntity::Work(work) => work_context(work),
        AnyEntity::Person(person) => person_context(person),
    }
}

/// Describe a work by its catalog number and first contributor.
pub(crate) fn work_context(work: &Work) -> String {
    let number = work.catalog_numbers().first();
    let name = work.contributors().first().map(|c| c.name.as_str());
    match (number, name) {
        (Some(number), Some(name)) => format!("{number} by {name}"),
        (Some(number), None) => number.to_string(),
        (None, Some(name)) => format!("by {name}"),
        (None, None) => String::new(),
    }
}

/// Describe a person by their sort name.
pub(crate) fn person_context(person: &Person) -> String {
    person.sort_name().to_string()
}
