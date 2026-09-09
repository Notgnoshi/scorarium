use std::collections::HashMap;
use std::sync::Arc;

use comparable::{Changed, Comparable};
use sqlx::SqliteConnection;

use crate::catalog::CatalogNumber;
use crate::input::{self, ContributorInput, ValidationError};
use crate::person::{self, Contributor};
use crate::publication::{self, Publication};
use crate::{Action, ArchiveInner, EntityKind, EntityRef, Event, Field, NotFound, Source, library};

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
    pub catalog_numbers: Vec<String>,
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
    pub(crate) catalog_numbers: Vec<CatalogNumber>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct WorkErrors {
    pub title: Option<ValidationError>,
    /// One slot per contributor, empty when they all passed
    pub contributors: Vec<Option<ValidationError>>,
    /// One slot per catalog number, empty when they all passed
    pub catalog_numbers: Vec<Option<ValidationError>>,
}

impl WorkErrors {
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.contributors.iter().all(Option::is_none)
            && self.catalog_numbers.iter().all(Option::is_none)
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
            catalog_numbers: Vec::new(),
        };
        let contributors = match input::parse_contributors(&self.contributors) {
            Ok(contributors) => contributors,
            Err(slots) => {
                errors.contributors = slots;
                Vec::new()
            }
        };
        let catalog_numbers = match input::parse_catalog_numbers(&self.catalog_numbers) {
            Ok(numbers) => numbers,
            Err(slots) => {
                errors.catalog_numbers = slots;
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
            catalog_numbers,
        })
    }
}

/// A work with its children, as read back
#[derive(Clone, Debug, Comparable)]
pub struct Work {
    #[comparable_ignore]
    pub id: i64,
    #[comparable_ignore]
    pub library_id: i64,
    pub title: String,
    pub key: Option<String>,
    pub time_signature: Option<String>,
    pub instrumentation: Option<String>,
    /// In the order they were entered
    pub catalog_numbers: Vec<CatalogNumber>,
    /// In link order
    pub contributors: Vec<Contributor>,
    #[comparable_ignore]
    archive: Arc<ArchiveInner>,
}

/// Which fields an edit changed, for the audit log.
fn changed_fields(old: &Work, new: &Work) -> Vec<Field> {
    let Changed::Changed(changes) = old.comparison(new) else {
        return Vec::new();
    };
    changes
        .iter()
        .map(|change| match change {
            WorkChange::Title(_) => Field::Title,
            WorkChange::Key(_) => Field::Key,
            WorkChange::TimeSignature(_) => Field::TimeSignature,
            WorkChange::Instrumentation(_) => Field::Instrumentation,
            WorkChange::CatalogNumbers(_) => Field::CatalogNumbers,
            WorkChange::Contributors(_) => Field::Contributors,
        })
        .collect()
}

impl Work {
    /// The publications containing this work, in arbitrary order
    pub async fn publications(&self) -> crate::Result<Vec<Publication>> {
        let mut tx = self.archive.begin_read().await?;
        let publications = publication::load_publications(
            &self.archive,
            &mut tx,
            self.library_id,
            None,
            Some(self.id),
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(publications)
    }

    /// What the work's edit page opens with
    pub fn raw_input(&self) -> WorkRawInput {
        WorkRawInput {
            id: Some(self.id),
            title: self.title.clone(),
            key: self.key.clone().unwrap_or_default(),
            time_signature: self.time_signature.clone().unwrap_or_default(),
            instrumentation: self.instrumentation.clone().unwrap_or_default(),
            contributors: self
                .contributors
                .iter()
                .map(|contributor| ContributorInput {
                    name: contributor.name.clone(),
                    role: contributor.role.clone(),
                })
                .collect(),
            catalog_numbers: self
                .catalog_numbers
                .iter()
                .map(|number| number.as_str().to_string())
                .collect(),
        }
    }

    /// The contributors credited with `role`, for listings with one column per role
    pub fn with_role(&self, role: &str) -> Vec<&Contributor> {
        self.contributors
            .iter()
            .filter(|contributor| contributor.role == role)
            .collect()
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
    /// One transaction: the work's fields, its contributors rebuilt in input order, a merge into
    /// an older work the edit collided with, orphan collection, and a reload of self. After a
    /// merge, self is the survivor, so its id differs from the work that was edited.
    ///
    /// Returns a [NotFound] error if the work has since been collected.
    pub async fn update(&mut self, input: &WorkInput) -> crate::Result<()> {
        let mut audited = self
            .archive
            .begin_audit(
                Source::User,
                Event::about(Action::Updated, self.entity_ref()),
            )
            .await?;
        let result = sqlx::query!(
            "UPDATE work SET title = ?, \"key\" = ?, time_signature = ?, instrumentation = ?
             WHERE library_id = ? AND id = ?",
            input.title,
            input.key,
            input.time_signature,
            input.instrumentation,
            self.library_id,
            self.id
        )
        .execute(&mut *audited)
        .await?;
        if result.rows_affected() == 0 {
            audited.rollback().await?;
            return Err(NotFound.into());
        }
        write_work_contributors(&mut audited, self.library_id, self.id, &input.contributors)
            .await?;
        write_work_catalog_numbers(&mut audited, self.id, &input.catalog_numbers).await?;
        // Before the merge, or the comparison is against whichever work absorbed this one
        let edited = load_works(
            &self.archive,
            &mut audited,
            self.library_id,
            Some(self.id),
            None,
        )
        .await?
        .pop()
        .expect("the work was just updated on this transaction");
        audited.set_fields(&changed_fields(self, &edited)).await?;
        let survivor = absorb_into_duplicate(&mut audited, self.library_id, self.id).await?;
        library::collect_orphans(&mut audited, self.library_id).await?;
        let reloaded = load_works(
            &self.archive,
            &mut audited,
            self.library_id,
            Some(survivor),
            None,
        )
        .await?
        .pop()
        .expect("the work was just updated on this transaction");
        audited.commit().await?;
        *self = reloaded;
        Ok(())
    }

    /// Get an EntityRef referring to this entity for use in the audit log
    pub(crate) fn entity_ref(&self) -> EntityRef {
        EntityRef {
            kind: EntityKind::Work,
            id: self.id,
            library_id: Some(self.library_id),
            label: self.title.clone(),
        }
    }
}

/// Load a library's works with their children: the one with `id`, or those a publication contains.
///
/// The three reads run on the caller's transaction so they see one snapshot. A publication's
/// contents come back in the order the works were added to it.
pub(crate) async fn load_works(
    shared: &Arc<ArchiveInner>,
    conn: &mut SqliteConnection,
    library_id: i64,
    id: Option<i64>,
    publication_id: Option<i64>,
) -> crate::Result<Vec<Work>> {
    let mut works: Vec<Work> = sqlx::query!(
        "SELECT id, library_id, title, \"key\", time_signature, instrumentation FROM work
         WHERE library_id = ?1
           AND (?2 IS NULL OR id = ?2)
           AND (?3 IS NULL OR id IN (SELECT work_id FROM publication_work WHERE publication_id = ?3))
         ORDER BY (SELECT id FROM publication_work WHERE work_id = work.id AND publication_id = ?3)",
        library_id,
        id,
        publication_id
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| Work {
        id: row.id,
        library_id: row.library_id,
        title: row.title,
        key: row.key,
        time_signature: row.time_signature,
        instrumentation: row.instrumentation,
        catalog_numbers: Vec::new(),
        contributors: Vec::new(),
        archive: shared.clone(),
    })
    .collect();
    let index: HashMap<i64, usize> = works
        .iter()
        .enumerate()
        .map(|(i, work)| (work.id, i))
        .collect();

    let catalog_numbers = sqlx::query!(
        "SELECT work_id, value FROM work_catalog_number
         WHERE work_id IN
            (SELECT id FROM work
             WHERE library_id = ?1
               AND (?2 IS NULL OR id = ?2)
               AND (?3 IS NULL OR id IN (SELECT work_id FROM publication_work WHERE publication_id = ?3)))
         ORDER BY id",
        library_id,
        id,
        publication_id
    )
    .fetch_all(&mut *conn)
    .await?;
    for row in catalog_numbers {
        works[index[&row.work_id]]
            .catalog_numbers
            .push(CatalogNumber::parse(&row.value));
    }

    let contributors = sqlx::query!(
        "SELECT c.work_id, c.person_id, p.name, c.role
         FROM work_contributor c JOIN person p ON p.id = c.person_id
         WHERE c.work_id IN
            (SELECT id FROM work
             WHERE library_id = ?1
               AND (?2 IS NULL OR id = ?2)
               AND (?3 IS NULL OR id IN (SELECT work_id FROM publication_work WHERE publication_id = ?3)))
         ORDER BY c.id",
        library_id,
        id,
        publication_id
    )
    .fetch_all(&mut *conn)
    .await?;
    for row in contributors {
        works[index[&row.work_id]].contributors.push(Contributor {
            person_id: row.person_id,
            name: row.name,
            role: row.role,
        });
    }

    Ok(works)
}

/// One work's catalog number, with what a suggestion needs to say about the work carrying it
#[derive(Clone, Debug)]
pub struct CatalogNumberEntry {
    pub library_id: i64,
    pub library_name: String,
    pub work_id: i64,
    pub title: String,
    /// The composer credited, if any; a work with several appears once per composer
    pub composer: Option<String>,
    pub number: CatalogNumber,
}

/// Every catalog number in a library or in the whole archive, by library, work title, and the
/// order the numbers were entered.
///
/// `public_only` leaves out works no non-private publication reaches, for the unauthenticated
/// suggestion route.
pub(crate) async fn load_catalog_numbers(
    conn: &mut SqliteConnection,
    library_id: Option<i64>,
    composer: Option<&str>,
    public_only: bool,
) -> crate::Result<Vec<CatalogNumberEntry>> {
    let rows = sqlx::query!(
        "SELECT l.id AS library_id, l.name AS library_name, w.id AS work_id, w.title,
                p.name AS composer, cn.value
         FROM work_catalog_number cn
         JOIN work w ON w.id = cn.work_id
         JOIN library l ON l.id = w.library_id
         LEFT JOIN work_contributor c ON c.work_id = w.id AND c.role = 'composer'
         LEFT JOIN person p ON p.id = c.person_id
         WHERE (?1 IS NULL OR w.library_id = ?1)
           AND (?2 IS NULL OR p.name = ?2)
           AND (?3 = 0 OR EXISTS (SELECT 1 FROM publication_work pw
                                  JOIN publication pub ON pub.id = pw.publication_id
                                  WHERE pw.work_id = w.id AND pub.private = 0))
         ORDER BY l.name, w.title, cn.id",
        library_id,
        composer,
        public_only
    )
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| CatalogNumberEntry {
            library_id: row.library_id,
            library_name: row.library_name,
            work_id: row.work_id,
            title: row.title,
            composer: row.composer,
            number: CatalogNumber::parse(&row.value),
        })
        .collect())
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
    write_work_catalog_numbers(&mut *conn, id, &input.catalog_numbers).await?;
    absorb_into_duplicate(&mut *conn, library_id, id).await
}

/// Reconcile a publication's contents against the works its input names.
///
/// An input whose id the publication already contains edits that work in place, fields and credits
/// alike; any other input creates a work. Works the input no longer names are unlinked rather than
/// deleted; cleanup is handled by orphan cleanup on the library. Existing links keep their
/// position, so reordering the input does not reorder the contents.
pub(crate) async fn write_publication_works(
    conn: &mut SqliteConnection,
    library_id: i64,
    publication_id: i64,
    contents: &[WorkInput],
) -> crate::Result<()> {
    let stored = sqlx::query_scalar!(
        "SELECT work_id FROM publication_work WHERE publication_id = ?",
        publication_id
    )
    .fetch_all(&mut *conn)
    .await?;
    let named: Vec<i64> = contents.iter().filter_map(|work| work.id).collect();
    for work_id in stored.iter().filter(|id| !named.contains(id)) {
        sqlx::query!(
            "DELETE FROM publication_work WHERE publication_id = ? AND work_id = ?",
            publication_id,
            work_id
        )
        .execute(&mut *conn)
        .await?;
    }
    for input in contents {
        // An id the publication does not contain names nothing this input may edit
        let Some(work_id) = input.id.filter(|id| stored.contains(id)) else {
            create_work_in_publication(&mut *conn, library_id, publication_id, input).await?;
            continue;
        };
        sqlx::query!(
            "UPDATE work SET title = ?, \"key\" = ?, time_signature = ?, instrumentation = ?
             WHERE library_id = ? AND id = ?",
            input.title,
            input.key,
            input.time_signature,
            input.instrumentation,
            library_id,
            work_id
        )
        .execute(&mut *conn)
        .await?;
        write_work_contributors(&mut *conn, library_id, work_id, &input.contributors).await?;
        write_work_catalog_numbers(&mut *conn, work_id, &input.catalog_numbers).await?;
        absorb_into_duplicate(&mut *conn, library_id, work_id).await?;
    }
    Ok(())
}

/// Put an existing work into another publication.
pub(crate) async fn link_work_to_publication(
    conn: &mut SqliteConnection,
    library_id: i64,
    publication_id: i64,
    work_id: i64,
) -> crate::Result<()> {
    sqlx::query!(
        "INSERT INTO publication_work (library_id, publication_id, work_id) VALUES (?, ?, ?)",
        library_id,
        publication_id,
        work_id,
    )
    .execute(conn)
    .await?;
    Ok(())
}

/// Fold `from` into `into` and delete `from`, on the caller's transaction.
///
/// `into` keeps every field it has; `from` fills only blanks, because the older record is the more
/// likely to be complete and correct. Credits, catalog numbers, and publication links move across
/// unless `into` already has them; links keep their rowid, so `into` takes `from`'s place in each
/// publication's contents.
pub(crate) async fn merge_works(
    conn: &mut SqliteConnection,
    library_id: i64,
    from: i64,
    into: i64,
) -> crate::Result<()> {
    let source = sqlx::query!(
        "SELECT \"key\", time_signature, instrumentation FROM work WHERE library_id = ? AND id = ?",
        library_id,
        from
    )
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query!(
        "UPDATE work SET \"key\" = COALESCE(\"key\", ?), time_signature = COALESCE(time_signature, ?),
             instrumentation = COALESCE(instrumentation, ?)
         WHERE library_id = ? AND id = ?",
        source.key,
        source.time_signature,
        source.instrumentation,
        library_id,
        into
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!(
        "INSERT OR IGNORE INTO work_contributor (library_id, work_id, person_id, role)
         SELECT library_id, ?, person_id, role FROM work_contributor WHERE work_id = ? ORDER BY id",
        into,
        from
    )
    .execute(&mut *conn)
    .await?;
    // The table is unique on text, but two spellings of one number are one number to the user
    let existing: Vec<CatalogNumber> = catalog_numbers_of(&mut *conn, into).await?;
    for number in catalog_numbers_of(&mut *conn, from).await? {
        if !existing.iter().any(|kept| kept.matches(&number)) {
            let value = number.as_str();
            sqlx::query!(
                "INSERT INTO work_catalog_number (work_id, value) VALUES (?, ?)",
                into,
                value
            )
            .execute(&mut *conn)
            .await?;
        }
    }
    sqlx::query!(
        "UPDATE publication_work SET work_id = ?1
         WHERE work_id = ?2
           AND publication_id NOT IN (SELECT publication_id FROM publication_work WHERE work_id = ?1)",
        into,
        from
    )
    .execute(&mut *conn)
    .await?;
    // Cascades the remaining links, credits, and numbers
    sqlx::query!(
        "DELETE FROM work WHERE library_id = ? AND id = ?",
        library_id,
        from
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

async fn catalog_numbers_of(
    conn: &mut SqliteConnection,
    work_id: i64,
) -> crate::Result<Vec<CatalogNumber>> {
    let values = sqlx::query_scalar!(
        "SELECT value FROM work_catalog_number WHERE work_id = ? ORDER BY id",
        work_id
    )
    .fetch_all(conn)
    .await?;
    Ok(values
        .iter()
        .map(|value| CatalogNumber::parse(value))
        .collect())
}

/// The oldest other work sharing a composer and a catalog number with this one
async fn find_duplicate(
    conn: &mut SqliteConnection,
    library_id: i64,
    work_id: i64,
) -> crate::Result<Option<i64>> {
    let own = catalog_numbers_of(&mut *conn, work_id).await?;
    if own.is_empty() {
        return Ok(None);
    }
    let candidates = sqlx::query!(
        "SELECT w.id, cn.value FROM work w
         JOIN work_catalog_number cn ON cn.work_id = w.id
         WHERE w.library_id = ?1 AND w.id != ?2
           AND w.id IN (SELECT work_id FROM work_contributor
                        WHERE role = 'composer'
                          AND person_id IN (SELECT person_id FROM work_contributor
                                            WHERE work_id = ?2 AND role = 'composer'))
         ORDER BY w.id",
        library_id,
        work_id
    )
    .fetch_all(conn)
    .await?;
    Ok(candidates
        .iter()
        .find(|row| {
            let number = CatalogNumber::parse(&row.value);
            own.iter().any(|mine| mine.matches(&number))
        })
        .map(|row| row.id))
}

/// Merge this work into a duplicate if it has one; returns the surviving id
pub(crate) async fn absorb_into_duplicate(
    conn: &mut SqliteConnection,
    library_id: i64,
    work_id: i64,
) -> crate::Result<i64> {
    match find_duplicate(&mut *conn, library_id, work_id).await? {
        Some(into) => {
            merge_works(conn, library_id, work_id, into).await?;
            Ok(into)
        }
        None => Ok(work_id),
    }
}

/// Rebuild a work's catalog numbers, so input order becomes stored order.
///
/// A number holds nothing beyond its text, so rebuilding outright loses nothing; creating and
/// updating share this.
pub(crate) async fn write_work_catalog_numbers(
    conn: &mut SqliteConnection,
    work_id: i64,
    numbers: &[CatalogNumber],
) -> crate::Result<()> {
    sqlx::query!("DELETE FROM work_catalog_number WHERE work_id = ?", work_id)
        .execute(&mut *conn)
        .await?;
    for number in numbers {
        let value = number.as_str();
        sqlx::query!(
            "INSERT INTO work_catalog_number (work_id, value) VALUES (?, ?)",
            work_id,
            value
        )
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
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
            catalog_numbers: vec!["Op. 27 No. 2".into(), "".into(), "op.27/2".into()],
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
                catalog_numbers: vec![
                    None,
                    Some(ValidationError::FillOrRemove),
                    Some(ValidationError::AlreadyListed),
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
            catalog_numbers: vec![" BWV 988 ".into()],
        };
        let parsed = raw.parse().unwrap();
        // A catalog number is kept as typed, apart from trimming
        assert_eq!(parsed.catalog_numbers[0].as_str(), "BWV 988");
        assert_eq!(
            parsed,
            WorkInput {
                id: Some(7),
                title: "Gnossienne No. 1".into(),
                // A field left blank is no value at all, not an empty one
                key: None,
                time_signature: Some("3/4".into()),
                instrumentation: Some("piano".into()),
                contributors: vec![contributor("Erik Satie", "composer")],
                catalog_numbers: vec![CatalogNumber::parse("BWV 988")],
            }
        );
    }
}
