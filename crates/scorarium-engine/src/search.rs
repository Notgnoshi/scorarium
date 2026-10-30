use std::collections::HashMap;

use scorarium_archive::fuzzy::{normalize, rank};

use crate::entity::{AnyEntity, Entity, EntityKind};
use crate::library::LibraryId;
use crate::person::{self, Person, PersonId};
use crate::publication::{self, Identifier, IdentifierKind, Publication, PublicationId};
use crate::suggest::{Candidate, CandidateScope};
use crate::work::{self, Work, WorkId};
use crate::{Engine, Result};

#[derive(Clone, Debug)]
pub struct SearchHit {
    pub library: LibraryId,
    pub library_name: String,
    pub private: bool,
    pub entity: AnyEntity,
}

impl Engine {
    /// Find committed publications, works, and people across every library, best match first
    pub async fn search(
        &self,
        query: &str,
        include_private: bool,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        if limit == 0 || normalize(query).is_empty() {
            return Ok(Vec::new());
        }
        let canonical = parse_identifier(query).map(|identifier| identifier.compact());
        let query = canonical.as_deref().unwrap_or(query);
        let mut conn = self.begin_read().await?;
        let scope = CandidateScope::Committed { include_private };
        let mut candidates: Vec<(EntityKind, Candidate)> = Vec::new();
        for candidate in publication::candidates(&mut conn, scope).await? {
            candidates.push((EntityKind::Publication, candidate));
        }
        for candidate in work::candidates(&mut conn, scope).await? {
            candidates.push((EntityKind::Work, candidate));
        }
        for candidate in person::candidates(&mut conn, scope).await? {
            candidates.push((EntityKind::Person, candidate));
        }
        let texts: Vec<&str> = candidates
            .iter()
            .map(|(_, candidate)| candidate.text.as_str())
            .collect();

        // Ranking and loading are separate statements, so an entity deleted in between is left out
        // rather than failing the search.
        let mut entities = Vec::new();
        for i in rank(query, &texts).into_iter().take(limit) {
            let (kind, Candidate { id, .. }) = candidates[i];
            let entity = match kind {
                EntityKind::Publication => {
                    Publication::fetch(self, &mut conn, PublicationId::new(id))
                        .await?
                        .map(AnyEntity::Publication)
                }
                EntityKind::Work => Work::fetch(self, &mut conn, WorkId::new(id))
                    .await?
                    .map(AnyEntity::Work),
                EntityKind::Person => Person::fetch(self, &mut conn, PersonId::new(id))
                    .await?
                    .map(AnyEntity::Person),
            };
            entities.extend(entity);
        }

        let libraries: HashMap<i64, (String, bool)> =
            sqlx::query!(r#"SELECT id, name, private AS "private: bool" FROM library"#)
                .fetch_all(&mut *conn)
                .await?
                .into_iter()
                .map(|row| (row.id, (row.name, row.private)))
                .collect();
        let mut hits = Vec::with_capacity(entities.len());
        for entity in entities {
            let library = match &entity {
                AnyEntity::Publication(publication) => publication.library(),
                AnyEntity::Work(work) => work.library(),
                AnyEntity::Person(person) => person.library(),
            };
            let Some((library_name, private)) = libraries.get(&library.get()) else {
                continue;
            };
            hits.push(SearchHit {
                library,
                library_name: library_name.clone(),
                private: *private,
                entity,
            });
        }
        Ok(hits)
    }
}

/// Check whether two names are the same after normalization
pub fn same_name(one: &str, other: &str) -> bool {
    normalize(one) == normalize(other)
}

/// Parse the whole query as an ISBN, or failing that an ISMN
fn parse_identifier(query: &str) -> Option<Identifier> {
    Identifier::new(IdentifierKind::Isbn, query)
        .or_else(|_| Identifier::new(IdentifierKind::Ismn, query))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EngineOptions;
    use crate::holding::NewHolding;
    use crate::publication::ImportSeed;

    #[tokio::test]
    async fn search_honours_visibility_drafts_and_limit() {
        let engine = Engine::demo(EngineOptions::default()).await.unwrap();

        assert!(
            engine
                .search("pro git", false, 10)
                .await
                .unwrap()
                .is_empty()
        );
        let hits = engine.search("pro git", true, 10).await.unwrap();
        let [hit] = hits.as_slice() else {
            panic!("expected one hit, got {hits:?}");
        };
        let AnyEntity::Publication(publication) = &hit.entity else {
            panic!("expected a publication, got {hit:?}");
        };
        assert_eq!(publication.title(), Some("Pro Git"));
        assert!(hit.private);
        assert_eq!(hit.library_name, "Books");

        // A draft in the public library is never a hit
        let sheet_music = engine
            .libraries()
            .await
            .unwrap()
            .into_iter()
            .find(|library| !library.is_private())
            .unwrap();
        sheet_music
            .create_publication(
                ImportSeed::Title("Pro Git handbook".into()),
                NewHolding::Physical { location: None },
            )
            .await
            .unwrap();
        assert_eq!(engine.search("pro git", true, 10).await.unwrap().len(), 1);

        assert_eq!(
            engine.search("rachmaninoff", true, 2).await.unwrap().len(),
            2
        );
        assert!(
            engine
                .search("rachmaninoff", true, 0)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn search_finds_a_publication_by_either_isbn_form() {
        let engine = Engine::demo(EngineOptions::default()).await.unwrap();
        // Imported as the ISBN-10 0-8065-0180-4 and stored as 978-0-8065-0180-2
        for query in ["0-8065-0180-4", "ISBN 0806501804", "9780806501802"] {
            let hits = engine.search(query, true, 10).await.unwrap();
            let [hit] = hits.as_slice() else {
                panic!("expected one hit for {query:?}, got {hits:?}");
            };
            let AnyEntity::Publication(publication) = &hit.entity else {
                panic!("expected a publication for {query:?}, got {hit:?}");
            };
            assert_eq!(
                publication.title(),
                Some("The Collected Writings of Ambrose Bierce")
            );
        }
    }

    #[test]
    fn same_name_ignores_case_diacritics_and_punctuation() {
        assert!(same_name("Etude-Tableau", "etude tableau"));
        assert!(same_name("Gymnopédie No. 1", "gymnopedie no 1"));
        // Word order still matters, unlike the fuzzy ranker
        assert!(!same_name("Rachmaninoff, Sergei", "sergei rachmaninoff"));
        assert!(!same_name("Op. 3 No. 2", "Op. 3 No. 4"));
    }
}
