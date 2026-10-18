use std::io::Write;

use scorarium_engine::archive::{Archive, Entity};

use crate::library::write_table;
use crate::person;

pub(crate) async fn run(archive: &Archive, query: &str, out: &mut impl Write) -> eyre::Result<()> {
    let rows: Vec<[String; 6]> = archive
        .search(query, false)
        .await?
        .into_iter()
        .map(|hit| {
            let (kind, id, title) = match &hit.entity {
                Entity::Publication(p) => ("publication", p.id, p.title.clone()),
                Entity::Work(w) => ("work", w.id, w.title.clone()),
                Entity::Person(p) => ("person", p.id, p.name.clone()),
            };
            [
                kind.to_string(),
                hit.library_id.to_string(),
                hit.library_name.clone(),
                id.to_string(),
                title,
                context(&hit.entity),
            ]
        })
        .collect();
    write_table(out, &rows)
}

pub(crate) fn context(entity: &Entity) -> String {
    match entity {
        Entity::Publication(publication) => publication.people.join(", "),
        Entity::Work(work) => {
            let number = work.numbers.first();
            let name = work.contributor.as_ref().map(|c| c.name.as_str());
            match (number, name) {
                (Some(number), Some(name)) => format!("{number} by {name}"),
                (Some(number), None) => number.clone(),
                (None, Some(name)) => format!("by {name}"),
                (None, None) => String::new(),
            }
        }
        Entity::Person(person) => person::disambiguator(person),
    }
}
