use crate::input::{self, ContributorInput, ValidationError};

/// A work's editable fields as entered from the web forms
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkRawInput {
    /// The work this edits, or a draft work's id; None for one being added
    pub id: Option<i64>,
    pub title: String,
    pub key: String,
    pub time_signature: String,
    pub instrumentation: String,
    pub contributors: Vec<ContributorInput>,
}

/// A work's parsed and validated fields
#[derive(Debug, PartialEq, Eq)]
pub struct WorkInput {
    pub(crate) id: Option<i64>,
    pub(crate) title: String,
    pub(crate) key: Option<String>,
    pub(crate) time_signature: Option<String>,
    pub(crate) instrumentation: Option<String>,
    pub(crate) contributors: Vec<ContributorInput>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct WorkErrors {
    pub title: Option<ValidationError>,
    /// One slot per contributor, empty when they all passed
    pub contributors: Vec<Option<ValidationError>>,
}

impl WorkErrors {
    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.contributors.iter().all(Option::is_none)
    }
}

impl WorkRawInput {
    /// Check and convert the input
    ///
    /// The key, the time signature and the instrumentation are free text
    pub fn parse(&self) -> Result<WorkInput, WorkErrors> {
        let title = self.title.trim();
        let mut errors = WorkErrors {
            title: title.is_empty().then_some(ValidationError::TitleRequired),
            contributors: Vec::new(),
        };
        let contributors = match input::parse_contributors(&self.contributors) {
            Ok(contributors) => contributors,
            Err(slots) => {
                errors.contributors = slots;
                Vec::new()
            }
        };
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(WorkInput {
            id: self.id,
            title: title.to_string(),
            key: input::optional(&self.key),
            time_signature: input::optional(&self.time_signature),
            instrumentation: input::optional(&self.instrumentation),
            contributors,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contributor(name: &str, role: &str) -> ContributorInput {
        ContributorInput {
            name: name.into(),
            role: role.into(),
        }
    }

    #[test]
    fn parse_reports_every_problem() {
        let raw = WorkRawInput {
            contributors: vec![
                contributor("Erik Satie", ""),
                contributor("Erik Satie", "composer"),
                contributor("Erik Satie", "composer"),
                contributor("", ""),
            ],
            ..WorkRawInput::default()
        };
        assert_eq!(
            raw.parse().unwrap_err(),
            WorkErrors {
                title: Some(ValidationError::TitleRequired),
                contributors: vec![
                    Some(ValidationError::RoleRequired),
                    None,
                    Some(ValidationError::AlreadyListed),
                    Some(ValidationError::FillOrRemove),
                ],
            }
        );
    }

    #[test]
    fn parse_takes_what_was_typed() {
        let raw = WorkRawInput {
            id: Some(7),
            title: "  Gnossienne No. 1  ".into(),
            key: String::new(),
            time_signature: "3/4".into(),
            instrumentation: "piano".into(),
            contributors: vec![contributor(" Erik Satie ", "composer")],
        };
        assert_eq!(
            raw.parse().unwrap(),
            WorkInput {
                id: Some(7),
                title: "Gnossienne No. 1".into(),
                // A field left blank is no value at all, not an empty one
                key: None,
                time_signature: Some("3/4".into()),
                instrumentation: Some("piano".into()),
                contributors: vec![contributor("Erik Satie", "composer")],
            }
        );
    }
}
