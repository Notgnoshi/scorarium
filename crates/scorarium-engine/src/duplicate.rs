use std::collections::{BTreeSet, HashMap};
use std::hash::Hash;

use crate::entity::{Contributed, Entity};
use crate::library::LibraryId;
use crate::link::{ExternalRecord, Link};
use crate::person::{self, Person, PersonId};
use crate::publication::{self, Identifier, IdentifierKind, Publication};
use crate::work::{self, Work};
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
    CatalogNumber,
}

/// A match between two entities, as indices into the list being grouped
struct Match {
    one: usize,
    other: usize,
    reason: DuplicateReason,
}

/// Collect the indices of the entities that hold each key
fn holders<E, K, I>(entities: &[E], keys: impl Fn(&E) -> I) -> HashMap<K, Vec<usize>>
where
    K: Eq + Hash,
    I: IntoIterator<Item = K>,
{
    let mut holders: HashMap<K, Vec<usize>> = HashMap::new();
    for (index, entity) in entities.iter().enumerate() {
        for key in keys(entity) {
            let indices = holders.entry(key).or_default();
            if indices.last() != Some(&index) {
                indices.push(index);
            }
        }
    }
    holders
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
    let mut matches = Vec::new();
    for indices in holders(entities, keys).into_values() {
        for pair in indices.windows(2) {
            matches.push(Match {
                one: pair[0],
                other: pair[1],
                reason,
            });
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

/// A value that one person has, and that two matching people share
#[derive(Clone, PartialEq, Eq, Hash)]
enum PersonKey {
    Id(PersonId),
    Name(String),
    Record(ExternalRecord),
}

/// List the keys of the work's contributors
fn contributor_keys(work: &Work, people: &HashMap<PersonId, Person>) -> Vec<PersonKey> {
    let mut keys = Vec::new();
    for contributor in work.contributors() {
        keys.push(PersonKey::Id(contributor.person));
        if let Some(person) = people.get(&contributor.person) {
            keys.extend(name_key(person).map(PersonKey::Name));
            keys.extend(records(person.links()).into_iter().map(PersonKey::Record));
        }
    }
    keys
}

/// Check whether catalog numbers tell two works apart
///
/// They do when the works have numbers in a common scheme and share no number.
fn numbers_differ(one: &Work, other: &Work) -> bool {
    let (ones, others) = (one.catalog_numbers(), other.catalog_numbers());
    ones.iter().any(|a| others.iter().any(|b| a.same_scheme(b)))
        && !ones.iter().any(|a| others.contains(a))
}

/// List the groups of works in `library` that may be duplicates
pub(crate) async fn works(
    engine: &Engine,
    library: LibraryId,
) -> Result<Vec<DuplicateGroup<Work>>> {
    let works = work::contained(engine, library).await?;
    let people: HashMap<PersonId, Person> = person::contributing(engine, library)
        .await?
        .into_iter()
        .map(|person| (person.id(), person))
        .collect();

    let mut matches = match_by_key(&works, DuplicateReason::Record, |work| {
        records(work.links())
    });
    matches.extend(match_by_key(
        &works,
        DuplicateReason::CatalogNumber,
        |work| {
            let mut keys = Vec::new();
            for key in contributor_keys(work, &people) {
                for number in work.catalog_numbers() {
                    keys.push((key.clone(), number.clone()));
                }
            }
            keys
        },
    ));
    let titles = holders(&works, |work| {
        let Some(title) = title_key(work.title()) else {
            return Vec::new();
        };
        contributor_keys(work, &people)
            .into_iter()
            .map(|key| (key, title.clone()))
            .collect()
    });
    for indices in titles.into_values() {
        for (position, &one) in indices.iter().enumerate() {
            for &other in &indices[position + 1..] {
                if !numbers_differ(&works[one], &works[other]) {
                    matches.push(Match {
                        one,
                        other,
                        reason: DuplicateReason::Title,
                    });
                }
            }
        }
    }
    Ok(group(works, &matches))
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

    #[tokio::test]
    async fn works_match_by_a_shared_contributor_with_a_number_or_a_title() {
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
        let scriabin = preludes
            .create_contributor("Alexander Scriabin".parse().unwrap(), composer.clone())
            .await
            .unwrap();

        let mut works = Vec::new();
        for (title, person, number) in [
            (
                "Prelude in C-sharp minor",
                &rachmaninoff,
                Some("Op. 3 No. 2"),
            ),
            ("Prelude", &rachmaninov, Some("Op. 3/2")),
            ("The Prelude", &rachmaninoff, None),
            ("Etude-Tableau", &rachmaninoff, Some("Op. 33 No. 1")),
            ("Etude-Tableau", &rachmaninoff, Some("Op. 33 No. 2")),
            ("Prelude", &scriabin, Some("Op. 3 No. 2")),
        ] {
            let mut work = preludes.create_work(title.parse().unwrap()).await.unwrap();
            work.add_contributor(person.id(), composer.clone())
                .await
                .unwrap();
            if let Some(number) = number {
                work.add_catalog_number(number.parse().unwrap())
                    .await
                    .unwrap();
            }
            works.push(work);
        }
        let mut proposed = preludes
            .propose_new_work(Source::OpenLibrary, "Prelude")
            .await
            .unwrap();
        proposed
            .add_contributor(rachmaninoff.id(), composer)
            .await
            .unwrap();

        let groups = library.duplicate_works().await.unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].entities, works[..3]);
        assert_eq!(
            groups[0].reasons,
            [DuplicateReason::Title, DuplicateReason::CatalogNumber]
        );
    }
}
