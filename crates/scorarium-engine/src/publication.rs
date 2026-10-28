use std::fmt;
use std::num::ParseIntError;
use std::str::FromStr;

use isbn::{Isbn, Isbn13, IsbnError};
use sqlx::SqliteConnection;

use crate::audit::{Action, AuditEvent, AuditId, AuditSource, AuditSubject, Audited};
use crate::contributor::{Contributor, clean_role};
use crate::entity::{Entity, EntityKind, Timestamp};
use crate::holding::{Holding, HoldingId, HoldingKind, NewHolding};
use crate::library::LibraryId;
use crate::person::{self, Person, PersonId};
use crate::value::{Classification, Stars, Year, clean_text};
use crate::work::{self, Work, WorkId};
use crate::{Engine, Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PublicationId(i64);

impl PublicationId {
    pub(crate) fn new(id: i64) -> PublicationId {
        PublicationId(id)
    }

    pub fn get(self) -> i64 {
        self.0
    }
}

impl fmt::Display for PublicationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for PublicationId {
    type Err = ParseIntError;

    fn from_str(text: &str) -> std::result::Result<PublicationId, ParseIntError> {
        text.parse().map(PublicationId)
    }
}

/// The different inputs that can be used to import a draft publication
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportSeed {
    /// A user-entered title
    Title(String),
}

/// The fields of a publication
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    Title,
    Publisher,
    Year,
    Stars,
    Note,
    Classification,
    Tags,
    Identifiers,
    Links,
    Contributors,
    Contents,
    Holdings,
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Field::Title => "title",
            Field::Publisher => "publisher",
            Field::Year => "year",
            Field::Stars => "stars",
            Field::Note => "note",
            Field::Classification => "classification",
            Field::Tags => "tags",
            Field::Identifiers => "identifiers",
            Field::Links => "links",
            Field::Contributors => "contributors",
            Field::Contents => "contents",
            Field::Holdings => "holdings",
        })
    }
}

impl FromStr for Field {
    type Err = Error;

    fn from_str(text: &str) -> Result<Field> {
        Ok(match text {
            "title" => Field::Title,
            "publisher" => Field::Publisher,
            "year" => Field::Year,
            "stars" => Field::Stars,
            "note" => Field::Note,
            "classification" => Field::Classification,
            "tags" => Field::Tags,
            "identifiers" => Field::Identifiers,
            "links" => Field::Links,
            "contributors" => Field::Contributors,
            "contents" => Field::Contents,
            "holdings" => Field::Holdings,
            other => return Err(eyre::eyre!("unknown publication field {other:?}").into()),
        })
    }
}

#[derive(Clone, Debug)]
pub struct Publication {
    engine: Engine,
    id: PublicationId,
    library: LibraryId,
    draft: bool,
    created_at: Timestamp,
    last_modified_at: Timestamp,
    title: Option<String>,
    publisher: Option<String>,
    year: Option<Year>,
    stars: Option<Stars>,
    note: Option<String>,
    classification: Option<Classification>,
    contributors: Vec<Contributor>,
    holdings: Vec<Holding>,
}

/// A POD type that can be deserialized into by sqlx
struct PublicationRow {
    id: i64,
    library_id: i64,
    title: Option<String>,
    publisher: Option<String>,
    year: Option<u16>,
    stars: Option<u8>,
    note: Option<String>,
    kind: Option<String>,
    draft: bool,
    created_at: Timestamp,
    last_modified_at: Timestamp,
}

impl Publication {
    /// Only a draft publication is allowed to be missing a title
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn publisher(&self) -> Option<&str> {
        self.publisher.as_deref()
    }

    pub fn year(&self) -> Option<Year> {
        self.year
    }

    pub fn stars(&self) -> Option<Stars> {
        self.stars
    }

    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    pub fn classification(&self) -> Option<Classification> {
        self.classification
    }

    pub fn contributors(&self) -> &[Contributor] {
        &self.contributors
    }

    pub fn holdings(&self) -> &[Holding] {
        &self.holdings
    }

    async fn from_row(
        engine: &Engine,
        conn: &mut SqliteConnection,
        row: PublicationRow,
    ) -> Result<Publication> {
        let contributors = sqlx::query!(
            "SELECT publication_contributor.person_id, person.name, publication_contributor.role
             FROM publication_contributor JOIN person ON person.id = publication_contributor.person_id
             WHERE publication_contributor.publication_id = ?
             ORDER BY publication_contributor.id",
            row.id
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(|contributor| Contributor {
            person: PersonId::new(contributor.person_id),
            name: contributor.name,
            role: contributor.role,
        })
        .collect();
        let holdings = sqlx::query!(
            "SELECT id, kind, location, loaned_to FROM holding WHERE publication_id = ? ORDER BY id",
            row.id
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(|holding| {
            let kind = match holding.kind.as_str() {
                "physical" => HoldingKind::Physical {
                    location: holding.location,
                },
                other => return Err(eyre::eyre!("unsupported holding kind {other:?}").into()),
            };
            Ok(Holding {
                id: HoldingId::new(holding.id),
                kind,
                loaned_to: holding.loaned_to,
            })
        })
        .collect::<Result<_>>()?;
        Ok(Publication {
            engine: engine.clone(),
            id: PublicationId::new(row.id),
            library: LibraryId::new(row.library_id),
            draft: row.draft,
            created_at: row.created_at,
            last_modified_at: row.last_modified_at,
            title: row.title,
            publisher: row.publisher,
            year: row.year.map(Year::try_from).transpose()?,
            stars: row.stars.map(Stars::try_from).transpose()?,
            note: row.note,
            classification: row.kind.as_deref().map(str::parse).transpose()?,
            contributors,
            holdings,
        })
    }

    async fn fetch(
        engine: &Engine,
        conn: &mut SqliteConnection,
        id: PublicationId,
    ) -> Result<Option<Publication>> {
        let row = sqlx::query_as!(
            PublicationRow,
            r#"SELECT id, library_id, title, publisher, year AS "year: u16", stars AS "stars: u8",
                      note, kind, draft AS "draft: bool",
                      created_at AS "created_at: Timestamp",
                      last_modified_at AS "last_modified_at: Timestamp"
               FROM publication WHERE id = ?"#,
            id.get()
        )
        .fetch_optional(&mut *conn)
        .await?;
        match row {
            Some(row) => Ok(Some(Publication::from_row(engine, conn, row).await?)),
            None => Ok(None),
        }
    }

    async fn refresh(&mut self, conn: &mut SqliteConnection) -> Result<()> {
        *self = Publication::fetch(&self.engine, conn, self.id)
            .await?
            .ok_or(Error::NotFound)?;
        Ok(())
    }

    fn audit_subject(&self) -> AuditSubject {
        AuditSubject::Entity {
            kind: EntityKind::Publication,
            id: self.id.get(),
            library: self.library,
            label: self.title.clone().unwrap_or_default(),
        }
    }

    async fn commit_update(&self, tx: Audited<'_>, field: Field) -> Result<()> {
        tx.record(
            AuditSource::User,
            Action::Updated,
            Some(&self.audit_subject()),
            Some(&field.to_string()),
        )
        .await?
        .commit()
        .await
    }

    /// Retitle the publication
    ///
    /// Returns [Error::NotFound] if the publication has been deleted.
    pub async fn set_title(&mut self, title: impl AsRef<str>) -> Result<()> {
        let title = clean_title(title.as_ref())?;
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        if self.title.as_deref() == Some(title) {
            return Ok(());
        }
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE publication SET title = ?, last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            title,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        self.commit_update(tx, Field::Title).await?;
        self.title = Some(title.to_string());
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Set or clear the publisher, treating a blank name as none
    ///
    /// Returns [Error::NotFound] if the publication has been deleted.
    pub async fn set_publisher(&mut self, publisher: Option<&str>) -> Result<()> {
        let publisher = clean_text(publisher);
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        if self.publisher.as_deref() == publisher {
            return Ok(());
        }
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE publication SET publisher = ?, last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            publisher,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        self.commit_update(tx, Field::Publisher).await?;
        self.publisher = publisher.map(str::to_string);
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Set or clear the year of publication
    ///
    /// Returns [Error::NotFound] if the publication has been deleted.
    pub async fn set_year(&mut self, year: Option<Year>) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        if self.year == year {
            return Ok(());
        }
        let value = year.map(Year::get);
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE publication SET year = ?, last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            value,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        self.commit_update(tx, Field::Year).await?;
        self.year = year;
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Set or clear the rating
    ///
    /// Returns [Error::NotFound] if the publication has been deleted.
    pub async fn set_stars(&mut self, stars: Option<Stars>) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        if self.stars == stars {
            return Ok(());
        }
        let value = stars.map(Stars::get);
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE publication SET stars = ?, last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            value,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        self.commit_update(tx, Field::Stars).await?;
        self.stars = stars;
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Set or clear the note, treating a blank note as none
    ///
    /// Returns [Error::NotFound] if the publication has been deleted.
    pub async fn set_note(&mut self, note: Option<&str>) -> Result<()> {
        let note = clean_text(note);
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        if self.note.as_deref() == note {
            return Ok(());
        }
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE publication SET note = ?, last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            note,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        self.commit_update(tx, Field::Note).await?;
        self.note = note.map(str::to_string);
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Classify the publication as music or prose
    ///
    /// Returns [Error::NotFound] if the publication has been deleted.
    pub async fn set_classification(&mut self, classification: Classification) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        if self.classification == Some(classification) {
            return Ok(());
        }
        let kind = classification.to_string();
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE publication SET kind = ?, last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            kind,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        let works = work::inherit_classification(&mut tx, self.id, classification).await?;
        let mut tx = tx
            .record(
                AuditSource::User,
                Action::Updated,
                Some(&self.audit_subject()),
                Some(&Field::Classification.to_string()),
            )
            .await?;
        let field = work::Field::Classification.to_string();
        for subject in works {
            tx = tx
                .record(
                    AuditSource::User,
                    Action::Updated,
                    Some(&subject),
                    Some(&field),
                )
                .await?;
        }
        tx.commit().await?;
        self.classification = Some(classification);
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Create a work in the publication with the owning publication's draft state and classification
    ///
    /// Returns [Error::NotFound] if the publication has been deleted.
    pub async fn create_work(&mut self, title: impl AsRef<str>) -> Result<Work> {
        let title = work::clean_title(title.as_ref())?;
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        let work = work::insert(
            &self.engine,
            &mut tx,
            self.library,
            title,
            self.draft,
            self.classification,
        )
        .await?;
        sqlx::query!(
            "INSERT INTO publication_work (library_id, publication_id, work_id) VALUES (?, ?, ?)",
            self.library.get(),
            self.id.get(),
            work.id().get()
        )
        .execute(&mut *tx)
        .await?;
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE publication SET last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        tx.record(
            AuditSource::User,
            Action::Created,
            Some(&work.audit_subject()),
            None,
        )
        .await?
        .record(
            AuditSource::User,
            Action::Updated,
            Some(&self.audit_subject()),
            Some(&Field::Contents.to_string()),
        )
        .await?
        .commit()
        .await?;
        self.last_modified_at = last_modified_at;
        Ok(work)
    }

    /// List every work in the publication
    pub async fn works(&self) -> Result<Vec<Work>> {
        work::in_publication(&self.engine, self.id).await
    }

    /// Create a person with the owning publication's draft state
    ///
    /// Returns [Error::NotFound] if the publication has been deleted.
    pub async fn create_contributor(
        &mut self,
        name: impl AsRef<str>,
        role: impl AsRef<str>,
    ) -> Result<Person> {
        let name = person::clean_name(name.as_ref())?;
        let role = clean_role(role.as_ref())?;
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        let person = person::insert(&self.engine, &mut tx, self.library, name, self.draft).await?;
        sqlx::query!(
            "INSERT INTO publication_contributor (library_id, publication_id, person_id, role)
             VALUES (?, ?, ?, ?)",
            self.library.get(),
            self.id.get(),
            person.id().get(),
            role
        )
        .execute(&mut *tx)
        .await?;
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE publication SET last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        tx.record(
            AuditSource::User,
            Action::Created,
            Some(&person.audit_subject()),
            None,
        )
        .await?
        .record(
            AuditSource::User,
            Action::Updated,
            Some(&self.audit_subject()),
            Some(&Field::Contributors.to_string()),
        )
        .await?
        .commit()
        .await?;
        self.contributors.push(Contributor {
            person: person.id(),
            name: person.name().to_string(),
            role: role.to_string(),
        });
        self.last_modified_at = last_modified_at;
        Ok(person)
    }
}

impl Entity for Publication {
    type Id = PublicationId;
    type Field = Field;

    async fn load(engine: &Engine, id: PublicationId) -> Result<Option<Publication>> {
        let mut conn = engine.begin_read().await?;
        Publication::fetch(engine, &mut conn, id).await
    }

    async fn reload(&mut self) -> Result<()> {
        let mut conn = self.engine.begin_read().await?;
        self.refresh(&mut conn).await
    }

    fn id(&self) -> PublicationId {
        self.id
    }

    fn library(&self) -> LibraryId {
        self.library
    }

    fn is_draft(&self) -> bool {
        self.draft
    }

    fn created_at(&self) -> Timestamp {
        self.created_at
    }

    fn last_modified_at(&self) -> Timestamp {
        self.last_modified_at
    }

    async fn audit_log(&self, before: Option<AuditId>, limit: usize) -> Result<Vec<AuditEvent>> {
        self.engine
            .entity_audit_log(
                EntityKind::Publication,
                self.id.get(),
                self.library,
                before,
                limit,
            )
            .await
    }

    async fn commit(&mut self) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        if !self.draft {
            return Ok(());
        }
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE publication SET draft = 0, last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        tx.record(
            AuditSource::User,
            Action::Committed,
            Some(&self.audit_subject()),
            None,
        )
        .await?
        .commit()
        .await?;
        self.draft = false;
        self.last_modified_at = last_modified_at;
        Ok(())
    }
}

/// Create a draft publication in `library` from `seed` with its first holding
pub(crate) async fn create(
    engine: &Engine,
    library: LibraryId,
    seed: ImportSeed,
    holding: NewHolding<'_>,
) -> Result<Publication> {
    let ImportSeed::Title(title) = seed;
    let title = clean_title(&title)?;
    let NewHolding::Physical { location } = holding;
    let location = clean_text(location);
    let mut tx = engine.begin_write().await?;
    let library = library.get();
    let id = sqlx::query_scalar!(
        "INSERT INTO publication (library_id, title, draft) VALUES (?, ?, 1) RETURNING id AS \"id!\"",
        library,
        title
    )
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query!(
        "INSERT INTO holding (publication_id, kind, location) VALUES (?, 'physical', ?)",
        id,
        location
    )
    .execute(&mut *tx)
    .await?;
    let publication = Publication::fetch(engine, &mut tx, PublicationId::new(id))
        .await?
        .ok_or(Error::NotFound)?;
    tx.record(
        AuditSource::User,
        Action::Created,
        Some(&publication.audit_subject()),
        None,
    )
    .await?
    .commit()
    .await?;
    Ok(publication)
}

/// List every publication in `library`, drafts included, oldest first
pub(crate) async fn in_library(engine: &Engine, library: LibraryId) -> Result<Vec<Publication>> {
    let mut conn = engine.begin_read().await?;
    let library = library.get();
    let rows = sqlx::query_as!(
        PublicationRow,
        r#"SELECT id, library_id, title, publisher, year AS "year: u16", stars AS "stars: u8",
                  note, kind, draft AS "draft: bool",
                  created_at AS "created_at: Timestamp",
                  last_modified_at AS "last_modified_at: Timestamp"
           FROM publication WHERE library_id = ? ORDER BY id"#,
        library
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut publications = Vec::with_capacity(rows.len());
    for row in rows {
        publications.push(Publication::from_row(engine, &mut conn, row).await?);
    }
    Ok(publications)
}

/// List every publication that contains `work`
pub(crate) async fn containing(engine: &Engine, work: WorkId) -> Result<Vec<Publication>> {
    let mut conn = engine.begin_read().await?;
    let work = work.get();
    let rows = sqlx::query_as!(
        PublicationRow,
        r#"SELECT publication.id, publication.library_id, publication.title, publication.publisher,
                  publication.year AS "year: u16", publication.stars AS "stars: u8",
                  publication.note, publication.kind, publication.draft AS "draft: bool",
                  publication.created_at AS "created_at: Timestamp",
                  publication.last_modified_at AS "last_modified_at: Timestamp"
           FROM publication JOIN publication_work ON publication_work.publication_id = publication.id
           WHERE publication_work.work_id = ?
           ORDER BY publication.id"#,
        work
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut publications = Vec::with_capacity(rows.len());
    for row in rows {
        publications.push(Publication::from_row(engine, &mut conn, row).await?);
    }
    Ok(publications)
}

/// List every publication that names `person` as a contributor
pub(crate) async fn with_contributor(
    engine: &Engine,
    person: PersonId,
) -> Result<Vec<Publication>> {
    let mut conn = engine.begin_read().await?;
    let person = person.get();
    // A person can contribute in several roles, so a join would repeat the publication
    let rows = sqlx::query_as!(
        PublicationRow,
        r#"SELECT id, library_id, title, publisher, year AS "year: u16", stars AS "stars: u8",
                  note, kind, draft AS "draft: bool",
                  created_at AS "created_at: Timestamp",
                  last_modified_at AS "last_modified_at: Timestamp"
           FROM publication
           WHERE id IN (SELECT publication_id FROM publication_contributor WHERE person_id = ?)
           ORDER BY id"#,
        person
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut publications = Vec::with_capacity(rows.len());
    for row in rows {
        publications.push(Publication::from_row(engine, &mut conn, row).await?);
    }
    Ok(publications)
}

fn clean_title(title: &str) -> Result<&str> {
    let title = title.trim();
    if title.is_empty() {
        return Err(Error::InvalidValue(
            "A publication title cannot be blank.".into(),
        ));
    }
    Ok(title)
}

/// The kinds of identifier printed on a publication
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IdentifierKind {
    Isbn,
    /// Like an ISBN, but for music
    Ismn,
    /// A generic publisher number
    ///
    /// These are typically printed on the front cover, if present.
    PublisherNumber,
    /// A more specific plate number to identify a particular engraving
    ///
    /// These are typically printed at the bottom of the page, if present.
    PlateNumber,
}

impl fmt::Display for IdentifierKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            IdentifierKind::Isbn => "isbn",
            IdentifierKind::Ismn => "ismn",
            IdentifierKind::PublisherNumber => "publisher_number",
            IdentifierKind::PlateNumber => "plate_number",
        };
        f.write_str(name)
    }
}

impl FromStr for IdentifierKind {
    type Err = Error;

    fn from_str(input: &str) -> Result<IdentifierKind> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("isbn") {
            Ok(IdentifierKind::Isbn)
        } else if input.eq_ignore_ascii_case("ismn") {
            Ok(IdentifierKind::Ismn)
        } else if input.eq_ignore_ascii_case("publisher_number") {
            Ok(IdentifierKind::PublisherNumber)
        } else if input.eq_ignore_ascii_case("plate_number") {
            Ok(IdentifierKind::PlateNumber)
        } else {
            Err(Error::InvalidValue(format!(
                "'{input}' must be one of: isbn, ismn, publisher_number, or plate_number."
            )))
        }
    }
}

/// An identifier printed on a publication, parsed into its canonical form
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Identifier {
    kind: IdentifierKind,
    text: String,
}

impl Identifier {
    /// Validate an identifier as typed or printed and put it in canonical form.
    ///
    /// ISBNs become hyphenated ISBN-13 (ISBN-10 is converted), ISMNs the hyphenated `979-0` form,
    /// and publisher and plate numbers are trimmed and uppercased.
    pub fn new(kind: IdentifierKind, text: &str) -> Result<Identifier> {
        let text = match kind {
            IdentifierKind::Isbn => normalize_isbn(strip_label(text, "ISBN"))?,
            IdentifierKind::Ismn => normalize_ismn(strip_label(text, "ISMN"))?,
            IdentifierKind::PublisherNumber | IdentifierKind::PlateNumber => {
                let text = text.trim();
                if text.is_empty() {
                    let message = match kind {
                        IdentifierKind::PublisherNumber => "A publisher number cannot be empty.",
                        _ => "A plate number cannot be empty.",
                    };
                    return Err(Error::InvalidValue(message.to_string()));
                }
                text.to_uppercase()
            }
        };
        Ok(Identifier { kind, text })
    }

    pub fn kind(&self) -> IdentifierKind {
        self.kind
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }
}

impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// Whether the text looks like it could be an ISBN or ISMN while it's being typed
pub fn looks_like_identifier(query: &str) -> bool {
    let query = query.trim().as_bytes();
    if query.get(..4).is_some_and(|head| {
        head.eq_ignore_ascii_case(b"isbn") || head.eq_ignore_ascii_case(b"ismn")
    }) {
        return true;
    }
    let compact: Vec<u8> = query
        .iter()
        .copied()
        .filter(|byte| !matches!(byte, b'-' | b' '))
        .collect();
    // The pre-2008 ISMN form starts with M, and an ISBN-10 check digit can be X
    let digits = compact
        .strip_prefix(b"M")
        .or_else(|| compact.strip_prefix(b"m"))
        .unwrap_or(&compact);
    let digits = digits
        .strip_suffix(b"X")
        .or_else(|| digits.strip_suffix(b"x"))
        .unwrap_or(digits);
    !digits.is_empty() && digits.iter().all(u8::is_ascii_digit)
}

/// Strip off the label prefix an ISBN or ISMN number is often printed with
///
/// "ISBN 978-...", "ISBN-13: 978-...", "ISBN 10: 0-...", and "ISMN M-..."
///
/// This allows users to enter the identifiers as they appear printed on a publication, even though
/// the prefix isn't actually necessary.
fn strip_label<'a>(value: &'a str, label: &str) -> &'a str {
    let value = value.trim();
    let Some(rest) = value
        .get(..label.len())
        .filter(|head| head.eq_ignore_ascii_case(label))
        .map(|_| &value[label.len()..])
    else {
        return value;
    };
    // Treat "ISBN-13", "ISBN 13", and "ISBN13" all the same
    let rest = rest.trim_start_matches(|c: char| c == '-' || c.is_whitespace());
    let rest = match rest.strip_prefix("13").or_else(|| rest.strip_prefix("10")) {
        Some(after) if !after.starts_with(|c: char| c.is_ascii_digit()) => after,
        _ => rest,
    };
    rest.trim_start_matches(|c: char| c == ':' || c.is_whitespace())
}

fn normalize_isbn(text: &str) -> Result<String> {
    let invalid = || Error::InvalidValue(format!("'{text}' is not a valid ISBN."));
    let isbn13 = match text.parse::<Isbn>() {
        Ok(Isbn::_10(isbn10)) => Isbn13::from(isbn10),
        Ok(Isbn::_13(isbn13)) => isbn13,
        Err(_) => return Err(invalid()),
    };
    match isbn13.hyphenate() {
        Ok(hyphenated) => Ok(hyphenated.to_string()),
        Err(IsbnError::UndefinedRange | IsbnError::InvalidGroup) => {
            // There's two yucky choices for parsing the ISBN registration group (because it's
            // constantly changing, which is miserable):
            //
            // 1. Download the ISBN tables from the international authority (and cache it?)
            // 2. Build the ISBN tables into scorarium at build time
            //
            // I picked the second option because it's simple, but it does mean that ISBNs assigned
            // after the scorarium binary is built (and Cargo.lock is updated) have the potential to
            // be unrecognized. Big yikes.
            //
            // I'm betting I don't hit that case because:
            //
            // 1. This is a personal project
            // 2. My personal library doesn't gain many brand new books, and essentially zero new
            //    music books
            // 3. I don't want to have to download the tables
            //
            // If future me runs into this issue, it should "just work" to update the 'isbn' crate
            // to its latest version and release a new scorarium build.
            Err(Error::InvalidValue(format!(
                "The registration group of '{text}' is not in scorarium's ISBN range table, which may be out of date."
            )))
        }
        Err(_) => Err(invalid()),
    }
}

fn normalize_ismn(text: &str) -> Result<String> {
    let invalid = || Error::InvalidValue(format!("'{text}' is not a valid ISMN."));
    let compact: String = text
        .chars()
        .filter(|c| !matches!(c, '-' | ' '))
        .collect::<String>()
        .to_ascii_uppercase();
    // The pre-2008 ten-character form spells the 979-0 prefix as a letter M
    let digits = match compact.strip_prefix('M') {
        Some(rest) => format!("9790{rest}"),
        None => compact,
    };
    if digits.len() != 13
        || !digits.starts_with("9790")
        || !digits.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(invalid());
    }
    let checksum: u32 = digits
        .bytes()
        .enumerate()
        .map(|(i, b)| u32::from(b - b'0') * if i % 2 == 0 { 1 } else { 3 })
        .sum();
    if !checksum.is_multiple_of(10) {
        return Err(invalid());
    }
    let body = &digits[4..12];
    let registrant_len = match body.as_bytes()[0] {
        b'0' => 3,
        b'1'..=b'3' => 4,
        b'4'..=b'6' => 5,
        b'7' | b'8' => 6,
        _ => 7,
    };
    let (registrant, item) = body.split_at(registrant_len);
    let check = &digits[12..];
    Ok(format!("979-0-{registrant}-{item}-{check}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EngineOptions;

    #[tokio::test]
    async fn publication_edits_audit_once_per_change() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), EngineOptions::default())
            .await
            .unwrap();
        let library = engine.create_library("Books", false).await.unwrap();
        let mut publication = library
            .create_publication(
                ImportSeed::Title("Pro Git".into()),
                NewHolding::Physical {
                    location: Some("Desk"),
                },
            )
            .await
            .unwrap();
        publication.set_publisher(Some("Apress")).await.unwrap();
        publication.set_publisher(Some("Apress")).await.unwrap();
        publication
            .set_stars(Some(Stars::try_from(3).unwrap()))
            .await
            .unwrap();
        publication.commit().await.unwrap();

        let subject = Some(AuditSubject::Entity {
            kind: EntityKind::Publication,
            id: publication.id().get(),
            library: library.id(),
            label: "Pro Git".into(),
        });
        let events: Vec<_> = publication
            .audit_log(None, 10)
            .await
            .unwrap()
            .into_iter()
            .map(|e| (e.action, e.subject, e.field))
            .collect();
        assert_eq!(
            events,
            [
                (Action::Committed, subject.clone(), None),
                (Action::Updated, subject.clone(), Some("stars".into())),
                (Action::Updated, subject.clone(), Some("publisher".into())),
                (Action::Created, subject, None),
            ]
        );

        let loaded = Publication::load(&engine, publication.id())
            .await
            .unwrap()
            .unwrap();
        let scalars = |p: &Publication| {
            (
                p.title().map(str::to_string),
                p.publisher().map(str::to_string),
                p.year(),
                p.stars(),
                p.note().map(str::to_string),
                p.classification(),
            )
        };
        assert_eq!(scalars(&loaded), scalars(&publication));
        assert_eq!(
            scalars(&loaded),
            (
                Some("Pro Git".into()),
                Some("Apress".into()),
                None,
                Some(Stars::try_from(3).unwrap()),
                None,
                None,
            )
        );
        assert!(!loaded.is_draft());
        let holdings: Vec<_> = loaded.holdings().iter().map(|h| &h.kind).collect();
        assert_eq!(
            holdings,
            [&HoldingKind::Physical {
                location: Some("Desk".into())
            }]
        );
    }

    fn n(kind: IdentifierKind, value: &str) -> Result<String> {
        Identifier::new(kind, value).map(|id| id.as_str().to_string())
    }

    #[test]
    fn isbn() {
        assert_eq!(
            n(IdentifierKind::Isbn, "0-486-23134-8").unwrap(),
            "978-0-486-23134-1"
        );
        for input in [
            "978-1-4950-0871-9",
            "978 1 4950 0871 9",
            "9781495008719",
            "ISBN 978-1-4950-0871-9",
            "ISBN: 978-1-4950-0871-9",
            "ISBN\t978-1-4950-0871-9",
            "isbn-13: 9781495008719",
            "ISBN 13: 9781495008719",
            "ISBN13 9781495008719",
        ] {
            assert_eq!(n(IdentifierKind::Isbn, input).unwrap(), "978-1-4950-0871-9");
        }
        for input in ["ISBN-10 0-7935-7224-X", "ISBN 10: 0-7935-7224-X"] {
            assert_eq!(n(IdentifierKind::Isbn, input).unwrap(), "978-0-7935-7224-3");
        }
        // The leading 10 is part of the number, not the form's name
        assert_eq!(
            n(IdentifierKind::Isbn, "ISBN 108456789X").unwrap(),
            "978-1-0845-6789-4"
        );
        assert!(matches!(
            n(IdentifierKind::Isbn, "978-1-4950-0871-0"),
            Err(Error::InvalidValue(_))
        ));
        // Checksum-valid, but 978-67 is not an assigned registration group
        assert!(matches!(
            n(IdentifierKind::Isbn, "9786700000007"),
            Err(Error::InvalidValue(_))
        ));
    }

    #[test]
    fn ismn() {
        for input in [
            "M-060-08002-9",
            "ISMN M-060-08002-9",
            "ISMN: 979-0-060-08002-9",
            "ISMN-13 979-0-060-08002-9",
            "979-0-060-08002-9",
        ] {
            assert_eq!(n(IdentifierKind::Ismn, input).unwrap(), "979-0-060-08002-9");
        }
        assert_eq!(
            n(IdentifierKind::Ismn, "9790299102349").unwrap(),
            "979-0-2991-0234-9"
        );
        assert!(matches!(
            n(IdentifierKind::Ismn, "979-0-060-08002-0"),
            Err(Error::InvalidValue(_))
        ));
    }

    #[test]
    fn free_text_numbers() {
        let plate = Identifier::new(IdentifierKind::PlateNumber, " ut 50061 ").unwrap();
        assert_eq!(plate.as_str(), "UT 50061");
        assert_eq!(plate.to_string(), "UT 50061");
        assert!(matches!(
            n(IdentifierKind::PublisherNumber, "   "),
            Err(Error::InvalidValue(_))
        ));
    }

    #[test]
    fn identifier_fragments_look_like_identifiers_and_titles_do_not() {
        for input in [
            "ISBN 978",
            "ismn",
            "978",
            "978-0-48",
            "978 0 486 23134 1",
            "M-060-080",
            "0-7935-7224-X",
            // Complete forms the Identifier tests accept
            "ISBN 13: 9781495008719",
            "isbn-13: 9781495008719",
            "ISBN 10: 0-7935-7224-X",
            "9781495008719",
            "M-060-08002-9",
            "979-0-060-08002-9",
        ] {
            assert!(looks_like_identifier(input), "{input:?}");
        }
        for input in [
            "",
            "Three Gymnopedies",
            "Op. 9 No. 2",
            "Symphony 40",
            "1984 Orwell",
            "The ISBN Handbook",
        ] {
            assert!(!looks_like_identifier(input), "{input:?}");
        }
    }
}
