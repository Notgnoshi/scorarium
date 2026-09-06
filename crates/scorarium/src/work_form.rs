use serde::Deserialize;

use crate::db::work::Work;
use crate::publication_form::{self, ContributorRow};

/// A work's editable fields as typed, before validation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkForm {
    pub title: String,
    pub key: String,
    pub time_signature: String,
    pub instrumentation: String,
    pub contributors: Vec<ContributorRow>,
}

/// A form's typed values, ready to be written.
#[derive(Debug, PartialEq, Eq)]
pub struct WorkUpdate {
    pub title: String,
    pub key: Option<String>,
    pub time_signature: Option<String>,
    pub instrumentation: Option<String>,
    pub contributors: Vec<ContributorRow>,
}

/// Problems with a form, one message per field. Empty means it can be submitted.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Errors {
    pub title: Option<String>,
    /// One slot per row, aligned with the form's rows
    pub contributors: Vec<Option<String>>,
}

impl Errors {
    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.contributors.iter().all(Option::is_none)
    }
}

impl WorkForm {
    /// Check and convert the form. Every problem is reported, not just the first.
    ///
    /// The key, the time signature and the instrumentation are free text: they are written in too
    /// many conventions to check, and a work missing one is no less of a work.
    pub fn parse(&self) -> Result<WorkUpdate, Errors> {
        let errors = Errors {
            title: self
                .title
                .is_empty()
                .then(|| "A title is required.".to_string()),
            contributors: publication_form::parse_contributors(&self.contributors),
        };
        if !errors.is_empty() {
            return Err(errors);
        }
        let optional = |value: &str| Some(value.to_string()).filter(|v| !v.is_empty());
        Ok(WorkUpdate {
            title: self.title.clone(),
            key: optional(&self.key),
            time_signature: optional(&self.time_signature),
            instrumentation: optional(&self.instrumentation),
            contributors: self.contributors.clone(),
        })
    }

    /// The form a stored work opens in: its values as typed, with a row per contributor. Unlike a
    /// work row on a publication form, this page shows and edits all of them.
    pub fn stored(work: &Work) -> Self {
        WorkForm {
            title: work.title.clone(),
            key: work.key.clone().unwrap_or_default(),
            time_signature: work.time_signature.clone().unwrap_or_default(),
            instrumentation: work.instrumentation.clone().unwrap_or_default(),
            contributors: work
                .contributors
                .iter()
                .map(|c| ContributorRow {
                    name: c.name.clone(),
                    role: c.role.clone(),
                })
                .collect(),
        }
    }
}

/// A submitted form, as the browser sends it. The stored work edit page and the draft work page
/// post the same shape.
#[derive(Deserialize)]
pub struct Submission {
    title: String,
    key: String,
    time_signature: String,
    instrumentation: String,
    // `default` covers a submission with no rows at all
    #[serde(default)]
    contributor_name: Vec<String>,
    #[serde(default)]
    contributor_role: Vec<String>,
}

impl From<Submission> for WorkForm {
    fn from(submission: Submission) -> Self {
        WorkForm {
            title: submission.title.trim().to_string(),
            key: submission.key.trim().to_string(),
            time_signature: submission.time_signature.trim().to_string(),
            instrumentation: submission.instrumentation.trim().to_string(),
            contributors: publication_form::contributor_rows(
                submission.contributor_name,
                submission.contributor_role,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contributor(name: &str, role: &str) -> ContributorRow {
        ContributorRow {
            name: name.into(),
            role: role.into(),
        }
    }

    #[test]
    fn form_errors() {
        let form = WorkForm {
            title: String::new(),
            key: String::new(),
            time_signature: String::new(),
            instrumentation: String::new(),
            contributors: vec![
                contributor("Erik Satie", ""),
                contributor("Erik Satie", "composer"),
                contributor("Erik Satie", "composer"),
                contributor("", ""),
            ],
        };
        assert_eq!(
            form.parse().unwrap_err(),
            Errors {
                title: Some("A title is required.".into()),
                contributors: vec![
                    Some("A role is required.".into()),
                    None,
                    Some("Already listed.".into()),
                    Some("Fill this in or remove it.".into()),
                ],
            }
        );

        let form = WorkForm {
            title: "Gnossienne No. 1".into(),
            key: String::new(),
            time_signature: "3/4".into(),
            instrumentation: "piano".into(),
            contributors: vec![contributor("Erik Satie", "composer")],
        };
        assert_eq!(
            form.parse().unwrap(),
            WorkUpdate {
                title: "Gnossienne No. 1".into(),
                // An empty field is no value at all, not an empty one
                key: None,
                time_signature: Some("3/4".into()),
                instrumentation: Some("piano".into()),
                contributors: vec![contributor("Erik Satie", "composer")],
            }
        );
    }
}
