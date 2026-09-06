use std::collections::BTreeSet;

use serde::Deserialize;

use crate::db::publication::{HoldingKind, Publication};
use crate::db::work::{Work, lead_contributor, roles};
use crate::identifier;

/// A publication's editable fields as typed, before validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicationForm {
    pub title: String,
    pub publisher: String,
    pub year: String,
    pub holdings: Vec<HoldingRow>,
    pub identifiers: Vec<IdentifierRow>,
    pub contributors: Vec<ContributorRow>,
    pub works: Vec<WorkRow>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HoldingRow {
    pub id: Option<i64>,
    pub kind: HoldingKind,
    /// Freeform for physical, a file path for digital; empty means none
    pub location: String,
}

impl HoldingRow {
    /// The hidden id field's value, empty for a copy being added.
    pub fn id_value(&self) -> String {
        self.id.map(|id| id.to_string()).unwrap_or_default()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IdentifierRow {
    /// One of the `identifier::Kind` names
    pub kind: String,
    pub value: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContributorRow {
    pub name: String,
    pub role: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkRow {
    /// The stored work this row edits; None for one being added
    pub id: Option<i64>,
    pub title: String,
    /// The work's lead contributor; both fields empty means none
    pub contributor: ContributorRow,
}

impl WorkRow {
    /// The hidden id field's value, empty for a work being added.
    pub fn id_value(&self) -> String {
        self.id.map(|id| id.to_string()).unwrap_or_default()
    }
}

/// A form's typed values, ready to become catalog rows.
#[derive(Debug, PartialEq, Eq)]
pub struct PublicationUpdate {
    pub title: String,
    pub publisher: Option<String>,
    pub year: Option<i64>,
    pub holdings: Vec<HoldingUpdate>,
    pub identifiers: Vec<(identifier::Kind, identifier::Normalized)>,
    pub contributors: Vec<ContributorRow>,
    pub works: Vec<WorkRow>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct HoldingUpdate {
    pub id: Option<i64>,
    pub kind: HoldingKind,
    pub location: Option<String>,
}

/// Problems with a form, one message per field. Empty means it can be submitted.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Errors {
    pub title: Option<String>,
    pub year: Option<String>,
    /// One slot per row, aligned with the form's rows
    pub holdings: Vec<Option<String>>,
    pub no_holdings: Option<String>,
    pub identifiers: Vec<Option<String>>,
    pub contributors: Vec<Option<String>>,
    pub works: Vec<Option<String>>,
}

impl Errors {
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.year.is_none()
            && self.holdings.iter().all(Option::is_none)
            && self.no_holdings.is_none()
            && self.identifiers.iter().all(Option::is_none)
            && self.contributors.iter().all(Option::is_none)
            && self.works.iter().all(Option::is_none)
    }
}

impl PublicationForm {
    /// Check and convert the form. Every problem is reported, not just the first.
    #[expect(clippy::result_large_err)]
    pub fn parse(&self) -> Result<PublicationUpdate, Errors> {
        let mut errors = Errors::default();
        if self.title.is_empty() {
            errors.title = Some("A title is required.".into());
        }
        let year = if self.year.is_empty() {
            None
        } else {
            match self.year.parse::<i64>() {
                Ok(year) => Some(year),
                Err(_) => {
                    errors.year = Some("The year must be a number.".into());
                    None
                }
            }
        };
        let holdings = parse_holdings(&self.holdings, &mut errors);

        let mut identifiers = Vec::new();
        let mut seen = BTreeSet::new();
        errors.identifiers = self
            .identifiers
            .iter()
            .map(|row| {
                if row.value.is_empty() {
                    return Some("Fill this in or remove it.".to_string());
                }
                let kind: identifier::Kind = match row.kind.parse() {
                    Ok(kind) => kind,
                    Err(_) => return Some("Unknown identifier kind.".to_string()),
                };
                let normalized = match identifier::normalize(kind, &row.value) {
                    Ok(normalized) => normalized,
                    Err(e) => return Some(e.to_string()),
                };
                // The schema is unique on (publication, kind, value), so a second row that
                // normalizes to the same value would fail the accept transaction; reject it here
                // instead.
                if !seen.insert((kind.as_str(), normalized.as_str().to_string())) {
                    return Some("Already listed.".to_string());
                }
                identifiers.push((kind, normalized));
                None
            })
            .collect();

        errors.contributors = parse_contributors(&self.contributors);

        errors.works = self
            .works
            .iter()
            .map(|row| {
                let ContributorRow { name, role } = &row.contributor;
                if row.title.is_empty() && name.is_empty() && role.is_empty() {
                    return Some("Fill this in or remove it.".to_string());
                }
                if row.title.is_empty() {
                    return Some("A title is required.".to_string());
                }
                // Both empty is allowed: an anonymous or folk piece has no contributor
                if name.is_empty() && !role.is_empty() {
                    return Some("A name is required.".to_string());
                }
                if role.is_empty() && !name.is_empty() {
                    return Some("A role is required.".to_string());
                }
                None
            })
            .collect();

        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(PublicationUpdate {
            title: self.title.clone(),
            publisher: Some(self.publisher.clone()).filter(|p| !p.is_empty()),
            year,
            holdings,
            identifiers,
            contributors: self.contributors.clone(),
            works: self.works.clone(),
        })
    }
}

/// Check contributor rows, one message per row. A work form credits people the same way a
/// publication form does, so both check their rows here.
pub fn parse_contributors(rows: &[ContributorRow]) -> Vec<Option<String>> {
    let mut seen = BTreeSet::new();
    rows.iter()
        .map(|row| {
            if row.name.is_empty() && row.role.is_empty() {
                return Some("Fill this in or remove it.".to_string());
            }
            if row.name.is_empty() {
                return Some("A name is required.".to_string());
            }
            if row.role.is_empty() {
                return Some("A role is required.".to_string());
            }
            if !seen.insert((row.name.as_str(), row.role.as_str())) {
                return Some("Already listed.".to_string());
            }
            None
        })
        .collect()
}

/// Contributor rows from a submission's parallel keys.
pub fn contributor_rows(name: Vec<String>, role: Vec<String>) -> Vec<ContributorRow> {
    name.into_iter()
        .zip(role)
        .map(|(name, role)| ContributorRow {
            name: name.trim().to_string(),
            role: role.trim().to_string(),
        })
        .collect()
}

/// Check copy rows, filling the holding slots of `errors`
pub fn parse_holdings(rows: &[HoldingRow], errors: &mut Errors) -> Vec<HoldingUpdate> {
    if rows.is_empty() {
        errors.no_holdings = Some("A publication needs at least one copy.".into());
    }
    let mut holdings = Vec::new();
    errors.holdings = rows
        .iter()
        .map(|row| {
            if row.kind == HoldingKind::Digital && row.location.is_empty() {
                return Some("Choose a file for a digital copy.".to_string());
            }
            holdings.push(HoldingUpdate {
                id: row.id,
                kind: row.kind,
                location: Some(row.location.clone()).filter(|l| !l.is_empty()),
            });
            None
        })
        .collect();
    holdings
}

/// A submitted form, as the browser sends it: one repeated key per column, so every row list
/// arrives as parallel arrays. The import review page and the publication edit page post the same
/// shape.
#[derive(Deserialize)]
pub struct Submission {
    title: String,
    publisher: String,
    year: String,
    // `default` covers a submission with no rows at all
    #[serde(default)]
    holding_id: Vec<String>,
    #[serde(default)]
    holding_kind: Vec<HoldingKind>,
    #[serde(default)]
    holding_location: Vec<String>,
    #[serde(default)]
    holding_file: Vec<String>,
    #[serde(default)]
    identifier_kind: Vec<String>,
    #[serde(default)]
    identifier_value: Vec<String>,
    #[serde(default)]
    contributor_name: Vec<String>,
    #[serde(default)]
    contributor_role: Vec<String>,
    #[serde(default)]
    work_id: Vec<String>,
    #[serde(default)]
    work_title: Vec<String>,
    #[serde(default)]
    work_contributor_name: Vec<String>,
    #[serde(default)]
    work_contributor_role: Vec<String>,
}

impl From<Submission> for PublicationForm {
    fn from(submission: Submission) -> Self {
        let identifiers = submission
            .identifier_kind
            .into_iter()
            .zip(submission.identifier_value)
            .map(|(kind, value)| IdentifierRow {
                kind: kind.trim().to_string(),
                value: value.trim().to_string(),
            })
            .collect();
        let contributors =
            contributor_rows(submission.contributor_name, submission.contributor_role);
        // The ids are read by position rather than zipped, as the copy rows do, so a submission
        // with no ids at all still yields rows naming no stored work.
        let works = submission
            .work_title
            .into_iter()
            .zip(submission.work_contributor_name)
            .zip(submission.work_contributor_role)
            .enumerate()
            .map(|(i, ((title, name), role))| WorkRow {
                id: submission
                    .work_id
                    .get(i)
                    .and_then(|id| id.trim().parse().ok()),
                title: title.trim().to_string(),
                contributor: ContributorRow {
                    name: name.trim().to_string(),
                    role: role.trim().to_string(),
                },
            })
            .collect();
        PublicationForm {
            title: submission.title.trim().to_string(),
            publisher: submission.publisher.trim().to_string(),
            year: submission.year.trim().to_string(),
            holdings: holding_rows(
                submission.holding_id,
                submission.holding_kind,
                submission.holding_location,
                submission.holding_file,
            ),
            identifiers,
            contributors,
            works,
        }
    }
}

/// A copy row's kind arrives as a constant "physical" followed by a "digital" when the row's
/// toggle is checked: the toggle is a checkbox, which submits nothing while unchecked, so the
/// constant is what keeps the rows countable.
fn holding_kinds(tokens: Vec<HoldingKind>) -> Vec<HoldingKind> {
    let mut kinds = Vec::new();
    for token in tokens {
        match (token, kinds.last_mut()) {
            (HoldingKind::Digital, Some(last)) => *last = HoldingKind::Digital,
            (token, _) => kinds.push(token),
        }
    }
    kinds
}

/// Copy rows from a submission's parallel keys. Every row submits a location and a file, and the
/// kind picks which one counts.
///
/// The ids are read by position rather than zipped: a missing or short id list leaves the rows it
/// does not reach naming no stored copy, which is what a page with nothing stored yet submits.
pub fn holding_rows(
    id: Vec<String>,
    kind: Vec<HoldingKind>,
    location: Vec<String>,
    file: Vec<String>,
) -> Vec<HoldingRow> {
    holding_kinds(kind)
        .into_iter()
        .zip(location)
        .zip(file)
        .enumerate()
        .map(|(i, ((kind, location), file))| HoldingRow {
            id: id.get(i).and_then(|id| id.trim().parse().ok()),
            kind,
            location: match kind {
                HoldingKind::Physical => location,
                HoldingKind::Digital => file,
            }
            .trim()
            .to_string(),
        })
        .collect()
}

impl PublicationForm {
    /// The form a stored publication opens in: its values as typed, every copy and work naming
    /// itself.
    pub fn stored(publication: &Publication, works: &[Work]) -> Self {
        PublicationForm {
            title: publication.title.clone(),
            publisher: publication.publisher.clone().unwrap_or_default(),
            year: publication.year.map(|y| y.to_string()).unwrap_or_default(),
            holdings: publication
                .holdings
                .iter()
                .map(|h| HoldingRow {
                    id: Some(h.id),
                    kind: h.kind,
                    location: h.location.clone().unwrap_or_default(),
                })
                .collect(),
            identifiers: publication
                .identifiers
                .iter()
                .map(|i| IdentifierRow {
                    kind: i.kind.as_str().to_string(),
                    value: i.value.clone(),
                })
                .collect(),
            contributors: publication
                .contributors
                .iter()
                .map(|c| ContributorRow {
                    name: c.name.clone(),
                    role: c.role.clone(),
                })
                .collect(),
            works: works
                .iter()
                .map(|w| WorkRow {
                    id: Some(w.id),
                    title: w.title.clone(),
                    contributor: lead_contributor(roles(&w.contributors))
                        .map(|i| ContributorRow {
                            name: w.contributors[i].name.clone(),
                            role: w.contributors[i].role.clone(),
                        })
                        .unwrap_or_default(),
                })
                .collect(),
        }
    }
}

/// "Erik Satie" sorts as "Satie, Erik". Compound surnames ("Ralph Vaughan Williams") come out
/// wrong and get fixed on the person page; the heuristic only has to be right often enough that
/// the user rarely types a name twice.
pub fn sort_name(name: &str) -> String {
    let mut parts: Vec<&str> = name.split_whitespace().collect();
    match parts.pop() {
        Some(last) if !parts.is_empty() => format!("{last}, {}", parts.join(" ")),
        _ => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work_row(id: Option<i64>, title: &str, name: &str, role: &str) -> WorkRow {
        WorkRow {
            id,
            title: title.into(),
            contributor: ContributorRow {
                name: name.into(),
                role: role.into(),
            },
        }
    }

    #[test]
    fn form_errors() {
        let form = PublicationForm {
            title: String::new(),
            publisher: String::new(),
            year: "abc".into(),
            holdings: vec![HoldingRow {
                id: None,
                kind: HoldingKind::Digital,
                location: String::new(),
            }],
            identifiers: vec![
                IdentifierRow {
                    kind: "isbn".into(),
                    value: "978-1-4950-0871-0".into(),
                },
                IdentifierRow {
                    kind: "isbn".into(),
                    value: "0-486-23134-8".into(),
                },
                IdentifierRow {
                    kind: "isbn".into(),
                    value: "978-0-486-23134-1".into(),
                },
                IdentifierRow {
                    kind: "isbn".into(),
                    value: String::new(),
                },
            ],
            contributors: vec![
                ContributorRow {
                    name: "Erik Satie".into(),
                    role: String::new(),
                },
                ContributorRow {
                    name: "Erik Satie".into(),
                    role: "composer".into(),
                },
                ContributorRow {
                    name: "Erik Satie".into(),
                    role: "composer".into(),
                },
                ContributorRow {
                    name: String::new(),
                    role: String::new(),
                },
            ],
            works: vec![
                work_row(None, "", "", ""),
                work_row(None, "", "Erik Satie", "composer"),
                work_row(None, "Gnossienne No. 1", "", "composer"),
                work_row(Some(7), "Gnossienne No. 2", "Erik Satie", ""),
            ],
        };
        assert_eq!(
            form.parse().unwrap_err(),
            Errors {
                title: Some("A title is required.".into()),
                year: Some("The year must be a number.".into()),
                holdings: vec![Some("Choose a file for a digital copy.".into())],
                no_holdings: None,
                identifiers: vec![
                    Some("invalid ISBN".into()),
                    None,
                    Some("Already listed.".into()),
                    Some("Fill this in or remove it.".into()),
                ],
                contributors: vec![
                    Some("A role is required.".into()),
                    None,
                    Some("Already listed.".into()),
                    Some("Fill this in or remove it.".into()),
                ],
                works: vec![
                    Some("Fill this in or remove it.".into()),
                    Some("A title is required.".into()),
                    Some("A name is required.".into()),
                    Some("A role is required.".into()),
                ],
            }
        );

        let form = PublicationForm {
            title: "Untitled".into(),
            publisher: String::new(),
            year: String::new(),
            holdings: Vec::new(),
            identifiers: Vec::new(),
            contributors: Vec::new(),
            works: Vec::new(),
        };
        assert_eq!(
            form.parse().unwrap_err(),
            Errors {
                no_holdings: Some("A publication needs at least one copy.".into()),
                ..Errors::default()
            }
        );

        let form = PublicationForm {
            title: "Three gymnopedies".into(),
            publisher: String::new(),
            year: String::new(),
            holdings: vec![HoldingRow {
                id: None,
                kind: HoldingKind::Physical,
                location: String::new(),
            }],
            identifiers: vec![IdentifierRow {
                kind: "isbn".into(),
                value: "0-486-23134-8".into(),
            }],
            contributors: vec![ContributorRow {
                name: "Erik Satie".into(),
                role: "composer".into(),
            }],
            works: vec![
                work_row(Some(3), "Gymnopedie No. 1", "Erik Satie", "composer"),
                // An anonymous piece needs no contributor
                work_row(None, "Trois gnossiennes", "", ""),
            ],
        };
        let validated = form.parse().unwrap();
        assert_eq!(validated.title, "Three gymnopedies");
        assert_eq!(validated.publisher, None);
        assert_eq!(validated.year, None);
        assert_eq!(
            validated.holdings,
            [HoldingUpdate {
                id: None,
                kind: HoldingKind::Physical,
                location: None,
            }]
        );
        assert_eq!(
            validated.identifiers,
            [(
                identifier::Kind::Isbn,
                identifier::normalize(identifier::Kind::Isbn, "0-486-23134-8").unwrap()
            )]
        );
        assert_eq!(
            validated.contributors,
            [ContributorRow {
                name: "Erik Satie".into(),
                role: "composer".into(),
            }]
        );
        assert_eq!(
            validated.works,
            [
                work_row(Some(3), "Gymnopedie No. 1", "Erik Satie", "composer"),
                work_row(None, "Trois gnossiennes", "", ""),
            ]
        );
    }
}
