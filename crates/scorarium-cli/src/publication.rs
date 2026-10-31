use std::io::Write;

use scorarium_engine::entity::Entity;
use scorarium_engine::holding::NewHolding;
use scorarium_engine::library::Library;
use scorarium_engine::publication::{Identifier, Publication};
use scorarium_engine::value::Tag;

use crate::command::{
    PublicationAdd, PublicationClear, PublicationCommand, PublicationRemove, PublicationSet,
    reject, require,
};
use crate::library::write_table;
use crate::{audit, person, reference, work};

/// The kinds of identifier printed on a publication.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub(crate) enum IdentifierKind {
    Isbn,
    Ismn,
    PublisherNumber,
    PlateNumber,
}

impl From<IdentifierKind> for scorarium_engine::publication::IdentifierKind {
    fn from(kind: IdentifierKind) -> Self {
        match kind {
            IdentifierKind::Isbn => Self::Isbn,
            IdentifierKind::Ismn => Self::Ismn,
            IdentifierKind::PublisherNumber => Self::PublisherNumber,
            IdentifierKind::PlateNumber => Self::PlateNumber,
        }
    }
}

/// The kinds of copy a library can hold.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub(crate) enum HoldingKind {
    Physical,
}

impl HoldingKind {
    /// Describe a new holding of this kind.
    fn new_holding(self, location: Option<&str>) -> NewHolding<'_> {
        match self {
            HoldingKind::Physical => NewHolding::Physical { location },
        }
    }
}

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
                    scorarium_engine::holding::HoldingKind::Physical { .. } => "physical",
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
            scorarium_engine::holding::HoldingKind::Physical {
                location: Some(location),
            } => format!("physical: {location}"),
            scorarium_engine::holding::HoldingKind::Physical { location: None } => {
                "physical".to_string()
            }
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

fn identifier_kind(kind: scorarium_engine::publication::IdentifierKind) -> &'static str {
    use scorarium_engine::publication::IdentifierKind as Kind;

    match kind {
        Kind::Isbn => "isbn",
        Kind::Ismn => "ismn",
        Kind::PublisherNumber => "publisher-number",
        Kind::PlateNumber => "plate-number",
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
        PublicationCommand::Set { field } => {
            let mut publication = resolve(library, require(subject, "publication")?).await?;
            match field {
                PublicationSet::Title { title } => publication.set_title(title).await?,
                PublicationSet::Publisher { publisher } => {
                    publication.set_publisher(Some(&publisher)).await?
                }
                PublicationSet::Year { year } => publication.set_year(Some(year)).await?,
                PublicationSet::Stars { stars } => publication.set_stars(Some(stars)).await?,
                PublicationSet::Note { note } => publication.set_note(Some(&note)).await?,
                PublicationSet::Classification { classification } => {
                    publication
                        .set_classification(classification.into())
                        .await?
                }
            }
            Ok(())
        }
        PublicationCommand::Clear { field } => {
            let mut publication = resolve(library, require(subject, "publication")?).await?;
            match field {
                PublicationClear::Publisher => publication.set_publisher(None).await?,
                PublicationClear::Year => publication.set_year(None).await?,
                PublicationClear::Stars => publication.set_stars(None).await?,
                PublicationClear::Note => publication.set_note(None).await?,
            }
            Ok(())
        }
        PublicationCommand::Add { field } => {
            let mut publication = resolve(library, require(subject, "publication")?).await?;
            match field {
                PublicationAdd::Tag { tag } => publication.add_tag(tag).await?,
                PublicationAdd::Identifier { kind, text } => {
                    publication
                        .add_identifier(Identifier::new(kind.into(), &text)?)
                        .await?
                }
                PublicationAdd::Link { url } => publication.add_link(url).await?,
                PublicationAdd::Contributor { person, role } => {
                    let person = person::resolve(library, &person).await?;
                    publication.add_contributor(person.id(), &role).await?
                }
                PublicationAdd::Work { work } => {
                    let work = work::resolve(library, &work).await?;
                    publication.add_work(work.id()).await?
                }
                PublicationAdd::Holding { kind, location } => {
                    let holding = kind.new_holding(location.as_deref());
                    publication.add_holding(holding).await?;
                }
            }
            Ok(())
        }
        PublicationCommand::Remove { field } => {
            let mut publication = resolve(library, require(subject, "publication")?).await?;
            match field {
                PublicationRemove::Tag { tag } => publication.remove_tag(&tag).await?,
                PublicationRemove::Identifier { kind, text } => {
                    publication
                        .remove_identifier(&Identifier::new(kind.into(), &text)?)
                        .await?
                }
                PublicationRemove::Link { link } => {
                    let id = reference::link(publication.links(), &link)?;
                    publication.remove_link(id).await?
                }
                PublicationRemove::Contributor { person, role } => {
                    let person = person::resolve(library, &person).await?.id();
                    let roles: Vec<String> = match role {
                        Some(role) => vec![role],
                        None => publication
                            .contributors()
                            .iter()
                            .filter(|c| c.person == person)
                            .map(|c| c.role.clone())
                            .collect(),
                    };
                    for role in roles {
                        publication.remove_contributor(person, &role).await?;
                    }
                }
                PublicationRemove::Work { work } => {
                    let work = work::resolve(library, &work).await?;
                    publication.remove_work(work.id()).await?
                }
                PublicationRemove::Holding { holding } => {
                    if !publication.holdings().iter().any(|h| h.id == holding) {
                        eyre::bail!("no holding '{holding}'");
                    }
                    publication.remove_holding(holding).await?
                }
            }
            Ok(())
        }
        PublicationCommand::Delete => {
            let publication = resolve(library, require(subject, "publication")?).await?;
            publication.delete().await?;
            Ok(())
        }
        PublicationCommand::Commit => {
            let mut publication = resolve(library, require(subject, "publication")?).await?;
            publication.commit().await?;
            Ok(())
        }
    }
}
