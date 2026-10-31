use std::io::Write;

use scorarium_engine::entity::Entity;
use scorarium_engine::library::Library;
use scorarium_engine::value::Tag;
use scorarium_engine::work::Work;

use crate::command::{WorkAdd, WorkClear, WorkCommand, WorkRemove, WorkSet, require};
use crate::library::write_table;
use crate::reference::Target;
use crate::{audit, person, reference, search};

pub(crate) async fn show(work: Work, out: &mut impl Write) -> eyre::Result<()> {
    let row = |label: &str, id: String, value: String| [label.to_string(), id, value];
    let mut rows = vec![
        row("id", work.id().to_string(), String::new()),
        row("title", String::new(), work.title().to_string()),
    ];
    let optional = [
        ("key", work.key().map(str::to_string)),
        ("time-signature", work.time_signature().map(str::to_string)),
        (
            "instrumentation",
            work.instrumentation().map(str::to_string),
        ),
        ("stars", work.stars().map(|stars| stars.to_string())),
        ("note", work.note().map(str::to_string)),
        (
            "classification",
            work.classification().map(|c| c.to_string()),
        ),
    ];
    for (label, value) in optional {
        if let Some(value) = value {
            rows.push(row(label, String::new(), value));
        }
    }
    if !work.tags().is_empty() {
        let tags: Vec<&str> = work.tags().iter().map(Tag::as_str).collect();
        rows.push(row("tags", String::new(), tags.join(", ")));
    }
    for number in work.catalog_numbers() {
        rows.push(row("catalog-number", String::new(), number.to_string()));
    }
    for contributor in work.contributors() {
        let value = format!("{} ({})", contributor.name, contributor.role);
        rows.push(row("contributor", String::new(), value));
    }
    for publication in work.publications().await? {
        rows.push(row(
            "publication",
            publication.id().to_string(),
            publication.title().unwrap_or_default().to_string(),
        ));
    }
    for link in work.links() {
        rows.push(row("link", link.id.to_string(), link.url.to_string()));
    }
    write_table(out, &rows)
}

/// Resolve the title or id of a work
pub(crate) async fn resolve(library: &Library, reference: &str) -> eyre::Result<Work> {
    reference::resolve(
        "work",
        reference,
        library.works().await?,
        |w| w.id(),
        |w| w.title(),
    )
}

/// Decide whether a work title or id links an existing work or creates a new one.
pub(crate) async fn target(
    library: &Library,
    reference: &str,
    new: bool,
) -> eyre::Result<Target<Work>> {
    reference::target(
        "work",
        reference,
        new,
        library.works().await?,
        |w| w.title(),
        search::work_context,
    )
}

/// Run a work command against a library.
pub(crate) async fn run(
    library: &Library,
    subject: Option<&str>,
    command: WorkCommand,
    out: &mut impl Write,
) -> eyre::Result<()> {
    match command {
        WorkCommand::Show => {
            let work = resolve(library, require(subject, "work")?).await?;
            show(work, out).await
        }
        WorkCommand::Audit(args) => {
            let work = resolve(library, require(subject, "work")?).await?;
            audit::entity_log(&work, args.limit(), out).await
        }
        WorkCommand::Commit => {
            let mut work = resolve(library, require(subject, "work")?).await?;
            work.commit().await?;
            Ok(())
        }
        WorkCommand::DiscardChanges => {
            let reference = require(subject, "work")?;
            let work = resolve(library, reference).await?;
            let Some(item) = work.review_item().await? else {
                eyre::bail!("work '{reference}' is not in the review queue");
            };
            item.discard().await?;
            Ok(())
        }
        WorkCommand::Set { field } => {
            let mut work = resolve(library, require(subject, "work")?).await?;
            match field {
                WorkSet::Title { title } => work.set_title(title).await?,
                WorkSet::Key { key } => work.set_key(Some(&key)).await?,
                WorkSet::TimeSignature { time_signature } => {
                    work.set_time_signature(Some(&time_signature)).await?
                }
                WorkSet::Instrumentation { instrumentation } => {
                    work.set_instrumentation(Some(&instrumentation)).await?
                }
                WorkSet::Stars { stars } => work.set_stars(Some(stars)).await?,
                WorkSet::Note { note } => work.set_note(Some(&note)).await?,
                WorkSet::Classification { classification } => {
                    work.set_classification(classification.into()).await?
                }
            }
            Ok(())
        }
        WorkCommand::Clear { field } => {
            let mut work = resolve(library, require(subject, "work")?).await?;
            match field {
                WorkClear::Key => work.set_key(None).await?,
                WorkClear::TimeSignature => work.set_time_signature(None).await?,
                WorkClear::Instrumentation => work.set_instrumentation(None).await?,
                WorkClear::Stars => work.set_stars(None).await?,
                WorkClear::Note => work.set_note(None).await?,
            }
            Ok(())
        }
        WorkCommand::Add { field } => {
            let mut work = resolve(library, require(subject, "work")?).await?;
            match field {
                WorkAdd::Tag { tag } => work.add_tag(tag).await?,
                WorkAdd::CatalogNumber { number } => work.add_catalog_number(number).await?,
                WorkAdd::Link { url } => work.add_link(url).await?,
                WorkAdd::Contributor { person, role, new } => {
                    match person::target(library, &person, new).await? {
                        Target::Existing(person) => {
                            work.add_contributor(person.id(), &role).await?
                        }
                        Target::New => {
                            work.create_contributor(&person, &role).await?;
                        }
                    }
                }
            }
            Ok(())
        }
        WorkCommand::Remove { field } => {
            let mut work = resolve(library, require(subject, "work")?).await?;
            match field {
                WorkRemove::Tag { tag } => work.remove_tag(&tag).await?,
                WorkRemove::CatalogNumber { number } => work.remove_catalog_number(&number).await?,
                WorkRemove::Link { link } => {
                    let id = reference::link(work.links(), &link)?;
                    work.remove_link(id).await?
                }
                WorkRemove::Contributor { person, role } => {
                    let person = person::resolve(library, &person).await?.id();
                    let roles: Vec<String> = match role {
                        Some(role) => vec![role],
                        None => work
                            .contributors()
                            .iter()
                            .filter(|c| c.person == person)
                            .map(|c| c.role.clone())
                            .collect(),
                    };
                    for role in roles {
                        work.remove_contributor(person, &role).await?;
                    }
                }
            }
            Ok(())
        }
    }
}
