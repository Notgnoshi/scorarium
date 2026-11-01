use crate::entity::{AnyEntity, Entity, Timestamp};
use crate::person::{Person, PersonId};
use crate::publication::{Publication, PublicationId};
use crate::work::{Work, WorkId};
use crate::{Engine, Error, Result};

#[derive(Clone, Debug)]
pub struct ReviewItem {
    pub library_name: String,
    pub private: bool,
    pub entity: AnyEntity,
    /// The timestamp the item was added to the review queue
    pub queued_at: Timestamp,
    /// The number of pending proposals for the entity
    pub proposals: usize,
}

impl ReviewItem {
    /// Remove the entry from the queue
    ///
    /// If the entry is a draft, the draft is deleted. Otherwise every pending proposal is rejected.
    ///
    /// Returns [Error::NotFound] if the entity has already been removed from the queue.
    pub async fn discard(self) -> Result<()> {
        match self.entity {
            AnyEntity::Publication(publication) => discard_entity(publication).await,
            AnyEntity::Work(work) => discard_entity(work).await,
            AnyEntity::Person(person) => discard_entity(person).await,
        }
    }
}

async fn discard_entity<E: Entity>(mut entity: E) -> Result<()> {
    entity.reload().await?;
    if entity.is_draft() {
        entity.delete().await
    } else if !entity.proposals().await?.is_empty() {
        entity.reject_all().await
    } else {
        Err(Error::NotFound)
    }
}

pub(crate) async fn item<E: Entity>(
    engine: &Engine,
    id: E::Id,
    wrap: impl FnOnce(E) -> AnyEntity,
) -> Result<Option<ReviewItem>> {
    let Some(entity) = E::load(engine, id).await? else {
        return Ok(None);
    };
    let proposals = entity.proposals().await?;
    let queued_at = if entity.is_draft() {
        entity.created_at()
    } else if let Some(first) = proposals.first() {
        first.created_at
    } else {
        return Ok(None);
    };
    let library = engine
        .library(entity.library())
        .await?
        .ok_or(Error::NotFound)?;
    Ok(Some(ReviewItem {
        library_name: library.name().to_owned(),
        private: library.is_private(),
        queued_at,
        proposals: proposals.len(),
        entity: wrap(entity),
    }))
}

impl Engine {
    /// List every draft and every entity with pending proposals, oldest first
    pub async fn review_queue(&self) -> Result<Vec<ReviewItem>> {
        let mut conn = self.begin_read().await?;
        // Entities are often queued within the same second, so use the kind and id to break sort ties.
        let rows = sqlx::query!(
            r#"SELECT 0 AS "rank!: i64", publication.id AS "id!: i64",
                      CASE WHEN publication.draft = 1 THEN publication.created_at
                           ELSE (SELECT MIN(created_at) FROM publication_proposal
                                 WHERE publication_id = publication.id) END
                          AS "queued_at!: Timestamp",
                      library.name AS "library_name!: String",
                      library.private AS "private!: bool",
                      (SELECT COUNT(*) FROM publication_proposal WHERE publication_id = publication.id)
                          AS "proposals!: i64"
               FROM publication JOIN library ON library.id = publication.library_id
               WHERE publication.draft = 1 OR EXISTS (SELECT 1 FROM publication_proposal
                                                      WHERE publication_id = publication.id)
               UNION ALL
               SELECT 1, work.id,
                      CASE WHEN work.draft = 1 THEN work.created_at
                           ELSE (SELECT MIN(created_at) FROM work_proposal
                                 WHERE work_id = work.id) END,
                      library.name, library.private,
                      (SELECT COUNT(*) FROM work_proposal WHERE work_id = work.id)
               FROM work JOIN library ON library.id = work.library_id
               WHERE work.draft = 1 OR EXISTS (SELECT 1 FROM work_proposal
                                              WHERE work_id = work.id)
               UNION ALL
               SELECT 2, person.id,
                      CASE WHEN person.draft = 1 THEN person.created_at
                           ELSE (SELECT MIN(created_at) FROM person_proposal
                                 WHERE person_id = person.id) END,
                      library.name, library.private,
                      (SELECT COUNT(*) FROM person_proposal WHERE person_id = person.id)
               FROM person JOIN library ON library.id = person.library_id
               WHERE person.draft = 1 OR EXISTS (SELECT 1 FROM person_proposal
                                              WHERE person_id = person.id)
               ORDER BY 3, 1, 2"#
        )
        .fetch_all(&mut *conn)
        .await?;

        // Querying and loading are separate statements, so an entity deleted in between is left
        // out rather than failing the whole queue.
        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            let entity = match row.rank {
                0 => Publication::fetch(self, &mut conn, PublicationId::new(row.id))
                    .await?
                    .map(AnyEntity::Publication),
                1 => Work::fetch(self, &mut conn, WorkId::new(row.id))
                    .await?
                    .map(AnyEntity::Work),
                _ => Person::fetch(self, &mut conn, PersonId::new(row.id))
                    .await?
                    .map(AnyEntity::Person),
            };
            if let Some(entity) = entity {
                items.push(ReviewItem {
                    library_name: row.library_name,
                    private: row.private,
                    entity,
                    queued_at: row.queued_at,
                    proposals: usize::try_from(row.proposals).expect("a count is never negative"),
                });
            }
        }
        Ok(items)
    }

    /// Count the entries `review_queue` would return
    pub async fn review_count(&self) -> Result<usize> {
        let mut conn = self.begin_read().await?;
        let count = sqlx::query_scalar!(
            r#"SELECT (SELECT COUNT(*) FROM publication
                        WHERE draft = 1 OR EXISTS (SELECT 1 FROM publication_proposal
                                                   WHERE publication_id = publication.id))
                    + (SELECT COUNT(*) FROM work
                        WHERE draft = 1 OR EXISTS (SELECT 1 FROM work_proposal
                                                   WHERE work_id = work.id))
                    + (SELECT COUNT(*) FROM person
                        WHERE draft = 1 OR EXISTS (SELECT 1 FROM person_proposal
                                                   WHERE person_id = person.id)) AS "count!: i64""#
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(usize::try_from(count).expect("a count is never negative"))
    }
}

#[cfg(test)]
mod tests {
    use scorarium_client::Source;

    use super::*;
    use crate::EngineOptions;
    use crate::holding::NewHolding;
    use crate::publication::{ImportSeed, Proposal};

    #[tokio::test]
    async fn discard_refuses_an_entry_no_longer_queued() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), EngineOptions::default())
            .await
            .unwrap();
        let library = engine.create_library("Books", false).await.unwrap();
        let mut publication = library
            .create_publication(
                ImportSeed::Title("Pro Git".into()),
                NewHolding::Physical { location: None },
            )
            .await
            .unwrap();
        let mut queue = engine.review_queue().await.unwrap();
        assert_eq!(queue.len(), 1);

        publication.commit().await.unwrap();

        let item = queue.pop().unwrap();
        assert!(matches!(item.discard().await, Err(Error::NotFound)));
        assert!(
            Publication::load(&engine, publication.id())
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn review_item_answers_for_a_queued_entity_only() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), EngineOptions::default())
            .await
            .unwrap();
        let library = engine.create_library("Books", false).await.unwrap();
        let mut publication = library
            .create_publication(
                ImportSeed::Title("Pro Git".into()),
                NewHolding::Physical { location: None },
            )
            .await
            .unwrap();

        let item = publication.review_item().await.unwrap().unwrap();
        assert_eq!(item.library_name, "Books");
        assert_eq!(item.queued_at, publication.created_at());
        assert!(matches!(item.entity, AnyEntity::Publication(ref p) if p.id() == publication.id()));

        publication.commit().await.unwrap();
        assert!(publication.review_item().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_committed_entity_with_proposals_is_queued_until_discarded() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), EngineOptions::default())
            .await
            .unwrap();
        let library = engine.create_library("Books", false).await.unwrap();
        let mut publication = library
            .create_publication(
                ImportSeed::Title("Pro Git".into()),
                NewHolding::Physical { location: None },
            )
            .await
            .unwrap();
        publication.commit().await.unwrap();
        assert!(engine.review_queue().await.unwrap().is_empty());

        publication
            .propose(Source::OpenLibrary, Proposal::Publisher("Apress".into()))
            .await
            .unwrap();
        let proposal_created_at = publication.proposals().await.unwrap()[0].created_at;

        let mut queue = engine.review_queue().await.unwrap();
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].proposals, 1);
        assert_eq!(queue[0].queued_at, proposal_created_at);
        assert_eq!(engine.review_count().await.unwrap(), 1);

        queue.pop().unwrap().discard().await.unwrap();

        assert!(engine.review_queue().await.unwrap().is_empty());
        assert!(
            Publication::load(&engine, publication.id())
                .await
                .unwrap()
                .is_some()
        );
    }
}
