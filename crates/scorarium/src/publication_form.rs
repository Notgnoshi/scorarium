use std::collections::BTreeSet;

use crate::db::publication::HoldingKind;
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HoldingRow {
    pub id: Option<i64>,
    pub kind: HoldingKind,
    /// Freeform for physical, a file path for digital; empty means none
    pub location: String,
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

/// A form's typed values, ready to become catalog rows.
#[derive(Debug, PartialEq, Eq)]
pub struct Validated {
    pub title: String,
    pub publisher: Option<String>,
    pub year: Option<i64>,
    pub holdings: Vec<ValidatedHolding>,
    pub identifiers: Vec<(identifier::Kind, identifier::Normalized)>,
    pub contributors: Vec<ContributorRow>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ValidatedHolding {
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
}

impl Errors {
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.year.is_none()
            && self.holdings.iter().all(Option::is_none)
            && self.no_holdings.is_none()
            && self.identifiers.iter().all(Option::is_none)
            && self.contributors.iter().all(Option::is_none)
    }
}

impl PublicationForm {
    /// Check and convert the form. Every problem is reported, not just the first.
    #[expect(clippy::result_large_err)]
    pub fn parse(&self) -> Result<Validated, Errors> {
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

        let mut seen = BTreeSet::new();
        errors.contributors = self
            .contributors
            .iter()
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
            .collect();

        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(Validated {
            title: self.title.clone(),
            publisher: Some(self.publisher.clone()).filter(|p| !p.is_empty()),
            year,
            holdings,
            identifiers,
            contributors: self.contributors.clone(),
        })
    }
}

/// Check copy rows, filling the holding slots of `errors`
pub fn parse_holdings(rows: &[HoldingRow], errors: &mut Errors) -> Vec<ValidatedHolding> {
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
            holdings.push(ValidatedHolding {
                id: row.id,
                kind: row.kind,
                location: Some(row.location.clone()).filter(|l| !l.is_empty()),
            });
            None
        })
        .collect();
    holdings
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
            }
        );

        let form = PublicationForm {
            title: "Untitled".into(),
            publisher: String::new(),
            year: String::new(),
            holdings: Vec::new(),
            identifiers: Vec::new(),
            contributors: Vec::new(),
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
        };
        let validated = form.parse().unwrap();
        assert_eq!(validated.title, "Three gymnopedies");
        assert_eq!(validated.publisher, None);
        assert_eq!(validated.year, None);
        assert_eq!(
            validated.holdings,
            [ValidatedHolding {
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
    }
}
