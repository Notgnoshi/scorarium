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
}

impl ReviewItem {
    /// Remove the entry from the queue
    ///
    /// If the entry is a draft, the draft is deleted.
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
    if !entity.is_draft() {
        return Err(Error::NotFound);
    }
    entity.delete().await
}

impl Engine {
    /// List every draft publication, work, and person in every library, oldest first
    pub async fn review_queue(&self) -> Result<Vec<ReviewItem>> {
        let mut conn = self.begin_read().await?;
        // Drafts are often created within the same second, so use the kind and id break ties to
        // break sort ties.
        let rows = sqlx::query!(
            r#"SELECT 0 AS "rank!: i64", publication.id AS "id!: i64",
                      publication.created_at AS "created_at!: Timestamp",
                      library.name AS "library_name!: String",
                      library.private AS "private!: bool"
               FROM publication JOIN library ON library.id = publication.library_id
               WHERE publication.draft = 1
               UNION ALL
               SELECT 1, work.id, work.created_at, library.name, library.private
               FROM work JOIN library ON library.id = work.library_id
               WHERE work.draft = 1
               UNION ALL
               SELECT 2, person.id, person.created_at, library.name, library.private
               FROM person JOIN library ON library.id = person.library_id
               WHERE person.draft = 1
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
                    queued_at: row.created_at,
                });
            }
        }
        Ok(items)
    }

    /// Count the entries `review_queue` would return
    pub async fn review_count(&self) -> Result<usize> {
        let mut conn = self.begin_read().await?;
        let count = sqlx::query_scalar!(
            r#"SELECT (SELECT COUNT(*) FROM publication WHERE draft = 1)
                    + (SELECT COUNT(*) FROM work WHERE draft = 1)
                    + (SELECT COUNT(*) FROM person WHERE draft = 1) AS "count!: i64""#
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(usize::try_from(count).expect("a count is never negative"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EngineOptions;
    use crate::holding::NewHolding;
    use crate::publication::ImportSeed;

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
}
