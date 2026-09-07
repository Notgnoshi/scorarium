use crate::identifier::{self, IdentifierRawInput};
use crate::input::{
    self, ContributorInput, HoldingErrors, HoldingInput, HoldingRawInput, ValidationError,
};
use crate::work::{WorkErrors, WorkInput, WorkRawInput};

/// A publication's editable fields as typed from the web form
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PublicationRawInput {
    pub title: String,
    pub publisher: String,
    pub year: String,
    pub holdings: Vec<HoldingRawInput>,
    pub identifiers: Vec<IdentifierRawInput>,
    pub contributors: Vec<ContributorInput>,
    pub contents: Vec<WorkRawInput>,
}

/// A publication's validated fields for use in database updates
#[derive(Debug, PartialEq, Eq)]
pub struct PublicationInput {
    pub(crate) title: String,
    pub(crate) publisher: Option<String>,
    pub(crate) year: Option<i64>,
    pub(crate) holdings: Vec<HoldingInput>,
    pub(crate) identifiers: Vec<(identifier::Kind, identifier::Normalized)>,
    pub(crate) contributors: Vec<ContributorInput>,
    pub(crate) contents: Vec<WorkInput>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PublicationErrors {
    pub title: Option<ValidationError>,
    pub year: Option<ValidationError>,
    pub holdings: HoldingErrors,
    /// One per identifier, empty when they all passed
    pub identifiers: Vec<Option<ValidationError>>,
    /// One per contributor, empty when they all passed
    pub contributors: Vec<Option<ValidationError>>,
    /// One per work
    pub contents: Vec<WorkErrors>,
}

impl PublicationErrors {
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.year.is_none()
            && self.holdings.is_empty()
            && self.identifiers.iter().all(Option::is_none)
            && self.contributors.iter().all(Option::is_none)
            && self.contents.iter().all(WorkErrors::is_empty)
    }
}

impl PublicationRawInput {
    /// Parse, validate, and convert the input
    pub fn parse(&self) -> Result<PublicationInput, PublicationErrors> {
        let mut errors = PublicationErrors::default();
        let title = self.title.trim();
        if title.is_empty() {
            errors.title = Some(ValidationError::TitleRequired);
        }
        let year = match self.year.trim() {
            "" => None,
            year => match year.parse::<i64>() {
                Ok(year) => Some(year),
                Err(_) => {
                    errors.year = Some(ValidationError::YearNotANumber);
                    None
                }
            },
        };
        let holdings = match input::parse_holdings(&self.holdings) {
            Ok(holdings) => holdings,
            Err(holding_errors) => {
                errors.holdings = holding_errors;
                Vec::new()
            }
        };
        let identifiers = match identifier::parse_identifiers(&self.identifiers) {
            Ok(identifiers) => identifiers,
            Err(slots) => {
                errors.identifiers = slots;
                Vec::new()
            }
        };
        let contributors = match input::parse_contributors(&self.contributors) {
            Ok(contributors) => contributors,
            Err(slots) => {
                errors.contributors = slots;
                Vec::new()
            }
        };
        // A work reports its own problems, so the contents keep one slot each either way
        let mut contents = Vec::new();
        for work in &self.contents {
            match work.parse() {
                Ok(work) => {
                    contents.push(work);
                    errors.contents.push(WorkErrors::default());
                }
                Err(work_errors) => errors.contents.push(work_errors),
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(PublicationInput {
            title: title.to_string(),
            publisher: input::optional(&self.publisher),
            year,
            holdings,
            identifiers,
            contributors,
            contents,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::HoldingKind;

    fn contributor(name: &str, role: &str) -> ContributorInput {
        ContributorInput {
            name: name.into(),
            role: role.into(),
        }
    }

    fn isbn(value: &str) -> IdentifierRawInput {
        IdentifierRawInput {
            kind: "isbn".into(),
            value: value.into(),
        }
    }

    fn holding(kind: HoldingKind, location: &str) -> HoldingRawInput {
        HoldingRawInput {
            id: None,
            kind,
            location: location.into(),
        }
    }

    #[test]
    fn parse_reports_every_problem() {
        let raw = PublicationRawInput {
            year: "abc".into(),
            holdings: vec![holding(HoldingKind::Digital, "")],
            identifiers: vec![
                isbn("978-1-4950-0871-0"),
                isbn("0-486-23134-8"),
                isbn("978-0-486-23134-1"),
                isbn(""),
                IdentifierRawInput {
                    kind: "barcode".into(),
                    value: "12345".into(),
                },
            ],
            contributors: vec![
                contributor("Erik Satie", ""),
                contributor("Erik Satie", "composer"),
                contributor("Erik Satie", "composer"),
                contributor("", ""),
            ],
            contents: vec![
                WorkRawInput {
                    title: "Gnossienne No. 1".into(),
                    contributors: vec![contributor("", "composer")],
                    ..WorkRawInput::default()
                },
                WorkRawInput {
                    title: "Gnossienne No. 2".into(),
                    ..WorkRawInput::default()
                },
            ],
            ..PublicationRawInput::default()
        };
        assert_eq!(
            raw.parse().unwrap_err(),
            PublicationErrors {
                title: Some(ValidationError::TitleRequired),
                year: Some(ValidationError::YearNotANumber),
                holdings: HoldingErrors {
                    none: None,
                    each: vec![Some(ValidationError::FileRequired)],
                },
                identifiers: vec![
                    Some(ValidationError::InvalidIdentifier(
                        identifier::Error::Invalid(identifier::Kind::Isbn)
                    )),
                    // An ISBN-10 and its ISBN-13 are the same identifier once normalized
                    None,
                    Some(ValidationError::AlreadyListed),
                    Some(ValidationError::FillOrRemove),
                    Some(ValidationError::UnknownIdentifierKind),
                ],
                contributors: vec![
                    Some(ValidationError::RoleRequired),
                    None,
                    Some(ValidationError::AlreadyListed),
                    Some(ValidationError::FillOrRemove),
                ],
                contents: vec![
                    WorkErrors {
                        title: None,
                        contributors: vec![Some(ValidationError::NameRequired)],
                    },
                    WorkErrors::default(),
                ],
            }
        );
    }

    #[test]
    fn a_publication_needs_a_copy() {
        let raw = PublicationRawInput {
            title: "Untitled".into(),
            ..PublicationRawInput::default()
        };
        assert_eq!(
            raw.parse().unwrap_err(),
            PublicationErrors {
                holdings: HoldingErrors {
                    none: Some(ValidationError::NoHoldings),
                    each: Vec::new(),
                },
                ..PublicationErrors::default()
            }
        );
    }

    #[test]
    fn parse_takes_what_was_typed() {
        let raw = PublicationRawInput {
            title: "  Three gymnopedies  ".into(),
            publisher: String::new(),
            year: " 1888 ".into(),
            holdings: vec![holding(HoldingKind::Physical, "")],
            identifiers: vec![isbn("0-486-23134-8")],
            contributors: vec![contributor("Erik Satie", "composer")],
            contents: vec![WorkRawInput {
                title: "Gymnopedie No. 1".into(),
                // An anonymous or folk piece credits nobody
                ..WorkRawInput::default()
            }],
        };
        let input = raw.parse().unwrap();

        assert_eq!(input.title, "Three gymnopedies");
        assert_eq!(input.publisher, None);
        assert_eq!(input.year, Some(1888));
        assert_eq!(
            input.holdings,
            [HoldingInput {
                id: None,
                kind: HoldingKind::Physical,
                location: None,
            }]
        );
        assert_eq!(
            input.identifiers,
            [(
                identifier::Kind::Isbn,
                identifier::normalize(identifier::Kind::Isbn, "978-0-486-23134-1").unwrap(),
            )]
        );
        assert_eq!(input.contributors, [contributor("Erik Satie", "composer")]);
        assert_eq!(input.contents.len(), 1);
        assert_eq!(input.contents[0].title, "Gymnopedie No. 1");
    }
}
