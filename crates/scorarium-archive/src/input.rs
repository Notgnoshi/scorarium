use std::collections::BTreeSet;
use std::fmt::{self, Display};

use crate::catalog::CatalogNumber;
use crate::identifier;

/// Why a field was refused. The [Display] is the message the page shows.
#[derive(Debug, PartialEq, Eq)]
pub enum ValidationError {
    TitleRequired,
    NameRequired,
    RoleRequired,
    FillOrRemove,
    AlreadyListed,
    YearNotANumber,
    NoHoldings,
    FileRequired,
    UnknownIdentifierKind,
    InvalidIdentifier(identifier::Error),
}

impl Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidationError::TitleRequired => write!(f, "A title is required."),
            ValidationError::NameRequired => write!(f, "A name is required."),
            ValidationError::RoleRequired => write!(f, "A role is required."),
            ValidationError::FillOrRemove => write!(f, "Fill this in or remove it."),
            ValidationError::AlreadyListed => write!(f, "Already listed."),
            ValidationError::YearNotANumber => write!(f, "The year must be a number."),
            ValidationError::NoHoldings => write!(f, "A publication needs at least one copy."),
            ValidationError::FileRequired => write!(f, "Choose a file for a digital copy."),
            ValidationError::UnknownIdentifierKind => write!(f, "Unknown identifier kind."),
            ValidationError::InvalidIdentifier(err) => write!(f, "{err}"),
        }
    }
}

/// A contributor and their role
///
/// This type is shared between the raw input from the web form and the "validated" contributor type
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContributorInput {
    pub name: String,
    pub role: String,
}

/// Check credits, one slot per input
///
/// A work credits people the same way a publication does, so both parsers check theirs here.
pub(crate) fn parse_contributors(
    raw: &[ContributorInput],
) -> Result<Vec<ContributorInput>, Vec<Option<ValidationError>>> {
    let mut contributors = Vec::new();
    let mut seen = BTreeSet::new();
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
            if !seen.insert((name.to_string(), role.to_string())) {
                return Some(ValidationError::AlreadyListed);
            }
            contributors.push(ContributorInput {
                name: name.to_string(),
                role: role.to_string(),
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

/// A field left blank holds no value, rather than an empty one
pub(crate) fn trimmed_or_none(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}
