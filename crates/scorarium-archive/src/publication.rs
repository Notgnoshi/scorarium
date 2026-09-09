use std::collections::HashMap;
use std::sync::Arc;

use comparable::{Changed, Comparable};
use sqlx::SqliteConnection;

use crate::holding::{self, Holding, HoldingErrors, HoldingInput, HoldingRawInput};
use crate::identifier::{self, Identifier, IdentifierRawInput};
use crate::input::{self, ContributorInput, ValidationError};
use crate::person::{self, Contributor};
use crate::work::{self, Work, WorkErrors, WorkInput, WorkRawInput};
use crate::{Action, ArchiveInner, EntityKind, EntityRef, Event, Field, NotFound, Source, library};

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
        let holdings = match holding::parse_holdings(&self.holdings) {
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
            publisher: input::trimmed_or_none(&self.publisher),
            year,
            holdings,
            identifiers,
            contributors,
            contents,
        })
    }
}

/// A publication with its children
#[derive(Clone, Debug, Comparable)]
pub struct Publication {
    #[comparable_ignore]
    pub id: i64,
    #[comparable_ignore]
    pub library_id: i64,
    pub title: String,
    pub publisher: Option<String>,
    pub year: Option<i64>,
    pub identifiers: Vec<Identifier>,
    /// In link order
    pub contributors: Vec<Contributor>,
    pub holdings: Vec<Holding>,
    #[comparable_ignore]
    archive: Arc<ArchiveInner>,
}

/// Which fields an edit changed, for the audit log.
fn changed_fields(old: &Publication, new: &Publication) -> Vec<Field> {
    let Changed::Changed(changes) = old.comparison(new) else {
        return Vec::new();
    };
    changes
        .iter()
        .map(|change| match change {
            PublicationChange::Title(_) => Field::Title,
            PublicationChange::Publisher(_) => Field::Publisher,
            PublicationChange::Year(_) => Field::Year,
            PublicationChange::Identifiers(_) => Field::Identifiers,
            PublicationChange::Contributors(_) => Field::Contributors,
            PublicationChange::Holdings(_) => Field::Holdings,
        })
        .collect()
}

impl Publication {
    /// The works this publication contains, in the order they were added to it
    pub async fn works(&self) -> crate::Result<Vec<Work>> {
        let mut tx = self.archive.begin_read().await?;
        let contents =
            work::load_works(&self.archive, &mut tx, self.library_id, None, Some(self.id)).await?;
        tx.commit().await?;
        Ok(contents)
    }

    /// What the publication's edit page opens with
    ///
    /// The contents are the ones [Publication::works] returned. A page showing only part of a
    /// work still hands the whole work back, so it belongs in the raw input either way.
    pub fn raw_input(&self, contents: &[Work]) -> PublicationRawInput {
        PublicationRawInput {
            title: self.title.clone(),
            publisher: self.publisher.clone().unwrap_or_default(),
            year: self.year.map(|year| year.to_string()).unwrap_or_default(),
            holdings: self
                .holdings
                .iter()
                .map(|holding| HoldingRawInput {
                    id: Some(holding.id),
                    kind: holding.kind,
                    location: holding.location.clone().unwrap_or_default(),
                })
                .collect(),
            identifiers: self
                .identifiers
                .iter()
                .map(|identifier| IdentifierRawInput {
                    kind: identifier.kind.as_str().to_string(),
                    value: identifier.value.clone(),
                })
                .collect(),
            contributors: self
                .contributors
                .iter()
                .map(|contributor| ContributorInput {
                    name: contributor.name.clone(),
                    role: contributor.role.clone(),
                })
                .collect(),
            contents: contents.iter().map(Work::raw_input).collect(),
        }
    }

    /// The roles one person is credited with, for pages about that person
    pub fn roles_of(&self, person_id: i64) -> Vec<&str> {
        self.contributors
            .iter()
            .filter(|contributor| contributor.person_id == person_id)
            .map(|contributor| contributor.role.as_str())
            .collect()
    }

    /// Apply an edited input, then collect whatever the edit left credited nowhere.
    ///
    /// Returns a [NotFound] error if the publication has since been deleted.
    pub async fn update(&mut self, input: &PublicationInput) -> crate::Result<()> {
        let mut audited = self
            .archive
            .begin_audit(
                Source::User,
                Event::about(Action::Updated, self.entity_ref()),
            )
            .await?;
        let result = sqlx::query!(
            "UPDATE publication SET title = ?, publisher = ?, year = ?
             WHERE library_id = ? AND id = ?",
            input.title,
            input.publisher,
            input.year,
            self.library_id,
            self.id
        )
        .execute(&mut *audited)
        .await?;
        if result.rows_affected() == 0 {
            audited.rollback().await?;
            return Err(NotFound.into());
        }
        let contents_before = contained_work_ids(&mut audited, self.id).await?;
        write_publication_children(&mut audited, self.library_id, self.id, input).await?;
        work::write_publication_works(&mut audited, self.library_id, self.id, &input.contents)
            .await?;
        let contents_after = contained_work_ids(&mut audited, self.id).await?;
        library::collect_orphans(&mut audited, self.library_id).await?;
        let reloaded = load_publications(
            &self.archive,
            &mut audited,
            self.library_id,
            Some(self.id),
            None,
            None,
        )
        .await?
        .pop()
        .expect("the publication was just updated on this transaction");
        // The contents are not a field of self, so the derived comparison cannot see them
        let mut fields = changed_fields(self, &reloaded);
        if contents_before != contents_after {
            fields.push(Field::Contents);
        }
        audited.set_fields(&fields).await?;
        audited.commit().await?;
        *self = reloaded;
        Ok(())
    }

    /// Delete the publication, then cleanup any orphans the deletion resulted in.
    ///
    /// Returns a [NotFound] error if the publication has since been deleted.
    pub async fn delete(self) -> crate::Result<()> {
        let mut audited = self
            .archive
            .begin_audit(
                Source::User,
                Event::about(Action::Deleted, self.entity_ref()),
            )
            .await?;
        let result = sqlx::query!(
            "DELETE FROM publication WHERE library_id = ? AND id = ?",
            self.library_id,
            self.id
        )
        .execute(&mut *audited)
        .await?;
        if result.rows_affected() == 0 {
            audited.rollback().await?;
            return Err(NotFound.into());
        }
        library::collect_orphans(&mut audited, self.library_id).await?;
        audited.commit().await?;
        Ok(())
    }

    /// Get an EntityRef referring to this entity for use in the audit log
    pub(crate) fn entity_ref(&self) -> EntityRef {
        EntityRef {
            kind: EntityKind::Publication,
            id: self.id,
            library_id: Some(self.library_id),
            label: self.title.clone(),
        }
    }
}

/// The ids of the works a publication contains, in link order, on the caller's transaction.
///
/// [Publication::works] opens its own transaction and so would read a different snapshot.
async fn contained_work_ids(
    conn: &mut SqliteConnection,
    publication_id: i64,
) -> crate::Result<Vec<i64>> {
    let ids = sqlx::query_scalar!(
        "SELECT work_id FROM publication_work WHERE publication_id = ? ORDER BY id",
        publication_id
    )
    .fetch_all(conn)
    .await?;
    Ok(ids)
}

/// Load a library's publications with their children: all of them, just the one with `id`, those
/// containing `work_id`, or those crediting `person_id` directly or through a contained work.
///
/// The four reads run on the caller's transaction so they see one snapshot. Otherwise a child row
/// of a publication created between the parent read and the child reads would have no parent here.
pub(crate) async fn load_publications(
    shared: &Arc<ArchiveInner>,
    conn: &mut SqliteConnection,
    library_id: i64,
    id: Option<i64>,
    work_id: Option<i64>,
    person_id: Option<i64>,
) -> crate::Result<Vec<Publication>> {
    let mut publications: Vec<Publication> = sqlx::query!(
        "SELECT id, library_id, title, publisher, year FROM publication
         WHERE library_id = ?1
           AND (?2 IS NULL OR id = ?2)
           AND (?3 IS NULL OR id IN (SELECT publication_id FROM publication_work WHERE work_id = ?3))
           AND (?4 IS NULL
                OR id IN (SELECT publication_id FROM publication_contributor WHERE person_id = ?4)
                OR id IN (SELECT pw.publication_id FROM publication_work pw
                          JOIN work_contributor wc ON wc.work_id = pw.work_id
                          WHERE wc.person_id = ?4))",
        library_id,
        id,
        work_id,
        person_id
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| Publication {
        id: row.id,
        library_id: row.library_id,
        title: row.title,
        publisher: row.publisher,
        year: row.year,
        identifiers: Vec::new(),
        contributors: Vec::new(),
        holdings: Vec::new(),
        archive: shared.clone(),
    })
    .collect();
    let index: HashMap<i64, usize> = publications
        .iter()
        .enumerate()
        .map(|(i, publication)| (publication.id, i))
        .collect();

    let identifiers = sqlx::query!(
        "SELECT id, publication_id, kind, value FROM publication_identifier
         WHERE publication_id IN
            (SELECT id FROM publication
             WHERE library_id = ?1
               AND (?2 IS NULL OR id = ?2)
               AND (?3 IS NULL OR id IN (SELECT publication_id FROM publication_work WHERE work_id = ?3))
               AND (?4 IS NULL
                    OR id IN (SELECT publication_id FROM publication_contributor WHERE person_id = ?4)
                    OR id IN (SELECT pw.publication_id FROM publication_work pw
                              JOIN work_contributor wc ON wc.work_id = pw.work_id
                              WHERE wc.person_id = ?4)))
         ORDER BY id",
        library_id,
        id,
        work_id,
        person_id
    )
    .fetch_all(&mut *conn)
    .await?;
    for row in identifiers {
        let kind = row.kind.parse().map_err(eyre::Report::msg)?;
        publications[index[&row.publication_id]]
            .identifiers
            .push(Identifier {
                id: row.id,
                kind,
                value: row.value,
            });
    }

    let contributors = sqlx::query!(
        "SELECT c.publication_id, c.person_id, p.name, c.role
         FROM publication_contributor c JOIN person p ON p.id = c.person_id
         WHERE c.publication_id IN
            (SELECT id FROM publication
             WHERE library_id = ?1
               AND (?2 IS NULL OR id = ?2)
               AND (?3 IS NULL OR id IN (SELECT publication_id FROM publication_work WHERE work_id = ?3))
               AND (?4 IS NULL
                    OR id IN (SELECT publication_id FROM publication_contributor WHERE person_id = ?4)
                    OR id IN (SELECT pw.publication_id FROM publication_work pw
                              JOIN work_contributor wc ON wc.work_id = pw.work_id
                              WHERE wc.person_id = ?4)))
         ORDER BY c.id",
        library_id,
        id,
        work_id,
        person_id
    )
    .fetch_all(&mut *conn)
    .await?;
    for row in contributors {
        publications[index[&row.publication_id]]
            .contributors
            .push(Contributor {
                person_id: row.person_id,
                name: row.name,
                role: row.role,
            });
    }

    let holdings = sqlx::query!(
        "SELECT id, publication_id, kind, location FROM holding
         WHERE publication_id IN
            (SELECT id FROM publication
             WHERE library_id = ?1
               AND (?2 IS NULL OR id = ?2)
               AND (?3 IS NULL OR id IN (SELECT publication_id FROM publication_work WHERE work_id = ?3))
               AND (?4 IS NULL
                    OR id IN (SELECT publication_id FROM publication_contributor WHERE person_id = ?4)
                    OR id IN (SELECT pw.publication_id FROM publication_work pw
                              JOIN work_contributor wc ON wc.work_id = pw.work_id
                              WHERE wc.person_id = ?4)))
         ORDER BY id",
        library_id,
        id,
        work_id,
        person_id
    )
    .fetch_all(&mut *conn)
    .await?;
    for row in holdings {
        let kind = row.kind.parse().map_err(eyre::Report::msg)?;
        publications[index[&row.publication_id]]
            .holdings
            .push(Holding {
                id: row.id,
                kind,
                location: row.location,
            });
    }

    Ok(publications)
}

/// Create a publication and everything it names, on the caller's transaction.
///
/// Every work of the input is created, ids and all ignored: linking an existing work into another
/// publication is not something the input can ask for yet.
pub(crate) async fn create_publication(
    shared: &Arc<ArchiveInner>,
    conn: &mut SqliteConnection,
    library_id: i64,
    input: &PublicationInput,
) -> crate::Result<Publication> {
    let created = sqlx::query!(
        "INSERT INTO publication (library_id, title, publisher, year) VALUES (?, ?, ?, ?)",
        library_id,
        input.title,
        input.publisher,
        input.year,
    )
    .execute(&mut *conn)
    .await?;
    let id = created.last_insert_rowid();
    write_publication_children(&mut *conn, library_id, id, input).await?;
    for content in &input.contents {
        work::create_work_in_publication(&mut *conn, library_id, id, content).await?;
    }
    let publication = load_publications(shared, conn, library_id, Some(id), None, None)
        .await?
        .pop()
        .expect("the publication was just created on this transaction");
    Ok(publication)
}

/// Write a publication's identifiers, contributor links and holdings.
///
/// What it leaves out is the contents, since a publication being created writes its works in full
/// while one being edited reconciles them against what is stored.
pub(crate) async fn write_publication_children(
    conn: &mut SqliteConnection,
    library_id: i64,
    publication_id: i64,
    input: &PublicationInput,
) -> crate::Result<()> {
    identifier::write_identifiers(&mut *conn, publication_id, &input.identifiers).await?;
    write_publication_contributors(&mut *conn, library_id, publication_id, &input.contributors)
        .await?;
    holding::write_holdings(&mut *conn, publication_id, &input.holdings).await?;
    Ok(())
}

/// Rebuild a publication's contributor links, so input order becomes link order.
///
/// A link holds nothing beyond what the input shows, so rebuilding outright loses nothing.
async fn write_publication_contributors(
    conn: &mut SqliteConnection,
    library_id: i64,
    publication_id: i64,
    contributors: &[ContributorInput],
) -> crate::Result<()> {
    sqlx::query!(
        "DELETE FROM publication_contributor WHERE publication_id = ?",
        publication_id
    )
    .execute(&mut *conn)
    .await?;
    for contributor in contributors {
        let person_id =
            person::find_or_create_person(&mut *conn, library_id, &contributor.name).await?;
        sqlx::query!(
            "INSERT INTO publication_contributor (library_id, publication_id, person_id, role)
             VALUES (?, ?, ?, ?)",
            library_id,
            publication_id,
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
    use crate::holding::HoldingKind;

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
                        catalog_numbers: Vec::new(),
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
