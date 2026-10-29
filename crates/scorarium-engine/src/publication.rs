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
use crate::link::{ExternalRecord, HttpUrl, Link, LinkId};
use crate::person::{self, Person, PersonId};
use crate::suggest::{Candidate, CandidateScope};
use crate::value::{Classification, Stars, Tag, Year, clean_text};
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
    /// An identifier printed on the publication
    Identifier(Identifier),
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
    tags: Vec<Tag>,
    identifiers: Vec<Identifier>,
    links: Vec<Link>,
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

    /// List the publication's tags alphabetically
    pub fn tags(&self) -> &[Tag] {
        &self.tags
    }

    /// List the publication's identifiers by kind, then by value
    pub fn identifiers(&self) -> &[Identifier] {
        &self.identifiers
    }

    /// List the publication's links in the order they were added
    pub fn links(&self) -> &[Link] {
        &self.links
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
        let tags = sqlx::query_scalar!(
            "SELECT tag FROM tag WHERE publication_id = ? ORDER BY tag",
            row.id
        )
        .fetch_all(&mut *conn)
        .await?
        .iter()
        .map(|tag| tag.parse())
        .collect::<Result<_>>()?;
        let mut identifiers = sqlx::query!(
            "SELECT kind, value FROM publication_identifier WHERE publication_id = ?",
            row.id
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(|identifier| Identifier::new(identifier.kind.parse()?, &identifier.value))
        .collect::<Result<Vec<_>>>()?;
        identifiers.sort();
        let links = sqlx::query!(
            "SELECT id, url, kind, external_id FROM publication_link
             WHERE publication_id = ? ORDER BY id",
            row.id
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(|link| Link::new(link.id, &link.url, &link.kind, link.external_id))
        .collect::<Result<_>>()?;
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
            tags,
            identifiers,
            links,
            contributors,
            holdings,
        })
    }

    pub(crate) async fn fetch(
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

    /// Name the publication by its title, or by its first identifier when it has no title
    fn label(&self) -> String {
        self.title
            .clone()
            .or_else(|| self.identifiers.first().map(Identifier::to_string))
            .unwrap_or_default()
    }

    fn audit_subject(&self) -> AuditSubject {
        AuditSubject::Entity {
            kind: EntityKind::Publication,
            id: self.id.get(),
            library: self.library,
            label: self.label(),
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

    /// Tag the publication
    ///
    /// Returns [Error::NotFound] if the publication has been deleted.
    pub async fn add_tag(&mut self, tag: Tag) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        let Err(position) = self.tags.binary_search(&tag) else {
            return Ok(());
        };
        sqlx::query!(
            "INSERT INTO tag (library_id, publication_id, tag) VALUES (?, ?, ?)",
            self.library.get(),
            self.id.get(),
            tag.as_str()
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
        self.commit_update(tx, Field::Tags).await?;
        self.tags.insert(position, tag);
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Remove a tag from the publication
    ///
    /// Returns [Error::NotFound] if the publication has been deleted.
    pub async fn remove_tag(&mut self, tag: &Tag) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        let Ok(position) = self.tags.binary_search(tag) else {
            return Ok(());
        };
        sqlx::query!(
            "DELETE FROM tag WHERE publication_id = ? AND tag = ?",
            self.id.get(),
            tag.as_str()
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
        self.commit_update(tx, Field::Tags).await?;
        self.tags.remove(position);
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Add an identifier to the publication
    ///
    /// Returns [Error::NotFound] if the publication has been deleted.
    pub async fn add_identifier(&mut self, identifier: Identifier) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        let Err(position) = self.identifiers.binary_search(&identifier) else {
            return Ok(());
        };
        let kind = identifier.kind().to_string();
        sqlx::query!(
            "INSERT INTO publication_identifier (publication_id, kind, value) VALUES (?, ?, ?)",
            self.id.get(),
            kind,
            identifier.as_str()
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
        self.commit_update(tx, Field::Identifiers).await?;
        self.identifiers.insert(position, identifier);
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Remove an identifier from the publication
    ///
    /// Returns [Error::NotFound] if the publication has been deleted.
    pub async fn remove_identifier(&mut self, identifier: &Identifier) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        let Ok(position) = self.identifiers.binary_search(identifier) else {
            return Ok(());
        };
        let kind = identifier.kind().to_string();
        sqlx::query!(
            "DELETE FROM publication_identifier WHERE publication_id = ? AND kind = ? AND value = ?",
            self.id.get(),
            kind,
            identifier.as_str()
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
        self.commit_update(tx, Field::Identifiers).await?;
        self.identifiers.remove(position);
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Link the publication to `url`
    ///
    /// Returns [Error::NotFound] if the publication has been deleted.
    pub async fn add_link(&mut self, url: HttpUrl) -> Result<()> {
        let record = ExternalRecord::recognize(EntityKind::Publication, &url);
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        if self
            .links
            .iter()
            .any(|link| link.url == url || (link.record.is_some() && link.record == record))
        {
            return Ok(());
        }
        let kind = record
            .as_ref()
            .map_or_else(|| "generic".to_string(), |record| record.kind().to_string());
        let external_id = record.as_ref().map(ExternalRecord::id);
        let id = sqlx::query_scalar!(
            r#"INSERT INTO publication_link (publication_id, url, kind, external_id)
               VALUES (?, ?, ?, ?) RETURNING id AS "id!""#,
            self.id.get(),
            url.as_url().as_str(),
            kind,
            external_id
        )
        .fetch_one(&mut *tx)
        .await?;
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE publication SET last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        self.commit_update(tx, Field::Links).await?;
        self.links.push(Link {
            id: LinkId::new(id),
            url,
            record,
        });
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Remove a link from the publication
    ///
    /// Returns [Error::NotFound] if the publication has been deleted.
    pub async fn remove_link(&mut self, link: LinkId) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        let removed = sqlx::query!(
            "DELETE FROM publication_link WHERE id = ? AND publication_id = ?",
            link.get(),
            self.id.get()
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if removed == 0 {
            return Ok(());
        }
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE publication SET last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        self.commit_update(tx, Field::Links).await?;
        self.links.retain(|existing| existing.id != link);
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Add a holding for another copy the library owns, treating a blank location as none
    ///
    /// Returns [Error::NotFound] if the publication has been deleted.
    pub async fn add_holding(&mut self, holding: NewHolding<'_>) -> Result<HoldingId> {
        let NewHolding::Physical { location } = holding;
        let location = clean_text(location);
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        let id = sqlx::query_scalar!(
            r#"INSERT INTO holding (publication_id, kind, location) VALUES (?, 'physical', ?)
               RETURNING id AS "id!""#,
            self.id.get(),
            location
        )
        .fetch_one(&mut *tx)
        .await?;
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE publication SET last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        self.commit_update(tx, Field::Holdings).await?;
        let id = HoldingId::new(id);
        self.holdings.push(Holding {
            id,
            kind: HoldingKind::Physical {
                location: location.map(str::to_string),
            },
            loaned_to: None,
        });
        self.last_modified_at = last_modified_at;
        Ok(id)
    }

    /// Remove a copy of the publication
    ///
    /// Returns [Error::InvalidValue] if it is the publication's only holding, and
    /// [Error::NotFound] if the publication has been deleted.
    pub async fn remove_holding(&mut self, holding: HoldingId) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        let Some(position) = self.holdings.iter().position(|h| h.id == holding) else {
            return Ok(());
        };
        if self.holdings.len() == 1 {
            return Err(Error::InvalidValue(
                "A publication needs at least one holding.".into(),
            ));
        }
        sqlx::query!("DELETE FROM holding WHERE id = ?", holding.get())
            .execute(&mut *tx)
            .await?;
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE publication SET last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        self.commit_update(tx, Field::Holdings).await?;
        self.holdings.remove(position);
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Lend a copy of the publication to `loaned_to`, or mark it returned with none
    ///
    /// Treat a blank name as none, and do nothing if the loan is unchanged.
    ///
    /// Returns [Error::NotFound] if the publication has been deleted or has no such holding.
    pub async fn set_holding_loaned_to(
        &mut self,
        holding: HoldingId,
        loaned_to: Option<&str>,
    ) -> Result<()> {
        let loaned_to = clean_text(loaned_to);
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        let Some(position) = self.holdings.iter().position(|h| h.id == holding) else {
            return Err(Error::NotFound);
        };
        if self.holdings[position].loaned_to.as_deref() == loaned_to {
            return Ok(());
        }
        sqlx::query!(
            "UPDATE holding SET loaned_to = ? WHERE id = ?",
            loaned_to,
            holding.get()
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
        self.commit_update(tx, Field::Holdings).await?;
        self.holdings[position].loaned_to = loaned_to.map(str::to_string);
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

    /// Add a work from the publication's library to the publication
    ///
    /// Commit the work and its contributors if the publication is committed, and give an
    /// unclassified work the publication's classification.
    ///
    /// Returns [Error::NotFound] if the publication has been deleted or its library has no such
    /// work.
    pub async fn add_work(&mut self, work: WorkId) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        if !work::exists(&mut tx, self.library, work).await? {
            return Err(Error::NotFound);
        }
        let inserted = sqlx::query!(
            "INSERT INTO publication_work (library_id, publication_id, work_id) VALUES (?, ?, ?)
             ON CONFLICT (publication_id, work_id) DO NOTHING",
            self.library.get(),
            self.id.get(),
            work.get()
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if inserted == 0 {
            return Ok(());
        }
        let mut committed = Vec::new();
        if !self.draft {
            committed.extend(work::commit_works_of(&mut tx, self.id).await?);
            committed.extend(person::commit_contributors_of_work(&mut tx, work).await?);
        }
        let classified = match self.classification {
            Some(classification) => {
                work::inherit_classification(&mut tx, self.id, classification).await?
            }
            None => Vec::new(),
        };
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE publication SET last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        let mut tx = tx
            .record(
                AuditSource::User,
                Action::Updated,
                Some(&self.audit_subject()),
                Some(&Field::Contents.to_string()),
            )
            .await?;
        for subject in committed {
            tx = tx
                .record(AuditSource::User, Action::Committed, Some(&subject), None)
                .await?;
        }
        let field = work::Field::Classification.to_string();
        for subject in classified {
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
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Remove a work from the publication along with any orphaned works and people
    ///
    /// Do nothing if the publication does not contain the work.
    ///
    /// Returns [Error::NotFound] if the publication has been deleted.
    pub async fn remove_work(&mut self, work: WorkId) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        let removed = sqlx::query!(
            "DELETE FROM publication_work WHERE publication_id = ? AND work_id = ?",
            self.id.get(),
            work.get()
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if removed == 0 {
            return Ok(());
        }
        let works = work::collect_orphan_works(&mut tx, self.library).await?;
        let people = person::collect_orphan_people(&mut tx, self.library).await?;
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE publication SET last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        let mut tx = tx
            .record(
                AuditSource::User,
                Action::Updated,
                Some(&self.audit_subject()),
                Some(&Field::Contents.to_string()),
            )
            .await?;
        for subject in works.iter().chain(&people) {
            tx = tx
                .record(
                    AuditSource::OrphanCleanup,
                    Action::Deleted,
                    Some(subject),
                    None,
                )
                .await?;
        }
        tx.commit().await?;
        self.last_modified_at = last_modified_at;
        Ok(())
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

    /// Name a person from the publication's library as a contributor in `role`
    ///
    /// Commit the person if it is a draft and the publication is committed.
    ///
    /// Returns [Error::NotFound] if the publication has been deleted or its library has no such
    /// person.
    pub async fn add_contributor(&mut self, person: PersonId, role: impl AsRef<str>) -> Result<()> {
        let role = clean_role(role.as_ref())?;
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        let name = person::name(&mut tx, self.library, person)
            .await?
            .ok_or(Error::NotFound)?;
        let inserted = sqlx::query!(
            "INSERT INTO publication_contributor (library_id, publication_id, person_id, role)
             VALUES (?, ?, ?, ?)
             ON CONFLICT (publication_id, person_id, role) DO NOTHING",
            self.library.get(),
            self.id.get(),
            person.get(),
            role
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if inserted == 0 {
            return Ok(());
        }
        let committed = if self.draft {
            Vec::new()
        } else {
            person::commit_contributors_of_publication(&mut tx, self.id).await?
        };
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE publication SET last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        let mut tx = tx
            .record(
                AuditSource::User,
                Action::Updated,
                Some(&self.audit_subject()),
                Some(&Field::Contributors.to_string()),
            )
            .await?;
        for subject in committed {
            tx = tx
                .record(AuditSource::User, Action::Committed, Some(&subject), None)
                .await?;
        }
        tx.commit().await?;
        self.contributors.push(Contributor {
            person,
            name,
            role: role.to_string(),
        });
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Remove a contributor from the publication
    ///
    /// Returns [Error::NotFound] if the publication has been deleted.
    pub async fn remove_contributor(
        &mut self,
        person: PersonId,
        role: impl AsRef<str>,
    ) -> Result<()> {
        let role = role.as_ref().trim();
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        let removed = sqlx::query!(
            "DELETE FROM publication_contributor
             WHERE publication_id = ? AND person_id = ? AND role = ?",
            self.id.get(),
            person.get(),
            role
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if removed == 0 {
            return Ok(());
        }
        let people = person::collect_orphan_people(&mut tx, self.library).await?;
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE publication SET last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        let mut tx = tx
            .record(
                AuditSource::User,
                Action::Updated,
                Some(&self.audit_subject()),
                Some(&Field::Contributors.to_string()),
            )
            .await?;
        for subject in people {
            tx = tx
                .record(
                    AuditSource::OrphanCleanup,
                    Action::Deleted,
                    Some(&subject),
                    None,
                )
                .await?;
        }
        tx.commit().await?;
        self.contributors
            .retain(|contributor| !(contributor.person == person && contributor.role == role));
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Delete the publication along with any orphaned works and people
    ///
    /// Returns [Error::NotFound] if the publication has already been deleted.
    pub async fn delete(mut self) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        sqlx::query!("DELETE FROM publication WHERE id = ?", self.id.get())
            .execute(&mut *tx)
            .await?;
        let works = work::collect_orphan_works(&mut tx, self.library).await?;
        let people = person::collect_orphan_people(&mut tx, self.library).await?;
        let mut tx = tx
            .record(
                AuditSource::User,
                Action::Deleted,
                Some(&self.audit_subject()),
                None,
            )
            .await?;
        for subject in works.iter().chain(&people) {
            tx = tx
                .record(
                    AuditSource::OrphanCleanup,
                    Action::Deleted,
                    Some(subject),
                    None,
                )
                .await?;
        }
        tx.commit().await
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
        if self.title.is_none() {
            return Err(Error::InvalidValue(
                "A publication needs a title before it can be committed.".into(),
            ));
        }
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE publication SET draft = 0, last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        let mut subjects = work::commit_works_of(&mut tx, self.id).await?;
        let works = sqlx::query_scalar!(
            "SELECT work_id FROM publication_work WHERE publication_id = ? ORDER BY work_id",
            self.id.get()
        )
        .fetch_all(&mut *tx)
        .await?;
        for work in works {
            subjects.extend(person::commit_contributors_of_work(&mut tx, WorkId::new(work)).await?);
        }
        subjects.extend(person::commit_contributors_of_publication(&mut tx, self.id).await?);
        let mut tx = tx
            .record(
                AuditSource::User,
                Action::Committed,
                Some(&self.audit_subject()),
                None,
            )
            .await?;
        for subject in subjects {
            tx = tx
                .record(AuditSource::User, Action::Committed, Some(&subject), None)
                .await?;
        }
        tx.commit().await?;
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
    let (title, identifier) = match &seed {
        ImportSeed::Title(title) => (Some(clean_title(title)?), None),
        ImportSeed::Identifier(identifier) => (None, Some(identifier)),
    };
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
    if let Some(identifier) = identifier {
        let kind = identifier.kind().to_string();
        sqlx::query!(
            "INSERT INTO publication_identifier (publication_id, kind, value) VALUES (?, ?, ?)",
            id,
            kind,
            identifier.as_str()
        )
        .execute(&mut *tx)
        .await?;
    }
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

/// Get publication search candidates
pub(crate) async fn candidates(
    conn: &mut SqliteConnection,
    scope: CandidateScope,
) -> Result<Vec<Candidate>> {
    let (library, include_drafts, include_private) = scope.binds();
    let rows = sqlx::query!(
        r#"SELECT p.id AS "id!", coalesce(p.title, '') AS "name!: String",
                  coalesce(p.title, '') || ' '
                  || coalesce((SELECT group_concat(person.name, ' ')
                               FROM publication_contributor c JOIN person ON person.id = c.person_id
                               WHERE c.publication_id = p.id), '') || ' '
                  || coalesce((SELECT group_concat(value || ' ' || replace(value, '-', ''), ' ')
                               FROM publication_identifier WHERE publication_id = p.id), '')
                  AS "text!: String"
           FROM publication p JOIN library l ON l.id = p.library_id
           WHERE (?1 IS NULL OR p.library_id = ?1) AND (?2 OR p.draft = 0) AND (?3 OR l.private = 0)
           ORDER BY p.id"#,
        library,
        include_drafts,
        include_private
    )
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| Candidate {
            id: row.id,
            name: row.name,
            text: row.text,
        })
        .collect())
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

/// List every publication in `library` tagged `tag`
pub(crate) async fn tagged(
    engine: &Engine,
    library: LibraryId,
    tag: &Tag,
) -> Result<Vec<Publication>> {
    let mut conn = engine.begin_read().await?;
    let library = library.get();
    let tag = tag.as_str();
    let rows = sqlx::query_as!(
        PublicationRow,
        r#"SELECT publication.id, publication.library_id, publication.title, publication.publisher,
                  publication.year AS "year: u16", publication.stars AS "stars: u8",
                  publication.note, publication.kind, publication.draft AS "draft: bool",
                  publication.created_at AS "created_at: Timestamp",
                  publication.last_modified_at AS "last_modified_at: Timestamp"
           FROM publication JOIN tag ON tag.publication_id = publication.id
           WHERE tag.library_id = ? AND tag.tag = ?
           ORDER BY publication.id"#,
        library,
        tag
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

    /// Get a compact form of the identifier
    pub fn compact(&self) -> String {
        self.text.replace('-', "")
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

    #[tokio::test]
    async fn commit_cascades_through_works_to_people() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), EngineOptions::default())
            .await
            .unwrap();
        let library = engine.create_library("Sheet music", false).await.unwrap();
        let mut publication = library
            .create_publication(
                ImportSeed::Title("Three gymnopedies for the piano".into()),
                NewHolding::Physical { location: None },
            )
            .await
            .unwrap();
        let mut work = publication.create_work("Gymnopedie No. 1").await.unwrap();
        let person = work
            .create_contributor("Erik Satie", "composer")
            .await
            .unwrap();
        let editor = publication
            .create_contributor("Isidor Philipp", "editor")
            .await
            .unwrap();
        assert!(publication.is_draft());
        assert!(work.is_draft());
        assert!(person.is_draft());
        assert!(editor.is_draft());

        publication.commit().await.unwrap();

        let work = Work::load(&engine, work.id()).await.unwrap().unwrap();
        let person = Person::load(&engine, person.id()).await.unwrap().unwrap();
        let editor = Person::load(&engine, editor.id()).await.unwrap().unwrap();
        assert!(!work.is_draft());
        assert!(!person.is_draft());
        assert!(!editor.is_draft());
    }

    #[tokio::test]
    async fn delete_collects_orphans() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), EngineOptions::default())
            .await
            .unwrap();
        let library = engine.create_library("Sheet music", false).await.unwrap();
        let mut album = library
            .create_publication(
                ImportSeed::Title("Russian piano album".into()),
                NewHolding::Physical { location: None },
            )
            .await
            .unwrap();
        let mut prelude = album.create_work("Prelude in C-sharp minor").await.unwrap();
        let composer = prelude
            .create_contributor("Sergei Rachmaninoff", "composer")
            .await
            .unwrap();
        let editor = album
            .create_contributor("Alexander Siloti", "editor")
            .await
            .unwrap();
        let mut masterpieces = library
            .create_publication(
                ImportSeed::Title("Rachmaninoff masterpieces for solo piano".into()),
                NewHolding::Physical { location: None },
            )
            .await
            .unwrap();
        masterpieces.add_work(prelude.id()).await.unwrap();

        album.delete().await.unwrap();

        assert!(Person::load(&engine, editor.id()).await.unwrap().is_none());
        assert!(Work::load(&engine, prelude.id()).await.unwrap().is_some());
        assert!(
            Person::load(&engine, composer.id())
                .await
                .unwrap()
                .is_some()
        );

        masterpieces.remove_work(prelude.id()).await.unwrap();

        assert!(Work::load(&engine, prelude.id()).await.unwrap().is_none());
        assert!(
            Person::load(&engine, composer.id())
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn contributors_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), EngineOptions::default())
            .await
            .unwrap();
        let library = engine.create_library("Sheet music", false).await.unwrap();
        let mut publication = library
            .create_publication(
                ImportSeed::Title("Three gymnopedies for the piano".into()),
                NewHolding::Physical { location: None },
            )
            .await
            .unwrap();
        let satie = publication
            .create_contributor("Erik Satie", "composer")
            .await
            .unwrap()
            .id();
        publication.add_contributor(satie, "editor").await.unwrap();
        publication.reload().await.unwrap();
        let contributor = |role: &str| Contributor {
            person: satie,
            name: "Erik Satie".into(),
            role: role.into(),
        };
        assert_eq!(
            publication.contributors(),
            [contributor("composer"), contributor("editor")]
        );

        publication
            .remove_contributor(satie, "composer")
            .await
            .unwrap();
        publication.reload().await.unwrap();
        assert_eq!(publication.contributors(), [contributor("editor")]);

        publication
            .remove_contributor(satie, "editor")
            .await
            .unwrap();
        assert!(Person::load(&engine, satie).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn tags_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), EngineOptions::default())
            .await
            .unwrap();
        let library = engine.create_library("Sheet music", false).await.unwrap();
        let mut publication = library
            .create_publication(
                ImportSeed::Title("Three gymnopedies for the piano".into()),
                NewHolding::Physical { location: None },
            )
            .await
            .unwrap();
        let piano: Tag = "piano".parse().unwrap();
        let duet: Tag = "duet".parse().unwrap();
        publication.add_tag(piano.clone()).await.unwrap();
        publication.add_tag(duet.clone()).await.unwrap();
        assert_eq!(publication.tags(), [duet.clone(), piano.clone()]);
        publication.reload().await.unwrap();
        assert_eq!(publication.tags(), [duet.clone(), piano.clone()]);

        publication.remove_tag(&duet).await.unwrap();
        publication.reload().await.unwrap();
        assert_eq!(publication.tags(), [piano]);
    }

    #[tokio::test]
    async fn identifier_seed() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), EngineOptions::default())
            .await
            .unwrap();
        let library = engine.create_library("Books", false).await.unwrap();
        let isbn = Identifier::new(IdentifierKind::Isbn, "978-1-68050-127-8").unwrap();
        let mut publication = library
            .create_publication(
                ImportSeed::Identifier(isbn.clone()),
                NewHolding::Physical { location: None },
            )
            .await
            .unwrap();
        let events: Vec<_> = publication
            .audit_log(None, 10)
            .await
            .unwrap()
            .into_iter()
            .map(|e| (e.action, e.subject))
            .collect();
        assert_eq!(
            events,
            [(
                Action::Created,
                Some(AuditSubject::Entity {
                    kind: EntityKind::Publication,
                    id: publication.id().get(),
                    library: library.id(),
                    label: "978-1-68050-127-8".into(),
                })
            )]
        );
        assert!(matches!(
            publication.commit().await,
            Err(Error::InvalidValue(_))
        ));

        publication.set_title("Practical Vim").await.unwrap();
        publication.reload().await.unwrap();
        assert_eq!(publication.identifiers(), [isbn]);
        assert_eq!(publication.title(), Some("Practical Vim"));
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
