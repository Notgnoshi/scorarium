use std::io::Write;

use scorarium_engine::entity::Entity;
use scorarium_engine::library::Library;
use scorarium_engine::value::Tag;
use scorarium_engine::work::Work;

use crate::command::{WorkCommand, require};
use crate::library::write_table;
use crate::{audit, reference};

pub(crate) async fn show(work: Work, out: &mut impl Write) -> eyre::Result<()> {
    let row = |label: &str, id: String, value: String| [label.to_string(), id, value];
    let mut rows = vec![
        row("id", work.id().to_string(), String::new()),
        row("title", String::new(), work.title().to_string()),
    ];
    let optional = [
        ("key", work.key().map(str::to_string)),
        ("time signature", work.time_signature().map(str::to_string)),
        (
            "instrumentation",
            work.instrumentation().map(str::to_string),
        ),
        ("stars", work.stars().map(|stars| stars.to_string())),
        ("note", work.note().map(str::to_string)),
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
        rows.push(row("catalog number", String::new(), number.to_string()));
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
        rows.push(row("link", String::new(), link.url.to_string()));
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
    }
}
