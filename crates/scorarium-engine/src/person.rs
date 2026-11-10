use std::collections::HashMap;
use std::fmt;
use std::num::ParseIntError;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use scorarium_client::Source;
use sqlx::SqliteConnection;

use crate::asset::{self, AssetSource};
use crate::audit::{Action, AuditPage, AuditSource, AuditSubject, Audited};
use crate::contributor::Role;
use crate::enrich::Enrichment;
use crate::entity::{self, AnyEntity, AnyEntityId, Entity, EntityKind, ProposalId, Timestamp};
use crate::library::LibraryId;
use crate::link::{ExternalRecord, HttpUrl, Link, LinkId};
use crate::publication::{self, Publication, PublicationId};
use crate::review::{self, ReviewItem};
use crate::search::same_name;
use crate::suggest::{Candidate, CandidateScope};
use crate::work::{self, Work, WorkId};
use crate::{Engine, Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PersonId(i64);

impl PersonId {
    pub(crate) fn new(id: i64) -> PersonId {
        PersonId(id)
    }

    pub fn get(self) -> i64 {
        self.0
    }
}

impl fmt::Display for PersonId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for PersonId {
    type Err = ParseIntError;

    fn from_str(text: &str) -> std::result::Result<PersonId, ParseIntError> {
        text.parse().map(PersonId)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    Name,
    Links,
    Profile,
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Field::Name => "name",
            Field::Links => "links",
            Field::Profile => "profile",
        })
    }
}

impl FromStr for Field {
    type Err = Error;

    fn from_str(text: &str) -> Result<Field> {
        Ok(match text {
            "name" => Field::Name,
            "links" => Field::Links,
            "profile" => Field::Profile,
            other => return Err(eyre::eyre!("unknown person field {other:?}").into()),
        })
    }
}

/// A proposed edit to a [Person]
#[derive(Clone, Debug)]
pub enum Proposal {
    Name(String),
    Link(HttpUrl),
    Profile(HttpUrl),
}

#[derive(Clone, Debug)]
pub struct Person {
    engine: Engine,
    id: PersonId,
    library: LibraryId,
    draft: bool,
    created_at: Timestamp,
    last_modified_at: Timestamp,
    name: String,
    sort_name: String,
    profile: Option<PathBuf>,
    links: Vec<Link>,
}

/// A POD type to serialize a person from the database into
struct PersonRow {
    id: i64,
    library_id: i64,
    name: String,
    sort_name: String,
    profile: Option<String>,
    draft: bool,
    created_at: Timestamp,
    last_modified_at: Timestamp,
}

impl Person {
    /// Return the engine the entity belongs to
    pub(crate) fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn sort_name(&self) -> &str {
        &self.sort_name
    }

    /// List the person's links in the order they were added
    pub fn links(&self) -> &[Link] {
        &self.links
    }

    /// Get the absolute path of the person's profile image
    pub fn profile(&self) -> Option<&Path> {
        self.profile.as_deref()
    }

    async fn from_row(
        engine: &Engine,
        conn: &mut SqliteConnection,
        row: PersonRow,
    ) -> Result<Person> {
        let links = sqlx::query!(
            "SELECT id, url, kind, external_id FROM person_link WHERE person_id = ? ORDER BY id",
            row.id
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(|link| Link::new(link.id, &link.url, &link.kind, link.external_id))
        .collect::<Result<_>>()?;
        Ok(Person {
            engine: engine.clone(),
            id: PersonId::new(row.id),
            library: LibraryId::new(row.library_id),
            draft: row.draft,
            created_at: row.created_at,
            last_modified_at: row.last_modified_at,
            name: row.name,
            sort_name: row.sort_name,
            profile: row.profile.map(|profile| engine.data_dir().join(profile)),
            links,
        })
    }

    pub(crate) async fn fetch(
        engine: &Engine,
        conn: &mut SqliteConnection,
        id: PersonId,
    ) -> Result<Option<Person>> {
        let row = sqlx::query_as!(
            PersonRow,
            r#"SELECT id, library_id, name, sort_name, profile, draft AS "draft: bool",
                      created_at AS "created_at: Timestamp",
                      last_modified_at AS "last_modified_at: Timestamp"
               FROM person WHERE id = ?"#,
            id.get()
        )
        .fetch_optional(&mut *conn)
        .await?;
        match row {
            Some(row) => Ok(Some(Person::from_row(engine, conn, row).await?)),
            None => Ok(None),
        }
    }

    async fn refresh(&mut self, conn: &mut SqliteConnection) -> Result<()> {
        *self = Person::fetch(&self.engine, conn, self.id)
            .await?
            .ok_or(Error::NotFound)?;
        Ok(())
    }

    pub(crate) fn audit_subject(&self) -> AuditSubject {
        audit_subject(self.id, self.library, &self.name)
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

    /// Rename the person along with the given sort name
    ///
    /// Returns [Error::NotFound] if the person has been deleted.
    pub async fn rename(
        &mut self,
        name: impl AsRef<str>,
        sort_name: impl AsRef<str>,
    ) -> Result<()> {
        let name = clean_name(name.as_ref())?;
        let sort_name = sort_name.as_ref().trim();
        if sort_name.is_empty() {
            return Err(Error::InvalidValue("A sort name cannot be blank.".into()));
        }
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        if self.name == name && self.sort_name == sort_name {
            return Ok(());
        }
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE person SET name = ?, sort_name = ?, last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            name,
            sort_name,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        self.commit_update(tx, Field::Name).await?;
        self.name = name.to_string();
        self.sort_name = sort_name.to_string();
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Replace the person's profile image with the image from `source`
    ///
    /// Returns [Error::InvalidValue] if the file is not recognized as an image or is larger than
    /// the limit, or [Error::NotFound] if the person has been deleted.
    pub async fn set_profile(&mut self, source: AssetSource<'_>) -> Result<()> {
        let staged = asset::stage(&self.engine, source).await?;
        let extension = staged.image_extension()?;
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        let relative = format!("assets/person/{}/profile.{extension}", self.id);
        let path = self.engine.data_dir().join(&relative);
        staged.persist(&path)?;
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE person SET profile = ?, last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            relative,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        self.commit_update(tx, Field::Profile).await?;
        self.last_modified_at = last_modified_at;
        if let Some(previous) = self.profile.replace(path.clone())
            && previous != path
        {
            remove_profile(&previous);
        }
        Ok(())
    }

    /// Remove the person's profile image
    ///
    /// Returns [Error::NotFound] if the person has been deleted.
    pub async fn clear_profile(&mut self) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        if self.profile.is_none() {
            return Ok(());
        }
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE person SET profile = NULL, last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        self.commit_update(tx, Field::Profile).await?;
        self.last_modified_at = last_modified_at;
        if let Some(previous) = self.profile.take() {
            remove_profile(&previous);
        }
        Ok(())
    }

    /// Link the person to `url`
    ///
    /// Returns [Error::NotFound] if the person has been deleted.
    pub async fn add_link(&mut self, url: HttpUrl) -> Result<()> {
        let record = ExternalRecord::recognize(EntityKind::Person, &url);
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
            r#"INSERT INTO person_link (person_id, url, kind, external_id) VALUES (?, ?, ?, ?)
               RETURNING id AS "id!""#,
            self.id.get(),
            url.as_url().as_str(),
            kind,
            external_id
        )
        .fetch_one(&mut *tx)
        .await?;
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE person SET last_modified_at = unixepoch() WHERE id = ?
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

    /// Remove a link from the person
    ///
    /// Returns [Error::NotFound] if the person has been deleted.
    pub async fn remove_link(&mut self, link: LinkId) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        let removed = sqlx::query!(
            "DELETE FROM person_link WHERE id = ? AND person_id = ?",
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
            r#"UPDATE person SET last_modified_at = unixepoch() WHERE id = ?
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

    pub(crate) async fn propose(
        &self,
        source: Source,
        value: Proposal,
    ) -> Result<Option<ProposalId>> {
        let (field, text) = match &value {
            Proposal::Name(name) => ("name", clean_name(name)?.to_string()),
            Proposal::Link(url) => ("link", url.to_string()),
            Proposal::Profile(url) => ("profile", url.to_string()),
        };
        let mut tx = self.engine.begin_write().await?;
        let current = Person::fetch(&self.engine, &mut tx, self.id)
            .await?
            .ok_or(Error::NotFound)?;
        let has = match &value {
            Proposal::Name(_) => current.name == text,
            Proposal::Link(url) => {
                let record = ExternalRecord::recognize(EntityKind::Person, url);
                current.links.iter().any(|link| {
                    link.url == *url || (link.record.is_some() && link.record == record)
                })
            }
            // A stored file cannot be compared to a URL.
            Proposal::Profile(_) => false,
        };
        if has {
            return Ok(None);
        }
        let source = source.to_string();
        let id = sqlx::query_scalar!(
            r#"INSERT INTO person_proposal (library_id, person_id, field, source, proposed_value)
               VALUES (?, ?, ?, ?, ?)
               ON CONFLICT DO NOTHING
               RETURNING id AS "id!""#,
            self.library.get(),
            self.id.get(),
            field,
            source,
            text
        )
        .fetch_optional(&mut *tx)
        .await?;
        #[expect(deprecated, reason = "making a proposal does not need an audit event")]
        tx.commit_unaudited().await?;
        Ok(id.map(ProposalId::new))
    }

    /// List every publication that names the person as a contributor
    pub async fn publications(&self) -> Result<Vec<Publication>> {
        publication::with_contributor(&self.engine, self.id).await
    }

    /// List every work that names the person as a contributor
    pub async fn works(&self) -> Result<Vec<Work>> {
        work::with_contributor(&self.engine, self.id).await
    }
}

impl Entity for Person {
    type Id = PersonId;
    type Field = Field;
    type Proposal = Proposal;
    const KIND: EntityKind = EntityKind::Person;

    async fn load(engine: &Engine, id: PersonId) -> Result<Option<Person>> {
        let mut conn = engine.begin_read().await?;
        Person::fetch(engine, &mut conn, id).await
    }

    async fn reload(&mut self) -> Result<()> {
        let mut conn = self.engine.begin_read().await?;
        self.refresh(&mut conn).await
    }

    fn id(&self) -> PersonId {
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

    async fn audit_log(&self, offset: usize, limit: usize) -> Result<AuditPage> {
        self.engine
            .entity_audit_log(AnyEntityId::Person(self.id), self.library, offset, limit)
            .await
    }

    async fn review_item(&self) -> Result<Option<ReviewItem>> {
        review::item(&self.engine, self.id, AnyEntity::Person).await
    }

    async fn proposals(&self) -> Result<Vec<entity::Proposal<Person>>> {
        let mut conn = self.engine.begin_read().await?;
        let rows = sqlx::query_as!(
            ProposalRow,
            r#"SELECT id, field, source, proposed_value,
                      created_at AS "created_at: Timestamp"
               FROM person_proposal WHERE person_id = ? ORDER BY id"#,
            self.id.get()
        )
        .fetch_all(&mut *conn)
        .await?;
        rows.into_iter().map(from_row).collect()
    }

    async fn accept(&mut self, proposal: ProposalId) -> Result<()> {
        let mut conn = self.engine.begin_read().await?;
        let proposal = self
            .fetch_proposal(&mut conn, proposal)
            .await?
            .ok_or(Error::NotFound)?;
        drop(conn);
        match proposal.value {
            Proposal::Name(name) => self.rename(&name, derive_sort_name(&name)).await?,
            Proposal::Link(url) => self.add_link(url).await?,
            Proposal::Profile(url) => self.set_profile(AssetSource::Url(url)).await?,
        }
        self.discard_proposal(proposal.id).await
    }

    async fn reject(&mut self, proposal: ProposalId) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        let rejected = sqlx::query!(
            "DELETE FROM person_proposal WHERE id = ? AND person_id = ?",
            proposal.get(),
            self.id.get()
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if rejected == 0 {
            return Err(Error::NotFound);
        }
        #[expect(
            deprecated,
            reason = "rejecting proposals does not need an audit event"
        )]
        tx.commit_unaudited().await?;
        self.engine
            .forget_enrichment_if_reviewed(AnyEntityId::Person(self.id))
            .await;
        Ok(())
    }

    async fn reject_all(&mut self) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        sqlx::query!(
            "DELETE FROM person_proposal WHERE person_id = ?",
            self.id.get()
        )
        .execute(&mut *tx)
        .await?;
        #[expect(
            deprecated,
            reason = "rejecting proposals does not need an audit event"
        )]
        tx.commit_unaudited().await?;
        self.engine
            .forget_enrichment_if_reviewed(AnyEntityId::Person(self.id))
            .await;
        Ok(())
    }

    async fn commit(&mut self) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        if !self.draft {
            return Ok(());
        }
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE person SET draft = 0, last_modified_at = unixepoch() WHERE id = ?
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
        self.engine
            .forget_completed_enrichment(AnyEntityId::Person(self.id));
        Ok(())
    }

    /// Delete the person
    ///
    /// Returns [Error::NotFound] if the person has already been deleted.
    async fn delete(mut self) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        sqlx::query!("DELETE FROM person WHERE id = ?", self.id.get())
            .execute(&mut *tx)
            .await?;
        tx.record(
            AuditSource::User,
            Action::Deleted,
            Some(&self.audit_subject()),
            None,
        )
        .await?
        .commit()
        .await?;
        asset::remove_dirs(&self.engine, &[self.audit_subject()]);
        self.engine.forget_enrichment(AnyEntityId::Person(self.id));
        Ok(())
    }

    async fn enrich(&self) -> Result<Enrichment> {
        Ok(self
            .engine
            .start_enrichment(AnyEntityId::Person(self.id), |_| {}))
    }

    fn enrichment(&self) -> Option<Enrichment> {
        self.engine.enrichment_of(AnyEntityId::Person(self.id))
    }
}

impl Person {
    async fn fetch_proposal(
        &self,
        conn: &mut SqliteConnection,
        id: ProposalId,
    ) -> Result<Option<entity::Proposal<Person>>> {
        let row = sqlx::query_as!(
            ProposalRow,
            r#"SELECT id, field, source, proposed_value,
                      created_at AS "created_at: Timestamp"
               FROM person_proposal WHERE id = ? AND person_id = ?"#,
            id.get(),
            self.id.get()
        )
        .fetch_optional(conn)
        .await?;
        row.map(from_row).transpose()
    }

    async fn discard_proposal(&self, proposal: ProposalId) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        sqlx::query!(
            "DELETE FROM person_proposal WHERE id = ? AND person_id = ?",
            proposal.get(),
            self.id.get()
        )
        .execute(&mut *tx)
        .await?;
        #[expect(
            deprecated,
            reason = "discarding proposals does not need an audit event"
        )]
        tx.commit_unaudited().await?;
        self.engine
            .forget_enrichment_if_reviewed(AnyEntityId::Person(self.id))
            .await;
        Ok(())
    }
}

/// A POD type to deserialize a person proposal from the database into
struct ProposalRow {
    id: i64,
    field: String,
    source: String,
    proposed_value: String,
    created_at: Timestamp,
}

fn from_row(row: ProposalRow) -> Result<entity::Proposal<Person>> {
    let value = match row.field.as_str() {
        "name" => Proposal::Name(row.proposed_value),
        "link" => Proposal::Link(row.proposed_value.parse()?),
        "profile" => Proposal::Profile(row.proposed_value.parse()?),
        other => return Err(eyre::eyre!("unknown person proposal field {other:?}").into()),
    };
    Ok(entity::Proposal {
        id: ProposalId::new(row.id),
        source: row.source.parse()?,
        created_at: row.created_at,
        value,
    })
}

/// Insert a person named `name` in `library` with a derived sort name and load them
pub(crate) async fn insert(
    engine: &Engine,
    conn: &mut SqliteConnection,
    library: LibraryId,
    name: &str,
    draft: bool,
) -> Result<Person> {
    let library = library.get();
    let sort_name = derive_sort_name(name);
    let id = sqlx::query_scalar!(
        r#"INSERT INTO person (library_id, name, sort_name, draft) VALUES (?, ?, ?, ?)
           RETURNING id AS "id!""#,
        library,
        name,
        sort_name,
        draft
    )
    .fetch_one(&mut *conn)
    .await?;
    Person::fetch(engine, conn, PersonId::new(id))
        .await?
        .ok_or(Error::NotFound)
}

/// Get the name and sort name of `person`, or none if `library` has no such person
pub(crate) async fn names(
    conn: &mut SqliteConnection,
    library: LibraryId,
    person: PersonId,
) -> Result<Option<(String, String)>> {
    let names = sqlx::query!(
        "SELECT name, sort_name FROM person WHERE id = ? AND library_id = ?",
        person.get(),
        library.get()
    )
    .fetch_optional(conn)
    .await?
    .map(|row| (row.name, row.sort_name));
    Ok(names)
}

/// List the people with the given `role` in `library`
pub(crate) async fn in_library(
    engine: &Engine,
    library: LibraryId,
    role: Option<&Role>,
) -> Result<Vec<Person>> {
    let mut conn = engine.begin_read().await?;
    let library = library.get();
    let rows = match role.map(Role::as_str) {
        Some(role) => {
            sqlx::query_as!(
                PersonRow,
                r#"SELECT id, library_id, name, sort_name, profile, draft AS "draft: bool",
                          created_at AS "created_at: Timestamp",
                          last_modified_at AS "last_modified_at: Timestamp"
                   FROM person
                   WHERE library_id = ?
                     AND (EXISTS (SELECT 1 FROM publication_contributor
                                  WHERE person_id = person.id AND role = ?)
                          OR EXISTS (SELECT 1 FROM work_contributor
                                     WHERE person_id = person.id AND role = ?))
                   ORDER BY id"#,
                library,
                role,
                role
            )
            .fetch_all(&mut *conn)
            .await?
        }
        None => {
            sqlx::query_as!(
                PersonRow,
                r#"SELECT id, library_id, name, sort_name, profile, draft AS "draft: bool",
                          created_at AS "created_at: Timestamp",
                          last_modified_at AS "last_modified_at: Timestamp"
                   FROM person WHERE library_id = ? ORDER BY id"#,
                library
            )
            .fetch_all(&mut *conn)
            .await?
        }
    };
    let mut people = Vec::with_capacity(rows.len());
    for row in rows {
        people.push(Person::from_row(engine, &mut conn, row).await?);
    }
    Ok(people)
}

/// A person in a listing, with the roles they hold and how often they are credited
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersonSummary {
    pub id: PersonId,
    pub name: String,
    pub sort_name: String,
    /// The absolute path of the profile image just like [Person::profile]
    pub profile: Option<PathBuf>,
    pub roles: Vec<Role>,
    pub publications: usize,
    pub works: usize,
}

/// Summarize the people in `library` with their visible roles and counts
pub(crate) async fn summarize(
    engine: &Engine,
    library: LibraryId,
    role: Option<&Role>,
    include_drafts: bool,
) -> Result<Vec<PersonSummary>> {
    let mut conn = engine.begin_read().await?;
    let library = library.get();
    let role_rows = sqlx::query!(
        r#"SELECT person.id, person.name, person.sort_name, person.profile, held.role AS "role!"
           FROM person
           JOIN (
               SELECT pc.person_id, pc.role, publication.draft
               FROM publication_contributor AS pc
               JOIN publication ON publication.id = pc.publication_id
               WHERE pc.library_id = ?1
               UNION ALL
               SELECT wc.person_id, wc.role, work.draft
               FROM work_contributor AS wc
               JOIN work ON work.id = wc.work_id
               WHERE wc.library_id = ?1
           ) AS held ON held.person_id = person.id
           WHERE person.library_id = ?1 AND (?2 OR (person.draft = 0 AND held.draft = 0))
           GROUP BY person.id, held.role
           ORDER BY person.id, held.role"#,
        library,
        include_drafts
    )
    .fetch_all(&mut *conn)
    .await?;
    let count_rows = sqlx::query!(
        r#"SELECT credited.person_id,
                  COUNT(DISTINCT credited.publication_id) AS "publications: u32",
                  COUNT(DISTINCT credited.work_id) AS "works: u32"
           FROM (
               SELECT pc.person_id, pc.publication_id, NULL AS work_id
               FROM publication_contributor AS pc
               JOIN publication ON publication.id = pc.publication_id
               WHERE pc.library_id = ?1 AND (?2 OR publication.draft = 0)
               UNION ALL
               SELECT wc.person_id, publication.id AS publication_id, wc.work_id
               FROM work_contributor AS wc
               JOIN work ON work.id = wc.work_id
               LEFT JOIN publication_work AS pw ON pw.work_id = wc.work_id
               LEFT JOIN publication ON publication.id = pw.publication_id
                                    AND (?2 OR publication.draft = 0)
               WHERE wc.library_id = ?1 AND (?2 OR work.draft = 0)
           ) AS credited
           GROUP BY credited.person_id"#,
        library,
        include_drafts
    )
    .fetch_all(&mut *conn)
    .await?;
    let counts: HashMap<i64, (usize, usize)> = count_rows
        .into_iter()
        .map(|row| {
            let counts = (row.publications as usize, row.works as usize);
            (row.person_id, counts)
        })
        .collect();

    let mut summaries: Vec<PersonSummary> = Vec::new();
    for row in role_rows {
        let held = Role::from_valid(row.role);
        match summaries.last_mut() {
            Some(summary) if summary.id.get() == row.id => summary.roles.push(held),
            _ => {
                let (publications, works) = counts.get(&row.id).copied().unwrap_or_default();
                summaries.push(PersonSummary {
                    id: PersonId::new(row.id),
                    name: row.name,
                    sort_name: row.sort_name,
                    profile: row.profile.map(|profile| engine.data_dir().join(profile)),
                    roles: vec![held],
                    publications,
                    works,
                });
            }
        }
    }
    summaries.retain(|summary| role.is_none_or(|role| summary.roles.contains(role)));
    summaries
        .sort_by_cached_key(|summary| (summary.sort_name.to_lowercase(), summary.name.clone()));
    Ok(summaries)
}

/// Get person search candidates from the given library
pub(crate) async fn candidates(
    conn: &mut SqliteConnection,
    scope: CandidateScope,
) -> Result<Vec<Candidate>> {
    let (library, include_drafts, include_private) = scope.binds();
    let rows = sqlx::query!(
        r#"SELECT p.id, p.name, p.name AS text
           FROM person p JOIN library l ON l.id = p.library_id
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

/// Find the people in `library`, drafts included, whose name is `name` after normalization
pub(crate) async fn find_by_name(
    engine: &Engine,
    library: LibraryId,
    name: &str,
) -> Result<Vec<Person>> {
    let mut conn = engine.begin_read().await?;
    let mut people = Vec::new();
    // Normalization happens in Rust, so SQL cannot compare names.
    for candidate in candidates(&mut conn, CandidateScope::Library(library)).await? {
        if same_name(&candidate.name, name) {
            let person = Person::fetch(engine, &mut conn, PersonId::new(candidate.id)).await?;
            people.extend(person);
        }
    }
    Ok(people)
}

/// Find the people in `library`, drafts included, linked to `record`
pub(crate) async fn find_by_record(
    engine: &Engine,
    library: LibraryId,
    record: &ExternalRecord,
) -> Result<Vec<Person>> {
    let people = in_library(engine, library, None).await?;
    Ok(people
        .into_iter()
        .filter(|person| {
            person
                .links()
                .iter()
                .any(|link| link.record.as_ref() == Some(record))
        })
        .collect())
}

/// Clear the draft mark of every draft person named as a contributor of `publication`
///
/// Return the audit subject of each person committed, in id order.
pub(crate) async fn commit_contributors_of_publication(
    conn: &mut SqliteConnection,
    publication: PublicationId,
) -> Result<Vec<AuditSubject>> {
    let mut people = sqlx::query!(
        "UPDATE person SET draft = 0, last_modified_at = unixepoch()
         WHERE draft = 1
           AND id IN (SELECT person_id FROM publication_contributor WHERE publication_id = ?)
         RETURNING id, library_id, name",
        publication.get()
    )
    .fetch_all(conn)
    .await?;
    // SQLite does not define the order of RETURNING rows
    people.sort_by_key(|person| person.id);
    Ok(people
        .into_iter()
        .map(|person| {
            audit_subject(
                PersonId::new(person.id),
                LibraryId::new(person.library_id),
                &person.name,
            )
        })
        .collect())
}

/// Clear the draft mark of every draft person named as a contributor of `work`
///
/// Return the audit subject of each person committed, in id order.
pub(crate) async fn commit_contributors_of_work(
    conn: &mut SqliteConnection,
    work: WorkId,
) -> Result<Vec<AuditSubject>> {
    let mut people = sqlx::query!(
        "UPDATE person SET draft = 0, last_modified_at = unixepoch()
         WHERE draft = 1
           AND id IN (SELECT person_id FROM work_contributor WHERE work_id = ?)
         RETURNING id, library_id, name",
        work.get()
    )
    .fetch_all(conn)
    .await?;
    // SQLite does not define the order of RETURNING rows
    people.sort_by_key(|person| person.id);
    Ok(people
        .into_iter()
        .map(|person| {
            audit_subject(
                PersonId::new(person.id),
                LibraryId::new(person.library_id),
                &person.name,
            )
        })
        .collect())
}

/// Delete every person in `library` not referenced by another entity
///
/// Return the audit subject of each person deleted, in id order.
pub(crate) async fn collect_orphan_people(
    conn: &mut SqliteConnection,
    library: LibraryId,
) -> Result<Vec<AuditSubject>> {
    let mut people = sqlx::query!(
        "DELETE FROM person
         WHERE library_id = ?
           AND id NOT IN (SELECT person_id FROM publication_contributor)
           AND id NOT IN (SELECT person_id FROM work_contributor)
           AND id NOT IN (SELECT person_id FROM publication_proposal WHERE person_id IS NOT NULL)
           AND id NOT IN (SELECT person_id FROM work_proposal WHERE person_id IS NOT NULL)
         RETURNING id AS \"id!\", name",
        library.get()
    )
    .fetch_all(conn)
    .await?;
    // SQLite does not define the order of RETURNING rows
    people.sort_by_key(|person| person.id);
    Ok(people
        .into_iter()
        .map(|person| audit_subject(PersonId::new(person.id), library, &person.name))
        .collect())
}

/// Remove a profile image that a committed transaction no longer refers to
fn remove_profile(path: &Path) {
    // The db record has already removed the path, so bubbling up an error would tell the caller the
    // update failed, but that's not entirely true, because it's been removed from the db. So just
    // log a warning and move on.
    if let Err(e) = std::fs::remove_file(path) {
        tracing::warn!("failed to remove {}: {e}", path.display());
    }
}

fn audit_subject(id: PersonId, library: LibraryId, name: &str) -> AuditSubject {
    AuditSubject::Entity {
        id: AnyEntityId::Person(id),
        library,
        label: name.to_string(),
    }
}

pub(crate) fn clean_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::InvalidValue(
            "A person's name cannot be blank.".into(),
        ));
    }
    Ok(name)
}

/// Derive a sort name, so that "Erik Satie" sorts as "Satie, Erik"
///
/// Compound surnames like "Ralph Vaughan Williams" come out wrong. That is acceptable because the
/// heuristic only has to be right often enough that the user rarely retypes a name.
pub fn derive_sort_name(name: &str) -> String {
    let mut parts: Vec<&str> = name.split_whitespace().collect();
    match parts.pop() {
        Some(last) if !parts.is_empty() => format!("{last}, {}", parts.join(" ")),
        _ => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::Contributed;
    use crate::holding::NewHolding;
    use crate::link::LinkKind;
    use crate::publication::ImportSeed;

    #[tokio::test]
    async fn links_collapse_on_record() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), crate::test::options())
            .await
            .unwrap();
        let library = engine.create_library("Sheet music", false).await.unwrap();
        let mut publication = library
            .import(
                ImportSeed::Title {
                    title: "Russian piano album".into(),
                    record: None,
                },
                NewHolding::Physical { location: None },
            )
            .await
            .unwrap();
        let mut person = publication
            .create_contributor("Sergei Rachmaninoff", "composer".parse().unwrap())
            .await
            .unwrap();
        for url in [
            "https://www.wikidata.org/wiki/Q131861",
            "https://m.wikidata.org/wiki/Q131861",
        ] {
            person.add_link(url.parse().unwrap()).await.unwrap();
        }
        person.reload().await.unwrap();

        let [link] = person.links() else {
            panic!("expected one link, got {:?}", person.links());
        };
        assert_eq!(
            link.url.to_string(),
            "https://www.wikidata.org/wiki/Q131861"
        );
        assert_eq!(
            link.record,
            Some(ExternalRecord::new(LinkKind::Wikidata, "Q131861"))
        );
    }

    #[tokio::test]
    async fn profile_images() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), crate::test::options())
            .await
            .unwrap();
        let library = engine.create_library("Sheet music", false).await.unwrap();
        let mut publication = library
            .import(
                ImportSeed::Title {
                    title: "Gymnopedies".into(),
                    record: None,
                },
                NewHolding::Physical { location: None },
            )
            .await
            .unwrap();
        let mut person = publication
            .create_contributor("Erik Satie", "composer".parse().unwrap())
            .await
            .unwrap();
        let staging = dir.path().join("staging");
        let upload = |bytes: &'static [u8]| AssetSource::Upload {
            data: Box::new(bytes),
            filename: "anything.bin".into(),
        };
        let png: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\0";
        let jpeg: &[u8] = b"\xff\xd8\xff\xe0\0\0\0\0";

        assert_eq!(person.profile(), None);
        person.set_profile(upload(png)).await.unwrap();
        let expected = dir
            .path()
            .join(format!("assets/person/{}/profile.png", person.id()));
        assert_eq!(person.profile(), Some(expected.as_path()));
        assert_eq!(std::fs::read(&expected).unwrap(), png);
        assert_eq!(std::fs::read_dir(&staging).unwrap().count(), 0);

        assert!(matches!(
            person.set_profile(upload(b"not an image")).await,
            Err(Error::InvalidValue(_))
        ));
        assert_eq!(person.profile(), Some(expected.as_path()));
        assert_eq!(std::fs::read_dir(&staging).unwrap().count(), 0);

        person.set_profile(upload(jpeg)).await.unwrap();
        let replaced = expected.with_extension("jpg");
        assert_eq!(person.profile(), Some(replaced.as_path()));
        assert!(!expected.exists());
        assert_eq!(std::fs::read(&replaced).unwrap(), jpeg);

        person.clear_profile().await.unwrap();
        assert_eq!(person.profile(), None);
        assert!(!replaced.exists());

        person.set_profile(upload(png)).await.unwrap();
        publication.delete().await.unwrap();
        assert!(
            !dir.path()
                .join(format!("assets/person/{}", person.id()))
                .exists()
        );
    }

    #[tokio::test]
    async fn people_summary_counts_credits_once_per_entity() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), crate::test::options())
            .await
            .unwrap();
        let library = engine.create_library("Sheet music", false).await.unwrap();
        let seed = |title: &str| ImportSeed::Title {
            title: title.into(),
            record: None,
        };

        let mut nocturnes = library
            .import(seed("Nocturnes"), NewHolding::Physical { location: None })
            .await
            .unwrap();
        let chopin = nocturnes
            .create_contributor("Frederic Chopin", "editor".parse().unwrap())
            .await
            .unwrap();
        for title in ["Nocturne No. 1", "Nocturne No. 2"] {
            let mut work = nocturnes.create_work(title).await.unwrap();
            work.add_contributor(chopin.id(), "composer".parse().unwrap())
                .await
                .unwrap();
        }
        nocturnes.commit().await.unwrap();
        let mut draft = library
            .import(seed("Untitled"), NewHolding::Physical { location: None })
            .await
            .unwrap();
        draft
            .add_contributor(chopin.id(), "arranger".parse().unwrap())
            .await
            .unwrap();

        let summarize = async |include_drafts| {
            let people = library.people_summary(None, include_drafts).await.unwrap();
            people
                .iter()
                .map(|p| {
                    let roles: Vec<_> = p.roles.iter().map(Role::to_string).collect();
                    (roles, p.publications, p.works)
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            summarize(false).await,
            [(vec!["composer".into(), "editor".into()], 1, 2)]
        );
        assert_eq!(
            summarize(true).await,
            [(
                vec!["arranger".into(), "composer".into(), "editor".into()],
                2,
                2
            )]
        );
    }
}
