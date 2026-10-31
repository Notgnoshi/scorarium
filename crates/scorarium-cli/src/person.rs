use std::io::Write;

use scorarium_engine::contributor::Contributor;
use scorarium_engine::entity::Entity;
use scorarium_engine::library::Library;
use scorarium_engine::person::{Person, PersonId};

use crate::command::{PersonAdd, PersonCommand, PersonRemove, PersonSet, reject, require};
use crate::library::write_table;
use crate::reference::Target;
use crate::{audit, reference, search};

/// List the contributors in a library, keeping only those with the given role.
pub(crate) async fn list(
    library: &Library,
    role: Option<&str>,
    out: &mut impl Write,
) -> eyre::Result<()> {
    let mut people = library.people(role).await?;
    people.sort_by(|a, b| a.sort_name().cmp(b.sort_name()));
    let rows: Vec<[String; 3]> = people
        .iter()
        .map(|person| {
            [
                person.id().to_string(),
                person.name().to_string(),
                person.sort_name().to_string(),
            ]
        })
        .collect();
    write_table(out, &rows)
}

/// Show a person with the publications and works they contributed to.
pub(crate) async fn show(person: Person, out: &mut impl Write) -> eyre::Result<()> {
    let row = |label: &str, id: String, value: String| [label.to_string(), id, value];
    let mut rows = vec![
        row("id", person.id().to_string(), String::new()),
        row("name", String::new(), person.name().to_string()),
        row("sort-name", String::new(), person.sort_name().to_string()),
    ];
    for link in person.links() {
        rows.push(row("link", link.id.to_string(), link.url.to_string()));
    }
    let mut publications = person.publications().await?;
    publications.sort_by(|a, b| a.title().cmp(&b.title()));
    for publication in &publications {
        let title = publication.title().unwrap_or_default();
        let roles = roles_of(publication.contributors(), person.id()).join(", ");
        rows.push(row(
            "publication",
            publication.id().to_string(),
            format!("{title} ({roles})"),
        ));
    }
    for work in person.works().await? {
        let roles = roles_of(work.contributors(), person.id()).join(", ");
        rows.push(row(
            "work",
            work.id().to_string(),
            format!("{} ({roles})", work.title()),
        ));
    }
    write_table(out, &rows)
}

/// List the roles `person` holds among `contributors`.
fn roles_of(contributors: &[Contributor], person: PersonId) -> Vec<&str> {
    contributors
        .iter()
        .filter(|c| c.person == person)
        .map(|c| c.role.as_str())
        .collect()
}

/// Resolve a person name or id
pub(crate) async fn resolve(library: &Library, reference: &str) -> eyre::Result<Person> {
    reference::resolve(
        "person",
        reference,
        library.people(None).await?,
        |p| p.id(),
        |p| p.name(),
    )
}

/// Decide whether a person name or id links an existing person or creates a new one.
pub(crate) async fn target(
    library: &Library,
    reference: &str,
    new: bool,
) -> eyre::Result<Target<Person>> {
    reference::target(
        "person",
        reference,
        new,
        library.people(None).await?,
        |p| p.name(),
        search::person_context,
    )
}

/// Run a person command against a library.
pub(crate) async fn run(
    library: &Library,
    subject: Option<&str>,
    command: PersonCommand,
    out: &mut impl Write,
) -> eyre::Result<()> {
    match command {
        PersonCommand::List { role } => {
            reject(subject, "person", "list")?;
            list(library, role.as_deref(), out).await
        }
        PersonCommand::Show => {
            let person = resolve(library, require(subject, "person")?).await?;
            show(person, out).await
        }
        PersonCommand::Audit(args) => {
            let person = resolve(library, require(subject, "person")?).await?;
            audit::entity_log(&person, args.limit(), out).await
        }
        PersonCommand::Commit => {
            let mut person = resolve(library, require(subject, "person")?).await?;
            person.commit().await?;
            Ok(())
        }
        PersonCommand::DiscardChanges => {
            let reference = require(subject, "person")?;
            let person = resolve(library, reference).await?;
            let Some(item) = person.review_item().await? else {
                eyre::bail!("person '{reference}' is not in the review queue");
            };
            item.discard().await?;
            Ok(())
        }
        PersonCommand::Set { field } => {
            let mut person = resolve(library, require(subject, "person")?).await?;
            match field {
                PersonSet::Name { name } => {
                    person.rename(&name, person.sort_name().to_string()).await?
                }
                PersonSet::SortName { sort_name } => {
                    person.rename(person.name().to_string(), &sort_name).await?
                }
            }
            Ok(())
        }
        PersonCommand::Add { field } => {
            let mut person = resolve(library, require(subject, "person")?).await?;
            match field {
                PersonAdd::Link { url } => Ok(person.add_link(url).await?),
            }
        }
        PersonCommand::Remove { field } => {
            let mut person = resolve(library, require(subject, "person")?).await?;
            match field {
                PersonRemove::Link { link } => {
                    let id = reference::link(person.links(), &link)?;
                    Ok(person.remove_link(id).await?)
                }
            }
        }
    }
}
