pub mod open_library;

use std::time::Duration;

use scorarium_archive::identifier::{self, Kind};
use scorarium_archive::{IdentifierRawInput, PublicationRawInput};

/// How long Start waits for a source before moving on with whatever arrived
pub const BUDGET: Duration = Duration::from_secs(5);

/// Fill a draft with what a source found, without disturbing what is already there.
pub fn merge(draft: &mut PublicationRawInput, found: PublicationRawInput) {
    fill(&mut draft.title, found.title);
    fill(&mut draft.publisher, found.publisher);
    fill(&mut draft.year, found.year);
    for identifier in found.identifiers {
        let found = normalized(&identifier);
        let already_present = draft
            .identifiers
            .iter()
            .any(|existing| normalized(existing).is_some_and(|existing| Some(existing) == found));
        if !already_present {
            draft.identifiers.push(identifier);
        }
    }
    for contributor in found.contributors {
        if !draft.contributors.contains(&contributor) {
            draft.contributors.push(contributor);
        }
    }
    for link in found.links {
        if !draft.links.contains(&link) {
            draft.links.push(link);
        }
    }
}

fn fill(field: &mut String, found: String) {
    if field.trim().is_empty() {
        *field = found;
    }
}

/// An identifier's canonical form, or None when it does not parse and so can never match another
fn normalized(raw: &IdentifierRawInput) -> Option<(Kind, identifier::Normalized)> {
    let kind: Kind = raw.kind.parse().ok()?;
    let value = identifier::normalize(kind, &raw.value).ok()?;
    Some((kind, value))
}

#[cfg(test)]
mod tests {
    use scorarium_archive::{ContributorInput, HoldingKind, HoldingRawInput, PersonRef};

    use super::*;

    fn isbn(value: &str) -> IdentifierRawInput {
        IdentifierRawInput {
            kind: "isbn".to_string(),
            value: value.to_string(),
        }
    }

    #[test]
    fn merge_keeps_what_the_draft_already_has() {
        let holding = HoldingRawInput {
            id: None,
            kind: HoldingKind::Physical,
            location: "Shelf 3".to_string(),
        };
        let mut draft = PublicationRawInput {
            title: "My title".to_string(),
            holdings: vec![holding.clone()],
            // As typed, not normalized, so the match has to go through normalization
            identifiers: vec![isbn("0486253929")],
            ..Default::default()
        };
        let found = PublicationRawInput {
            title: "Bagatelles, Rondos and Other Shorter Works for Piano".to_string(),
            publisher: "Dover Publications".to_string(),
            year: "1987".to_string(),
            identifiers: vec![isbn("978-0-486-25392-3"), isbn("978-0-486-23134-1")],
            contributors: vec![ContributorInput {
                name: "Ludwig van Beethoven".to_string(),
                role: "author".to_string(),
                person: PersonRef::Unresolved,
            }],
            links: vec!["https://openlibrary.org/books/OL7636066M".to_string()],
            ..Default::default()
        };

        merge(&mut draft, found);

        assert_eq!(
            draft,
            PublicationRawInput {
                title: "My title".to_string(),
                publisher: "Dover Publications".to_string(),
                year: "1987".to_string(),
                holdings: vec![holding],
                identifiers: vec![isbn("0486253929"), isbn("978-0-486-23134-1")],
                contributors: vec![ContributorInput {
                    name: "Ludwig van Beethoven".to_string(),
                    role: "author".to_string(),
                    person: PersonRef::Unresolved,
                }],
                links: vec!["https://openlibrary.org/books/OL7636066M".to_string()],
                ..Default::default()
            }
        );
    }
}
