use std::collections::BTreeSet;
use std::fmt::{self, Display};
use std::str::FromStr;

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

/// Whether a holding is a thing on a shelf or a file
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldingKind {
    Physical,
    Digital,
}

impl HoldingKind {
    pub fn as_str(self) -> &'static str {
        match self {
            HoldingKind::Physical => "physical",
            HoldingKind::Digital => "digital",
        }
    }
}

impl FromStr for HoldingKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "physical" => Ok(HoldingKind::Physical),
            "digital" => Ok(HoldingKind::Digital),
            _ => Err(format!("unknown holding kind: {s}")),
        }
    }
}

/// One holding as entered in the web form
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HoldingRawInput {
    /// The holding this edits; None for a holding being created
    pub id: Option<i64>,
    pub kind: HoldingKind,
    /// Freeform for a physical copy, a filepath in the assets directory for a digital one
    pub location: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct HoldingInput {
    pub(crate) id: Option<i64>,
    pub(crate) kind: HoldingKind,
    pub(crate) location: Option<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct HoldingErrors {
    /// A publication with no copies at all is a publication nobody holds
    pub none: Option<ValidationError>,
    /// One slot per raw copy
    pub each: Vec<Option<ValidationError>>,
}

impl HoldingErrors {
    pub fn is_empty(&self) -> bool {
        self.none.is_none() && self.each.iter().all(Option::is_none)
    }
}

/// Check copies on their own, for the import entry page, which has no publication yet.
pub fn parse_holdings(raw: &[HoldingRawInput]) -> Result<Vec<HoldingInput>, HoldingErrors> {
    let mut holdings = Vec::new();
    let errors = HoldingErrors {
        none: raw.is_empty().then_some(ValidationError::NoHoldings),
        each: raw
            .iter()
            .map(|holding| {
                let location = optional(&holding.location);
                if holding.kind == HoldingKind::Digital && location.is_none() {
                    return Some(ValidationError::FileRequired);
                }
                holdings.push(HoldingInput {
                    id: holding.id,
                    kind: holding.kind,
                    location,
                });
                None
            })
            .collect(),
    };
    if errors.is_empty() {
        Ok(holdings)
    } else {
        Err(errors)
    }
}

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

pub(crate) fn optional(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}
