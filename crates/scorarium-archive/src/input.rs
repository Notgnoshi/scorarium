use std::collections::BTreeSet;
use std::fmt::{self, Display};

use crate::catalog::CatalogNumber;
use crate::fuzzy::normalize;
use crate::identifier;
use crate::summary::{PersonSummary, same_name};

/// Why a field was refused. The [Display] is the message the page shows.
#[derive(Debug, PartialEq, Eq)]
pub enum ValidationError {
    TitleRequired,
    NameRequired,
    RoleRequired,
    NameShared,
    FillOrRemove,
    AlreadyListed,
    YearNotANumber,
    NoHoldings,
    FileRequired,
    UnknownIdentifierKind,
    InvalidIdentifier(identifier::Error),
    StarsInvalid,
    InvalidTag(String),
    InvalidUrl,
}

impl Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidationError::TitleRequired => write!(f, "A title is required."),
            ValidationError::NameRequired => write!(f, "A name is required."),
            ValidationError::RoleRequired => write!(f, "A role is required."),
            ValidationError::NameShared => {
                write!(
                    f,
                    "Several people have this name. Pick one, or create another."
                )
            }
            ValidationError::FillOrRemove => write!(f, "Fill this in or remove it."),
            ValidationError::AlreadyListed => write!(f, "Already listed."),
            ValidationError::YearNotANumber => write!(f, "The year must be a number."),
            ValidationError::NoHoldings => write!(f, "A publication needs at least one copy."),
            ValidationError::FileRequired => write!(f, "Choose a file for a digital copy."),
            ValidationError::UnknownIdentifierKind => write!(f, "Unknown identifier kind."),
            ValidationError::InvalidIdentifier(err) => write!(f, "{err}"),
            ValidationError::StarsInvalid => write!(f, "A rating is 1 to 5 stars."),
            ValidationError::InvalidTag(tag) => {
                write!(f, "'{tag}' has invalid characters. Use [a-zA-Z0-9_-]")
            }
            ValidationError::InvalidUrl => {
                write!(f, "Invalid URL")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PersonRef {
    /// An existing person, whose stored name the posted name never changes
    Linked(i64),
    /// A person to create with the typed name, even when a namesake exists
    New,
    /// Nobody chosen yet; refused when submitted
    #[default]
    Unresolved,
}

/// A contributor and their role
///
/// This type is shared between the raw input from the web form and the "validated" contributor type
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContributorInput {
    pub name: String,
    pub role: String,
    pub person: PersonRef,
}

impl ContributorInput {
    pub(crate) fn resolve_by_name(&mut self, persons: &[PersonSummary]) {
        if self.person != PersonRef::Unresolved {
            return;
        }
        let mut namesakes = persons
            .iter()
            .filter(|person| same_name(&person.name, &self.name));
        self.person = match (namesakes.next(), namesakes.next()) {
            (None, _) => PersonRef::New,
            (Some(person), None) => PersonRef::Linked(person.id),
            (Some(_), Some(_)) => PersonRef::Unresolved,
        };
    }
}

/// Check credits, one slot per input
///
/// A work credits people the same way a publication does, so both parsers check theirs here.
pub(crate) fn parse_contributors(
    raw: &[ContributorInput],
) -> Result<Vec<ContributorInput>, Vec<Option<ValidationError>>> {
    let mut contributors = Vec::new();
    let mut seen_persons = BTreeSet::new();
    let mut seen_new = BTreeSet::new();
    let errors: Vec<Option<ValidationError>> = raw
        .iter()
        .map(|contributor| {
            let name = contributor.name.trim();
            let role = contributor.role.trim();
            // Both empty is a credit the user started and gave up on, rather than two mistakes
            if name.is_empty() && role.is_empty() {
                return Some(ValidationError::FillOrRemove);
            }
            if name.is_empty() {
                return Some(ValidationError::NameRequired);
            }
            if role.is_empty() {
                return Some(ValidationError::RoleRequired);
            }
            let person_seen = match contributor.person {
                PersonRef::Linked(id) => !seen_persons.insert((id, role.to_string())),
                PersonRef::New => !seen_new.insert((normalize(name), role.to_string())),
                // Names are resolved before parsing, so one still unresolved is shared by several
                // people, and which of them is meant is the user's choice
                PersonRef::Unresolved => return Some(ValidationError::NameShared),
            };
            // A credit repeats another only when it names the same person in the same role.
            if person_seen {
                return Some(ValidationError::AlreadyListed);
            }
            contributors.push(ContributorInput {
                name: name.to_string(),
                role: role.to_string(),
                person: contributor.person,
            });
            None
        })
        .collect();
    if errors.iter().all(Option::is_none) {
        Ok(contributors)
    } else {
        Err(errors)
    }
}

/// Check catalog numbers, one slot per input
pub(crate) fn parse_catalog_numbers(
    raw: &[String],
) -> Result<Vec<CatalogNumber>, Vec<Option<ValidationError>>> {
    let mut numbers: Vec<CatalogNumber> = Vec::new();
    let errors: Vec<Option<ValidationError>> = raw
        .iter()
        .map(|raw| {
            let number = CatalogNumber::parse(raw);
            if number.as_str().is_empty() {
                return Some(ValidationError::FillOrRemove);
            }
            if numbers.iter().any(|seen| seen.matches(&number)) {
                return Some(ValidationError::AlreadyListed);
            }
            numbers.push(number);
            None
        })
        .collect();
    if errors.iter().all(Option::is_none) {
        Ok(numbers)
    } else {
        Err(errors)
    }
}

pub(crate) fn parse_links(raw: &[String]) -> Result<Vec<String>, Vec<Option<ValidationError>>> {
    let mut links: Vec<String> = Vec::new();
    let errors: Vec<Option<ValidationError>> = raw
        .iter()
        .map(|raw| {
            let raw = raw.trim();
            if raw.is_empty() {
                return Some(ValidationError::FillOrRemove);
            }
            let Ok(url) = url::Url::parse(raw) else {
                return Some(ValidationError::InvalidUrl);
            };
            if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
                return Some(ValidationError::InvalidUrl);
            }
            if links.iter().any(|seen| seen == url.as_str()) {
                return Some(ValidationError::AlreadyListed);
            }
            links.push(url.into());
            None
        })
        .collect();
    if errors.iter().all(Option::is_none) {
        Ok(links)
    } else {
        Err(errors)
    }
}

/// Check a rating, which a publication and a work each carry
pub(crate) fn parse_stars(raw: &str) -> Result<Option<i64>, ValidationError> {
    match raw.trim() {
        "" => Ok(None),
        stars => match stars.parse::<i64>() {
            Ok(stars) if (1..=5).contains(&stars) => Ok(Some(stars)),
            _ => Err(ValidationError::StarsInvalid),
        },
    }
}

/// A field left blank holds no value, rather than an empty one
pub(crate) fn trimmed_or_none(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::summary::PersonSummary;

    #[test]
    fn a_name_resolves_only_when_it_matches_one_person() {
        let person = |id: i64, name: &str| PersonSummary {
            id,
            name: name.into(),
            title: String::new(),
            others: 0,
        };
        let persons = [person(1, "Erik Satie"), person(2, "Sue"), person(3, "Sue")];
        let contributor = |name: &str, person: PersonRef| ContributorInput {
            name: name.into(),
            role: "composer".into(),
            person,
        };
        let mut contributors = [
            contributor("erik satie", PersonRef::Unresolved),
            contributor("Sue", PersonRef::Unresolved),
            contributor("Nobody", PersonRef::Unresolved),
            // Already resolved: left alone, even though a namesake exists
            contributor("Erik Satie", PersonRef::New),
        ];
        for contributor in &mut contributors {
            contributor.resolve_by_name(&persons);
        }
        let resolved: Vec<PersonRef> = contributors.iter().map(|c| c.person).collect();
        assert_eq!(
            resolved,
            [
                PersonRef::Linked(1),
                PersonRef::Unresolved,
                PersonRef::New,
                PersonRef::New,
            ]
        );
    }

    #[test]
    fn links_are_normalized_and_checked() {
        let raw = [
            "  https://IMSLP.org  ",
            "https://imslp.org:443/",
            "imslp.org",
            "javascript:alert(1)",
            "",
        ];
        let raw: Vec<String> = raw.iter().map(|link| link.to_string()).collect();
        assert_eq!(
            parse_links(&raw).unwrap_err(),
            vec![
                None,
                // The same address once the host is lowercased and the default port dropped
                Some(ValidationError::AlreadyListed),
                // A bare host is not a web address; the scheme is what makes it one
                Some(ValidationError::InvalidUrl),
                Some(ValidationError::InvalidUrl),
                Some(ValidationError::FillOrRemove),
            ]
        );
        assert_eq!(
            parse_links(&["  https://IMSLP.org  ".to_string()]).unwrap(),
            vec!["https://imslp.org/".to_string()]
        );
    }
}
