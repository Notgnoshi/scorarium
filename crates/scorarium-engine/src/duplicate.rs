use std::collections::{BTreeSet, HashMap};
use std::hash::Hash;

use crate::entity::Entity;
use crate::library::LibraryId;
use crate::link::{ExternalRecord, Link};
use crate::person::{self, Person};
use crate::publication::{self, Identifier, IdentifierKind, Publication};
use crate::{Engine, Result, fuzzy};

/// Entities that may all be the same
#[derive(Clone, Debug)]
pub struct DuplicateGroup<E: Entity> {
    pub entities: Vec<E>,
    pub reasons: Vec<DuplicateReason>,
}

/// A kind of value that potential duplicates share
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DuplicateReason {
    Identifier,
    Title,
    Name,
    Record,
}

/// A match between two entities, as indices into the list being grouped
struct Match {
    one: usize,
    other: usize,
    reason: DuplicateReason,
}

/// Match the entities that have a key in common
fn match_by_key<E, K, I>(
    entities: &[E],
    reason: DuplicateReason,
    keys: impl Fn(&E) -> I,
) -> Vec<Match>
where
    K: Eq + Hash,
    I: IntoIterator<Item = K>,
{
    let mut last_holder = HashMap::new();
    let mut matches = Vec::new();
    for (index, entity) in entities.iter().enumerate() {
        for key in keys(entity) {
            if let Some(previous) = last_holder.insert(key, index)
                && previous != index
            {
                matches.push(Match {
                    one: previous,
                    other: index,
                    reason,
                });
            }
        }
    }
    matches
}

/// Collect the entities connected by matches into groups
fn group<E: Entity>(entities: Vec<E>, matches: &[Match]) -> Vec<DuplicateGroup<E>> {
    fn root(parents: &mut [usize], mut index: usize) -> usize {
        while parents[index] != index {
            parents[index] = parents[parents[index]];
            index = parents[index];
        }
        index
    }

    let mut parents: Vec<usize> = (0..entities.len()).collect();
    for found in matches {
        let one = root(&mut parents, found.one);
        let other = root(&mut parents, found.other);
        parents[one.max(other)] = one.min(other);
    }

    let mut reasons = vec![BTreeSet::new(); entities.len()];
    for found in matches {
        reasons[root(&mut parents, found.one)].insert(found.reason);
    }

    let mut members: Vec<Vec<E>> = entities.iter().map(|_| Vec::new()).collect();
    for (index, entity) in entities.into_iter().enumerate() {
        members[root(&mut parents, index)].push(entity);
    }

    members
        .into_iter()
        .zip(reasons)
        .filter(|(entities, _)| entities.len() > 1)
        .map(|(entities, reasons)| DuplicateGroup {
            entities,
            reasons: reasons.into_iter().collect(),
        })
        .collect()
}

fn name_key(person: &Person) -> Option<String> {
    Some(fuzzy::normalize(person.name())).filter(|key| !key.is_empty())
}

fn records(links: &[Link]) -> Vec<ExternalRecord> {
    links
        .iter()
        .filter_map(|link| link.record.clone())
        .collect()
}

fn title_key(title: &str) -> Option<String> {
    let normalized = fuzzy::normalize(title);
    let key = normalized.strip_prefix("the ").unwrap_or(&normalized);
    Some(key.to_string()).filter(|key| !key.is_empty())
}

fn standard_numbers(publication: &Publication) -> Vec<Identifier> {
    publication
        .identifiers()
        .iter()
        .filter(|identifier| {
            matches!(
                identifier.kind(),
                IdentifierKind::Isbn | IdentifierKind::Ismn
            )
        })
        .cloned()
        .collect()
}

/// List the groups of publications in `library` that may be duplicates
pub(crate) async fn publications(
    engine: &Engine,
    library: LibraryId,
) -> Result<Vec<DuplicateGroup<Publication>>> {
    let publications = publication::in_library(engine, library).await?;
    let mut matches = match_by_key(&publications, DuplicateReason::Identifier, standard_numbers);
    matches.extend(match_by_key(
        &publications,
        DuplicateReason::Title,
        |publication| publication.title().and_then(title_key),
    ));
    Ok(group(publications, &matches))
}

/// An entity that may be the same as another, with the values that the two share
#[derive(Clone, Debug)]
pub struct Duplicate<E: Entity> {
    pub entity: E,
    pub reasons: Vec<DuplicateReason>,
}

/// List the publications in its library that may duplicate `publication`, oldest first
pub(crate) async fn of_publication(
    publication: &Publication,
) -> Result<Vec<Duplicate<Publication>>> {
    let numbers = standard_numbers(publication);
    let title = publication.title().and_then(title_key);
    let others = publication::in_library(publication.engine(), publication.library()).await?;
    Ok(others
        .into_iter()
        .filter(|other| other.id() != publication.id())
        .filter_map(|other| {
            let mut reasons = Vec::new();
            if standard_numbers(&other)
                .iter()
                .any(|number| numbers.contains(number))
            {
                reasons.push(DuplicateReason::Identifier);
            }
            if title.is_some() && other.title().and_then(title_key) == title {
                reasons.push(DuplicateReason::Title);
            }
            (!reasons.is_empty()).then_some(Duplicate {
                entity: other,
                reasons,
            })
        })
        .collect())
}

/// List the groups of people in `library` that may be duplicates
pub(crate) async fn people(
    engine: &Engine,
    library: LibraryId,
) -> Result<Vec<DuplicateGroup<Person>>> {
    let people = person::contributing(engine, library).await?;
    let mut matches = match_by_key(&people, DuplicateReason::Name, name_key);
    matches.extend(match_by_key(&people, DuplicateReason::Record, |person| {
        records(person.links())
    }));
    Ok(group(people, &matches))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contributor::Role;
    use crate::entity::{Contributed, Source};
    use crate::holding::NewHolding;
    use crate::link::HttpUrl;
    use crate::publication::{Identifier, IdentifierKind, ImportSeed};

    #[tokio::test]
    async fn people_sharing_a_name_or_a_record_form_one_group() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), crate::test::options())
            .await
            .unwrap();
        let library = engine
            .create_library("Sheet music".parse().unwrap(), false)
            .await
            .unwrap();
        let mut preludes = library
            .import(
                ImportSeed::title("Preludes"),
                NewHolding::Physical { location: None },
            )
            .await
            .unwrap();
        let composer: Role = "composer".parse().unwrap();
        let record: HttpUrl = "https://openlibrary.org/authors/OL1234A".parse().unwrap();
        let mut rachmaninoff = preludes
            .create_contributor("Sergei Rachmaninoff".parse().unwrap(), composer.clone())
            .await
            .unwrap();
        rachmaninoff.add_link(record.clone()).await.unwrap();
        let mut rachmaninov = preludes
            .create_contributor("Sergei Rachmaninov".parse().unwrap(), composer.clone())
            .await
            .unwrap();
        rachmaninov.add_link(record).await.unwrap();
        let lowercase = preludes
            .create_contributor("sergei rachmaninov".parse().unwrap(), composer.clone())
            .await
            .unwrap();
        preludes
            .create_contributor("Erik Satie".parse().unwrap(), composer)
            .await
            .unwrap();

        let groups = library.duplicate_people().await.unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].entities, [rachmaninoff, rachmaninov, lowercase]);
        assert_eq!(
            groups[0].reasons,
            [DuplicateReason::Name, DuplicateReason::Record]
        );
    }

    #[tokio::test]
    async fn a_proposed_person_matches_nothing_until_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), crate::test::options())
            .await
            .unwrap();
        let library = engine
            .create_library("Sheet music".parse().unwrap(), false)
            .await
            .unwrap();
        let mut gymnopedies = library
            .import(
                ImportSeed::title("Gymnopedies"),
                NewHolding::Physical { location: None },
            )
            .await
            .unwrap();
        let composer: Role = "composer".parse().unwrap();
        let satie = gymnopedies
            .create_contributor("Erik Satie".parse().unwrap(), composer.clone())
            .await
            .unwrap();
        let proposed = gymnopedies
            .propose_new_contributor(Source::OpenLibrary, "Erik Satie", &composer)
            .await
            .unwrap();
        assert!(library.duplicate_people().await.unwrap().is_empty());

        let proposal = gymnopedies.proposals().await.unwrap()[0].id;
        gymnopedies.accept(proposal).await.unwrap();

        let groups = library.duplicate_people().await.unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].entities, [satie, proposed]);
        assert_eq!(groups[0].reasons, [DuplicateReason::Name]);
    }

    #[tokio::test]
    async fn publications_sharing_an_identifier_or_a_title_form_one_group() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), crate::test::options())
            .await
            .unwrap();
        let library = engine
            .create_library("Books".parse().unwrap(), false)
            .await
            .unwrap();
        let import = async |title: &str| {
            library
                .import(
                    ImportSeed::title(title),
                    NewHolding::Physical { location: None },
                )
                .await
                .unwrap()
        };
        let isbn = Identifier::new(IdentifierKind::Isbn, "978-0-486-25392-3").unwrap();
        let plate = Identifier::new(IdentifierKind::PlateNumber, "D 1234").unwrap();
        let mut gymnopedies = import("Gymnopedies").await;
        gymnopedies.add_identifier(isbn.clone()).await.unwrap();
        let mut the_hobbit = import("The Hobbit").await;
        the_hobbit.add_identifier(isbn).await.unwrap();
        let mut hobbit = import("hobbit").await;
        hobbit.add_identifier(plate.clone()).await.unwrap();
        let mut preludes = import("Preludes").await;
        preludes.add_identifier(plate).await.unwrap();

        let groups = library.duplicate_publications().await.unwrap();
        assert_eq!(groups.len(), 1);
        let members: Vec<_> = groups[0].entities.iter().map(Entity::id).collect();
        assert_eq!(members, [gymnopedies.id(), the_hobbit.id(), hobbit.id()]);
        assert_eq!(
            groups[0].reasons,
            [DuplicateReason::Identifier, DuplicateReason::Title]
        );

        let duplicates: Vec<_> = the_hobbit
            .duplicates()
            .await
            .unwrap()
            .into_iter()
            .map(|duplicate| (duplicate.entity.id(), duplicate.reasons))
            .collect();
        assert_eq!(
            duplicates,
            [
                (gymnopedies.id(), vec![DuplicateReason::Identifier]),
                (hobbit.id(), vec![DuplicateReason::Title]),
            ]
        );
        assert!(preludes.duplicates().await.unwrap().is_empty());
    }
}
