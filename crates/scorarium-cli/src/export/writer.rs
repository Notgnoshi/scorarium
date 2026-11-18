use std::collections::HashMap;
use std::io::Write;

use scorarium_engine::contributor::Contributor;
use scorarium_engine::entity::{Contributed, Entity};
use scorarium_engine::library::Library;
use scorarium_engine::person::{PersonId, derive_sort_name};
use scorarium_engine::publication::{IdentifierKind, Publication};
use scorarium_engine::search::normalize;
use scorarium_engine::value::{NonBlank, title_sort_key};
use scorarium_engine::work::{Work, WorkId};

use crate::export::label::{Labeler, lead};
use crate::library::visibility;

/// How much of a library to export as a script
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportLevel {
    /// Publications with just their titles and contributors
    Publications,
    /// Publications and their works, with catalog numbers and contributors
    Works,
}

/// Write a script that recreates the committed publications of `library`.
pub async fn export_script(
    library: &Library,
    level: ExportLevel,
    out: &mut impl Write,
) -> eyre::Result<()> {
    let mut publications = library.publications().await?;
    publications.retain(|p| !p.is_draft());
    publications.sort_by_cached_key(|p| {
        let lead = lead(p.contributors());
        (
            lead.is_none(),
            lead.map(|c| normalize(&c.sort_name)).unwrap_or_default(),
            title_sort_key(p.title().unwrap_or_default()),
            p.id().get(),
        )
    });

    let description = match level {
        ExportLevel::Publications => "publications only",
        ExportLevel::Works => "publications and works",
    };
    writeln!(out, "# {}, exported with {description}", library.name())?;
    writeln!(out)?;

    let mut exporter = Exporter {
        out,
        level,
        labeler: Labeler::default(),
        people: HashMap::new(),
        works: HashMap::new(),
    };
    let name = library.name();
    exporter.command(
        &["library", "create"],
        &[name, visibility(library.is_private())],
        &[],
    )?;
    exporter.command(&["library", name, "select"], &[], &[])?;
    for publication in &publications {
        writeln!(exporter.out)?;
        exporter.publication(publication).await?;
    }
    writeln!(exporter.out)?;
    exporter.command(&["wait"], &[], &[])
}

struct Exporter<'a, W: Write> {
    out: &'a mut W,
    level: ExportLevel,
    labeler: Labeler,
    /// The label of each person already written
    people: HashMap<PersonId, String>,
    /// The label of each work already written
    works: HashMap<WorkId, String>,
}

impl<W: Write> Exporter<'_, W> {
    /// Write one command: its keywords, then its values and options.
    ///
    /// Write the options first and the values after `--` when a value starts with `-`.
    fn command(
        &mut self,
        keywords: &[&str],
        values: &[&str],
        options: &[(&str, Option<&str>)],
    ) -> eyre::Result<()> {
        let values: Vec<String> = values.iter().map(|v| v.replace("\r\n", "\n")).collect();
        let mut flags = Vec::new();
        for (name, value) in options {
            match value {
                Some(value) if value.starts_with('-') => flags.push(format!("{name}={value}")),
                Some(value) => flags.extend([name.to_string(), value.to_string()]),
                None => flags.push(name.to_string()),
            }
        }
        let mut words: Vec<String> = keywords.iter().map(|k| k.to_string()).collect();
        if values.iter().any(|v| v.starts_with('-')) {
            words.extend(flags);
            words.push("--".to_string());
            words.extend(values);
        } else {
            words.extend(values);
            words.extend(flags);
        }
        let line = shlex::try_join(words.iter().map(String::as_str))?;
        writeln!(self.out, "{line}")?;
        Ok(())
    }

    async fn publication(&mut self, publication: &Publication) -> eyre::Result<()> {
        let label = self.labeler.publication(publication);
        let subject = format!("@{label}");
        let title = publication.title();
        // Identifiers are ordered by kind, so the seed is an ISBN when there is one.
        let mut identifiers = publication
            .identifiers()
            .iter()
            .filter_map(|i| match i.kind() {
                IdentifierKind::Isbn => Some(("isbn", i.as_str())),
                IdentifierKind::Ismn => Some(("ismn", i.as_str())),
                IdentifierKind::PublisherNumber | IdentifierKind::PlateNumber => None,
            });
        let seed = identifiers.next();

        let seed_text = seed.map_or(title.unwrap_or_default(), |(_, text)| text);
        self.command(&["import"], &[seed_text], &[("--as", Some(&label))])?;
        if seed.is_some()
            && let Some(title) = title
        {
            self.command(&["publication", &subject, "set", "title"], &[title], &[])?;
        }
        for (kind, text) in identifiers {
            self.command(
                &["publication", &subject, "add", "identifier", kind],
                &[text],
                &[],
            )?;
        }
        for contributor in publication.contributors() {
            self.contributor(&["publication", &subject], contributor)?;
        }
        if self.level != ExportLevel::Publications {
            for work in publication.works().await?.iter().filter(|w| !w.is_draft()) {
                self.work(&["publication", &subject], work)?;
            }
        }
        self.command(&["publication", &subject, "commit"], &[], &[])
    }

    /// Write `<subject> add work`, creating the work at the first mention.
    fn work(&mut self, subject: &[&str], work: &Work) -> eyre::Result<()> {
        let keywords = [subject, &["add", "work"]].concat();
        if let Some(label) = self.works.get(&work.id()) {
            return self.command(&keywords, &[&format!("@{label}")], &[]);
        }

        let label = self.labeler.work(work);
        self.command(
            &keywords,
            &[work.title()],
            &[("--new", None), ("--as", Some(&label))],
        )?;
        let subject = format!("@{label}");
        for number in work.catalog_numbers() {
            self.command(
                &["work", &subject, "add", "catalog-number"],
                &[number.as_str()],
                &[],
            )?;
        }
        for contributor in work.contributors() {
            self.contributor(&["work", &subject], contributor)?;
        }
        self.works.insert(work.id(), label);
        Ok(())
    }

    /// Write `<subject> add contributor`, creating the person at the first mention.
    fn contributor(&mut self, subject: &[&str], contributor: &Contributor) -> eyre::Result<()> {
        let keywords = [subject, &["add", "contributor"]].concat();
        let role = contributor.role.as_str();
        if let Some(label) = self.people.get(&contributor.person) {
            let person = format!("@{label}");
            return self.command(&keywords, &[&person, role], &[]);
        }

        let label = self.labeler.person(&contributor.name);
        self.command(
            &keywords,
            &[&contributor.name, role],
            &[("--new", None), ("--as", Some(&label))],
        )?;
        let name: NonBlank = contributor.name.parse()?;
        if derive_sort_name(&name).as_str() != contributor.sort_name {
            let person = format!("@{label}");
            self.command(
                &["person", &person, "set", "sort-name"],
                &[&contributor.sort_name],
                &[],
            )?;
        }
        self.people.insert(contributor.person, label);
        Ok(())
    }
}
