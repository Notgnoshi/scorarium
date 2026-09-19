use scorarium_archive::identifier::{self, Kind};
use scorarium_archive::{ContributorInput, IdentifierRawInput, Lookup, PublicationRawInput};
use scorarium_client::Priority;
use scorarium_client::open_library::{Author, Edition, OpenLibrary, WorkHit};
use tokio::time::Instant;

/// Everything Open Library says about an ISBN, within the deadline.
pub async fn lookup_isbn(
    client: &OpenLibrary<'_>,
    isbn: &str,
    deadline: Instant,
) -> (Option<PublicationRawInput>, Lookup) {
    let lookup = client.edition_by_isbn(isbn, Priority::Interactive);
    let edition = match tokio::time::timeout_at(deadline, lookup).await {
        Ok(Ok(Some(edition))) => edition,
        Ok(Ok(None)) => return (None, Lookup::Failed(format!("no record for {isbn}"))),
        Ok(Err(error)) => return (None, Lookup::Failed(format!("edition {isbn}: {error:#}"))),
        Err(_) => return (None, Lookup::Failed(format!("edition {isbn}: timed out"))),
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

    let publication = to_publication(&edition, &authors);
    if problems.is_empty() {
        (Some(publication), Lookup::Found)
    } else {
        (Some(publication), Lookup::Failed(problems.join("; ")))
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

/// Convert an Open Library [Edition] to scorarium's [PublicationRawInput]
pub fn to_publication(edition: &Edition, authors: &[Author]) -> PublicationRawInput {
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
        contributors: authors
            .iter()
            .map(|author| ContributorInput {
                name: author.name.clone(),
                role: "author".to_string(),
            })
            .collect(),
        links: vec![edition.url()],
        ..Default::default()
    }
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
        let author = |olid: &str, name: &str| Author {
            olid: olid.to_string(),
            name: name.to_string(),
            personal_name: None,
            alternate_names: Vec::new(),
            birth_date: None,
            death_date: None,
            remote_ids: HashMap::new(),
        };
        let authors = [
            author("OL127077A", "Ludwig van Beethoven"),
            author("OL2A", "Somebody"),
        ];

        assert_eq!(
            to_publication(&edition, &authors),
            PublicationRawInput {
                title: "Bagatelles, Rondos and Other Shorter Works: for Piano".to_string(),
                publisher: "Dover Publications".to_string(),
                year: "1987".to_string(),
                identifiers: vec![IdentifierRawInput {
                    kind: "isbn".to_string(),
                    value: "978-0-486-25392-3".to_string(),
                }],
                contributors: vec![
                    ContributorInput {
                        name: "Ludwig van Beethoven".to_string(),
                        role: "author".to_string(),
                    },
                    ContributorInput {
                        name: "Somebody".to_string(),
                        role: "author".to_string(),
                    },
                ],
                links: vec!["https://openlibrary.org/books/OL7636066M".to_string()],
                ..Default::default()
            }
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
