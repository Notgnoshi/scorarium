use std::io::Write;

use scorarium_engine::archive::{Library, Publication, Tag};

use crate::library::write_table;

/// List a library's publications, keeping only those carrying every given tag.
pub(crate) async fn list(
    library: &Library,
    tags: &[Tag],
    out: &mut impl Write,
) -> eyre::Result<()> {
    let rows: Vec<[String; 5]> = library
        .publications()
        .await?
        .into_iter()
        .filter(|publication| {
            tags.iter()
                .all(|tag| publication.tags.iter().any(|t| t == tag.as_str()))
        })
        .map(|publication| {
            let contributors: Vec<&str> = publication
                .contributors
                .iter()
                .map(|contributor| contributor.name.as_str())
                .collect();
            let holdings: Vec<&str> = publication
                .holdings
                .iter()
                .map(|holding| holding.kind.as_str())
                .collect();
            [
                publication.id.to_string(),
                publication.title,
                contributors.join(", "),
                publication
                    .year
                    .map(|year| year.to_string())
                    .unwrap_or_default(),
                holdings.join(", "),
            ]
        })
        .collect();
    write_table(out, &rows)
}

pub(crate) async fn show(publication: Publication, out: &mut impl Write) -> eyre::Result<()> {
    let row = |label: &str, id: String, value: String| [label.to_string(), id, value];
    let mut rows = vec![
        row("id", publication.id.to_string(), String::new()),
        row("title", String::new(), publication.title.clone()),
    ];
    let optional = [
        ("publisher", publication.publisher.clone()),
        ("year", publication.year.map(|year| year.to_string())),
        ("stars", publication.stars.map(|stars| stars.to_string())),
        ("note", publication.note.clone()),
    ];
    for (label, value) in optional {
        if let Some(value) = value {
            rows.push(row(label, String::new(), value));
        }
    }
    if !publication.tags.is_empty() {
        rows.push(row("tags", String::new(), publication.tags.join(", ")));
    }
    for identifier in &publication.identifiers {
        let value = format!("{} {}", identifier.kind.as_str(), identifier.value);
        rows.push(row("identifier", String::new(), value));
    }
    for contributor in &publication.contributors {
        let value = format!("{} ({})", contributor.name, contributor.role);
        rows.push(row("contributor", String::new(), value));
    }
    for holding in &publication.holdings {
        let value = match &holding.location {
            Some(location) => format!("{}: {location}", holding.kind.as_str()),
            None => holding.kind.as_str().to_string(),
        };
        rows.push(row("holding", String::new(), value));
    }
    for work in publication.works().await? {
        rows.push(row("work", work.id.to_string(), work.title));
    }
    for link in &publication.links {
        rows.push(row("link", String::new(), link.clone()));
    }
    write_table(out, &rows)
}
