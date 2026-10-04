use scorarium_archive::external_id::{self, EntityKind};
use scorarium_archive::identifier::{self, Kind};
use scorarium_archive::{ExternalPerson, IdentifierRawInput, Lookup, PublicationRawInput};
use scorarium_client::Priority;
use scorarium_client::open_library::{Author, Edition, OpenLibrary, WorkHit};
use tokio::time::Instant;

/// Everything Open Library says about an ISBN, within the deadline.
pub async fn lookup_isbn(
    client: &OpenLibrary<'_>,
    isbn: &str,
    deadline: Instant,
) -> (
    Option<PublicationRawInput>,
    Vec<(ExternalPerson, String)>,
    Lookup,
) {
    let lookup = client.edition_by_isbn(isbn, Priority::Interactive);
    let edition = match tokio::time::timeout_at(deadline, lookup).await {
        Ok(Ok(Some(edition))) => edition,
        Ok(Ok(None)) => {
            return (
                None,
                Vec::new(),
                Lookup::Failed(format!("no record for {isbn}")),
            );
        }
        Ok(Err(error)) => {
            return (
                None,
                Vec::new(),
                Lookup::Failed(format!("edition {isbn}: {error:#}")),
            );
        }
        Err(_) => {
            return (
                None,
                Vec::new(),
                Lookup::Failed(format!("edition {isbn}: timed out")),
            );
        }
    };

    let mut authors = Vec::new();
    let mut problems = Vec::new();
    for olid in &edition.authors {
        let lookup = client.author(olid, Priority::Interactive);
        match tokio::time::timeout_at(deadline, lookup).await {
            Ok(Ok(Some(author))) => authors.push(author),
            Ok(Ok(None)) => problems.push(format!("author {olid} not found")),
            Ok(Err(error)) => {
                problems.push(format!("author {olid}: {error:#}"));
                break;
            }
            Err(_) => {
                problems.push(format!("author {olid} timed out"));
                break;
            }
        }
    }

    let publication = to_publication(&edition);
    let contributors = to_contributors(&authors);
    if problems.is_empty() {
        (Some(publication), contributors, Lookup::Found)
    } else {
        (
            Some(publication),
            contributors,
            Lookup::Failed(problems.join("; ")),
        )
    }
}

/// Candidates for a title as typed, within the deadline.
pub async fn search_titles(
    client: &OpenLibrary<'_>,
    title: &str,
    deadline: Instant,
) -> Vec<WorkHit> {
    let num_candidates = 5;
    let search = client.search_title(title, num_candidates, Priority::Interactive);
    match tokio::time::timeout_at(deadline, search).await {
        Ok(Ok(hits)) => hits,
        Ok(Err(_)) | Err(_) => Vec::new(),
    }
}

/// Convert an Open Library [Edition] to scorarium's [PublicationRawInput], without its authors
pub fn to_publication(edition: &Edition) -> PublicationRawInput {
    let title = match &edition.subtitle {
        Some(subtitle) => format!("{}: {subtitle}", edition.title),
        None => edition.title.clone(),
    };

    // Open Library lists an ISBN-10 and ISBN-13 for the same printing; normalization makes them
    // one identifier
    let mut identifiers: Vec<IdentifierRawInput> = Vec::new();
    for raw in edition.isbn_13.iter().chain(&edition.isbn_10) {
        let Ok(normalized) = identifier::normalize(Kind::Isbn, raw) else {
            continue;
        };
        if !identifiers.iter().any(|i| i.value == normalized.as_str()) {
            identifiers.push(IdentifierRawInput {
                kind: Kind::Isbn.as_str().to_string(),
                value: normalized.as_str().to_string(),
            });
        }
    }

    PublicationRawInput {
        title,
        publisher: edition.publishers.first().cloned().unwrap_or_default(),
        year: edition
            .publish_date
            .as_deref()
            .and_then(year)
            .unwrap_or_default(),
        identifiers,
        links: vec![edition.url()],
        ..Default::default()
    }
}

/// Open Library's IDs for the author identifier sources the archive can link to
const AUTHOR_IDS: [(&str, external_id::Kind); 10] = [
    ("wikidata", external_id::Kind::Wikidata),
    ("viaf", external_id::Kind::Viaf),
    ("isni", external_id::Kind::Isni),
    ("gnd", external_id::Kind::Gnd),
    ("lc_naf", external_id::Kind::Loc),
    ("musicbrainz", external_id::Kind::MusicBrainz),
    ("goodreads", external_id::Kind::Goodreads),
    ("librarything", external_id::Kind::LibraryThing),
    ("project_gutenberg", external_id::Kind::Gutenberg),
    ("librivox", external_id::Kind::Librivox),
];

pub fn to_contributors(authors: &[Author]) -> Vec<(ExternalPerson, String)> {
    authors
        .iter()
        .map(|author| {
            let own = person_link(external_id::Kind::OpenLibrary, &author.olid);
            let others = AUTHOR_IDS
                .iter()
                .filter_map(|(key, kind)| person_link(*kind, author.remote_ids.get(*key)?));
            let person = ExternalPerson {
                name: author.name.clone(),
                links: own
                    .into_iter()
                    .chain(others)
                    .chain(author.links.iter().cloned())
                    .collect(),
            };
            (person, "author".to_string())
        })
        .collect()
}

/// The archive's URL for a person's record at a site
fn person_link(kind: external_id::Kind, id: &str) -> Option<String> {
    let url = external_id::build(EntityKind::Person, kind, id)?;
    // The builder does not check the identifier, and a malformed one names no record
    external_id::recognize(EntityKind::Person, &url)?;
    Some(url.into())
}

/// The first run of exactly four ASCII digits in a free-text date
fn year(publish_date: &str) -> Option<String> {
    publish_date
        .split(|c: char| !c.is_ascii_digit())
        .find(|run| run.len() == 4)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn a_full_edition_becomes_a_draft() {
        let edition = Edition {
            olid: "OL7636066M".to_string(),
            title: "Bagatelles, Rondos and Other Shorter Works".to_string(),
            subtitle: Some("for Piano".to_string()),
            authors: vec!["OL127077A".to_string(), "OL2A".to_string()],
            contributions: vec!["Somebody (Illustrator)".to_string()],
            by_statement: Some("by Beethoven".to_string()),
            publishers: vec![
                "Dover Publications".to_string(),
                "Somebody Else".to_string(),
            ],
            publish_date: Some("July 1, 1987".to_string()),
            isbn_10: vec!["0486253929".to_string()],
            isbn_13: vec!["9780486253923".to_string()],
            number_of_pages: Some(128),
            covers: vec![310277],
            works: vec!["OL1258206W".to_string()],
        };
        assert_eq!(
            to_publication(&edition),
            PublicationRawInput {
                title: "Bagatelles, Rondos and Other Shorter Works: for Piano".to_string(),
                publisher: "Dover Publications".to_string(),
                year: "1987".to_string(),
                identifiers: vec![IdentifierRawInput {
                    kind: "isbn".to_string(),
                    value: "978-0-486-25392-3".to_string(),
                }],
                links: vec!["https://openlibrary.org/books/OL7636066M".to_string()],
                ..Default::default()
            }
        );
    }

    #[test]
    fn an_author_becomes_an_external_person_with_the_links_the_archive_recognizes() {
        let beethoven = Author {
            olid: "OL127077A".to_string(),
            name: "Ludwig van Beethoven".to_string(),
            personal_name: None,
            alternate_names: Vec::new(),
            birth_date: None,
            death_date: None,
            remote_ids: HashMap::from([
                ("viaf".to_string(), "32182557".to_string()),
                ("wikidata".to_string(), "Q255".to_string()),
                ("imdb".to_string(), "nm0002727".to_string()),
            ]),
            links: vec!["http://en.wikipedia.org/wiki/Ludwig_van_Beethoven".to_string()],
        };

        assert_eq!(
            to_contributors(&[beethoven]),
            [(
                ExternalPerson {
                    name: "Ludwig van Beethoven".to_string(),
                    links: vec![
                        "https://openlibrary.org/authors/OL127077A".to_string(),
                        "https://www.wikidata.org/wiki/Q255".to_string(),
                        "https://viaf.org/viaf/32182557".to_string(),
                        "http://en.wikipedia.org/wiki/Ludwig_van_Beethoven".to_string(),
                    ],
                },
                "author".to_string(),
            )]
        );
    }

    #[test]
    fn year_is_the_first_four_digit_run() {
        assert_eq!(year("July 1, 1987"), Some("1987".to_string()));
        assert_eq!(year("c1990"), Some("1990".to_string()));
        assert_eq!(year("19870701"), None);
        assert_eq!(year(""), None);
    }
}
