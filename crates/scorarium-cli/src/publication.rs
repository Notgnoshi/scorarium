use std::io::Write;

use scorarium_engine::entity::Entity;
use scorarium_engine::holding::HoldingKind;
use scorarium_engine::library::Library;
use scorarium_engine::publication::{IdentifierKind, Publication};
use scorarium_engine::value::Tag;

use crate::command::{PublicationCommand, reject, require};
use crate::library::write_table;
use crate::{audit, reference};

/// List a library's publications, keeping only those carrying every given tag.
pub(crate) async fn list(
    library: &Library,
    tags: &[Tag],
    out: &mut impl Write,
) -> eyre::Result<()> {
    let rows: Vec<[String; 6]> = library
        .publications()
        .await?
        .into_iter()
        .filter(|publication| tags.iter().all(|tag| publication.tags().contains(tag)))
        .map(|publication| {
            let contributors: Vec<&str> = publication
                .contributors()
                .iter()
                .map(|contributor| contributor.name.as_str())
                .collect();
            let holdings: Vec<&str> = publication
                .holdings()
                .iter()
                .map(|holding| match holding.kind {
                    HoldingKind::Physical { .. } => "physical",
                })
                .collect();
            [
                publication.id().to_string(),
                if publication.is_draft() {
                    "draft".to_string()
                } else {
                    String::new()
                },
                publication.title().unwrap_or_default().to_string(),
                contributors.join(", "),
                publication
                    .year()
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
    let mut rows = vec![row("id", publication.id().to_string(), String::new())];
    if publication.is_draft() {
        rows.push(row("draft", String::new(), String::new()));
    }
    rows.push(row(
        "title",
        String::new(),
        publication.title().unwrap_or_default().to_string(),
    ));
    let optional = [
        ("publisher", publication.publisher().map(str::to_string)),
        ("year", publication.year().map(|year| year.to_string())),
        ("stars", publication.stars().map(|stars| stars.to_string())),
        ("note", publication.note().map(str::to_string)),
        (
            "classification",
            publication.classification().map(|c| c.to_string()),
        ),
    ];
    for (label, value) in optional {
        if let Some(value) = value {
            rows.push(row(label, String::new(), value));
        }
    }
    if !publication.tags().is_empty() {
        let tags: Vec<&str> = publication.tags().iter().map(Tag::as_str).collect();
        rows.push(row("tags", String::new(), tags.join(", ")));
    }
    for identifier in publication.identifiers() {
        let value = format!("{} {}", identifier_kind(identifier.kind()), identifier);
        rows.push(row("identifier", String::new(), value));
    }
    for contributor in publication.contributors() {
        let value = format!("{} ({})", contributor.name, contributor.role);
        rows.push(row("contributor", String::new(), value));
    }
    for holding in publication.holdings() {
        let value = match &holding.kind {
            HoldingKind::Physical {
                location: Some(location),
            } => format!("physical: {location}"),
            HoldingKind::Physical { location: None } => "physical".to_string(),
        };
        rows.push(row("holding", holding.id.to_string(), value));
    }
    for work in publication.works().await? {
        rows.push(row("work", work.id().to_string(), work.title().to_string()));
    }
    for link in publication.links() {
        rows.push(row("link", link.id.to_string(), link.url.to_string()));
    }
    write_table(out, &rows)
}

fn identifier_kind(kind: IdentifierKind) -> &'static str {
    match kind {
        IdentifierKind::Isbn => "isbn",
        IdentifierKind::Ismn => "ismn",
        IdentifierKind::PublisherNumber => "publisher-number",
        IdentifierKind::PlateNumber => "plate-number",
    }
}

/// Resolve a publication title or id
pub(crate) async fn resolve(library: &Library, reference: &str) -> eyre::Result<Publication> {
    reference::resolve(
        "publication",
        reference,
        library.publications().await?,
        |p| p.id(),
        |p| p.title().unwrap_or_default(),
    )
}

/// Run a publication command against a library.
pub(crate) async fn run(
    library: &Library,
    subject: Option<&str>,
    command: PublicationCommand,
    out: &mut impl Write,
) -> eyre::Result<()> {
    match command {
        PublicationCommand::List { tag } => {
            reject(subject, "publication", "list")?;
            let tags = tag
                .iter()
                .map(|tag| tag.parse())
                .collect::<Result<Vec<Tag>, _>>()?;
            list(library, &tags, out).await
        }
        PublicationCommand::Show => {
            let publication = resolve(library, require(subject, "publication")?).await?;
            show(publication, out).await
        }
        PublicationCommand::Audit(args) => {
            let publication = resolve(library, require(subject, "publication")?).await?;
            audit::entity_log(&publication, args.limit(), out).await
        }
    }
}
