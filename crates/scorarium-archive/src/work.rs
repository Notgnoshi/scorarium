use sqlx::SqliteConnection;

use crate::input::{self, ContributorInput, ValidationError};
use crate::person;

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
            key: input::trimmed_or_none(&self.key),
            time_signature: input::trimmed_or_none(&self.time_signature),
            instrumentation: input::trimmed_or_none(&self.instrumentation),
            contributors,
        })
    }
}

/// Create a work and put it in a publication, on the caller's transaction.
///
/// The input's id is ignored: a publication creates every work it names, since linking an existing
/// work into another publication is not something the input can ask for yet.
pub(crate) async fn create_work_in_publication(
    conn: &mut SqliteConnection,
    library_id: i64,
    publication_id: i64,
    input: &WorkInput,
) -> crate::Result<i64> {
    let created = sqlx::query!(
        "INSERT INTO work (library_id, title, \"key\", time_signature, instrumentation) VALUES (?, ?, ?, ?, ?)",
        library_id,
        input.title,
        input.key,
        input.time_signature,
        input.instrumentation,
    )
    .execute(&mut *conn)
    .await?;
    let id = created.last_insert_rowid();
    sqlx::query!(
        "INSERT INTO publication_work (library_id, publication_id, work_id) VALUES (?, ?, ?)",
        library_id,
        publication_id,
        id,
    )
    .execute(&mut *conn)
    .await?;
    write_work_contributors(&mut *conn, library_id, id, &input.contributors).await?;
    Ok(id)
}

/// Rebuild a work's contributor links, so input order becomes link order.
///
/// A link holds nothing beyond what the input shows, so rebuilding outright loses nothing. On a
/// work with no links yet the delete finds nothing, so creating and updating share this.
pub(crate) async fn write_work_contributors(
    conn: &mut SqliteConnection,
    library_id: i64,
    work_id: i64,
    contributors: &[ContributorInput],
) -> crate::Result<()> {
    sqlx::query!("DELETE FROM work_contributor WHERE work_id = ?", work_id)
        .execute(&mut *conn)
        .await?;
    for contributor in contributors {
        let person_id =
            person::find_or_create_person(&mut *conn, library_id, &contributor.name).await?;
        sqlx::query!(
            "INSERT INTO work_contributor (library_id, work_id, person_id, role) VALUES (?, ?, ?, ?)",
            library_id,
            work_id,
            person_id,
            contributor.role,
        )
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
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
