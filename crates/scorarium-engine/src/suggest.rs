use scorarium_archive::fuzzy::{normalize, rank};

use crate::library::{self, LibraryId};
use crate::person::{self, Person, PersonId};
use crate::publication::{self, Publication, PublicationId};
use crate::value::Tag;
use crate::work::{self, CatalogNumber, Work, WorkId};
use crate::{Engine, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SuggestField {
    Publication,
    Work,
    /// Catalog numbers, narrowed to the works `composer` composed when given
    WorkNumber {
        composer: Option<PersonId>,
    },
    Person,
    Role,
    Publisher,
    Tag,
    Key,
    TimeSignature,
    Instrumentation,
    Location,
}

#[derive(Clone, Debug)]
pub enum Suggested {
    Publication(Publication),
    Work {
        work: Work,
        number: Option<CatalogNumber>,
    },
    Person(Person),
    Role(String),
    Publisher(String),
    Tag {
        tag: Tag,
        count: usize,
    },
    Key(String),
    TimeSignature(String),
    Instrumentation(String),
    Location(String),
}

#[derive(Clone, Debug)]
pub struct Suggestion {
    /// The search query is equal to the suggestion after normalization
    pub exact: bool,
    pub item: Suggested,
}

const CONVENTIONAL_ROLES: [&str; 5] = ["arranger", "author", "composer", "editor", "translator"];

pub(crate) async fn suggest(
    engine: &Engine,
    library: LibraryId,
    field: SuggestField,
    query: &str,
    limit: usize,
) -> Result<Vec<Suggestion>> {
    if normalize(query).is_empty()
        && matches!(
            field,
            SuggestField::Publication
                | SuggestField::Work
                | SuggestField::WorkNumber { .. }
                | SuggestField::Person
        )
    {
        return Ok(Vec::new());
    }
    let mut conn = engine.begin_read().await?;
    // The entity arms rank and load in separate statements, so an entity deleted in between is
    // left out rather than failing the call.
    let mut suggestions = match field {
        SuggestField::Publication => {
            let candidates = publication::candidates(&mut conn, library).await?;
            let mut suggestions = Vec::new();
            for (id, exact) in ranked_candidates(query, candidates, limit) {
                let id = PublicationId::new(id);
                if let Some(publication) = Publication::fetch(engine, &mut conn, id).await? {
                    let item = Suggested::Publication(publication);
                    suggestions.push(Suggestion { exact, item });
                }
            }
            suggestions
        }
        SuggestField::Work => {
            let candidates = work::candidates(&mut conn, library).await?;
            let mut suggestions = Vec::new();
            for (id, exact) in ranked_candidates(query, candidates, limit) {
                if let Some(work) = Work::fetch(engine, &mut conn, WorkId::new(id)).await? {
                    let item = Suggested::Work { work, number: None };
                    suggestions.push(Suggestion { exact, item });
                }
            }
            suggestions
        }
        SuggestField::WorkNumber { composer } => {
            let wanted: CatalogNumber = query.parse()?;
            let candidates = work::number_candidates(&mut conn, library, composer).await?;
            let (mut same, rest): (Vec<_>, Vec<_>) = candidates
                .into_iter()
                .partition(|candidate| candidate.number == wanted);
            let (mut begun, other): (Vec<_>, Vec<_>) = rest
                .into_iter()
                .partition(|candidate| candidate.number.starts_with(&wanted));
            for tier in [&mut same, &mut begun] {
                tier.sort_by(|a, b| a.number.cmp(&b.number).then_with(|| a.title.cmp(&b.title)));
            }
            let texts: Vec<&str> = other
                .iter()
                .map(|candidate| candidate.text.as_str())
                .collect();
            let fuzzy: Vec<(WorkId, CatalogNumber, bool)> = rank(query, &texts)
                .into_iter()
                .map(|i| (other[i].work, other[i].number.clone(), false))
                .collect();
            let mut hits: Vec<(WorkId, CatalogNumber, bool)> = same
                .into_iter()
                .map(|candidate| (candidate.work, candidate.number, true))
                .chain(
                    begun
                        .into_iter()
                        .map(|candidate| (candidate.work, candidate.number, false)),
                )
                .chain(fuzzy)
                .collect();
            hits.truncate(limit);
            let mut suggestions = Vec::new();
            for (id, number, exact) in hits {
                if let Some(work) = Work::fetch(engine, &mut conn, id).await? {
                    let item = Suggested::Work {
                        work,
                        number: Some(number),
                    };
                    suggestions.push(Suggestion { exact, item });
                }
            }
            suggestions
        }
        SuggestField::Person => {
            let candidates = person::candidates(&mut conn, library).await?;
            let mut suggestions = Vec::new();
            for (id, exact) in ranked_candidates(query, candidates, limit) {
                if let Some(person) = Person::fetch(engine, &mut conn, PersonId::new(id)).await? {
                    let item = Suggested::Person(person);
                    suggestions.push(Suggestion { exact, item });
                }
            }
            suggestions
        }
        SuggestField::Role => {
            let stored = sqlx::query_scalar!(
                r#"SELECT DISTINCT role AS "role!" FROM (
                       SELECT role FROM publication_contributor WHERE library_id = ?1
                       UNION
                       SELECT role FROM work_contributor WHERE library_id = ?1
                   )"#,
                library.get()
            )
            .fetch_all(&mut *conn)
            .await?;
            let mut roles: Vec<String> = Vec::new();
            for role in stored {
                if !roles.iter().any(|kept| kept.eq_ignore_ascii_case(&role)) {
                    roles.push(role);
                }
            }
            for role in CONVENTIONAL_ROLES {
                if !roles.iter().any(|kept| kept.eq_ignore_ascii_case(role)) {
                    roles.push(role.to_string());
                }
            }
            roles.sort_by_key(|role| role.to_ascii_lowercase());
            ranked(query, roles, String::as_str, Suggested::Role)
        }
        SuggestField::Publisher => {
            let publishers = sqlx::query_scalar!(
                r#"SELECT DISTINCT publisher AS "value!" FROM publication
                   WHERE library_id = ? AND publisher IS NOT NULL ORDER BY publisher"#,
                library.get()
            )
            .fetch_all(&mut *conn)
            .await?;
            ranked(query, publishers, String::as_str, Suggested::Publisher)
        }
        SuggestField::Tag => {
            let counts = library::tag_counts(&mut conn, library, true).await?;
            ranked(
                query,
                counts,
                |(tag, _): &(Tag, usize)| tag.as_str(),
                |(tag, count)| Suggested::Tag { tag, count },
            )
        }
        SuggestField::Key => {
            let keys = sqlx::query_scalar!(
                r#"SELECT DISTINCT "key" AS "value!" FROM work
                   WHERE library_id = ? AND "key" IS NOT NULL ORDER BY "key""#,
                library.get()
            )
            .fetch_all(&mut *conn)
            .await?;
            ranked(query, keys, String::as_str, Suggested::Key)
        }
        SuggestField::TimeSignature => {
            let time_signatures = sqlx::query_scalar!(
                r#"SELECT DISTINCT time_signature AS "value!" FROM work
                   WHERE library_id = ? AND time_signature IS NOT NULL ORDER BY time_signature"#,
                library.get()
            )
            .fetch_all(&mut *conn)
            .await?;
            ranked(
                query,
                time_signatures,
                String::as_str,
                Suggested::TimeSignature,
            )
        }
        SuggestField::Instrumentation => {
            let instrumentations = sqlx::query_scalar!(
                r#"SELECT DISTINCT instrumentation AS "value!" FROM work
                   WHERE library_id = ? AND instrumentation IS NOT NULL ORDER BY instrumentation"#,
                library.get()
            )
            .fetch_all(&mut *conn)
            .await?;
            ranked(
                query,
                instrumentations,
                String::as_str,
                Suggested::Instrumentation,
            )
        }
        SuggestField::Location => {
            let locations = sqlx::query_scalar!(
                r#"SELECT DISTINCT holding.location AS "value!" FROM holding
                   JOIN publication ON publication.id = holding.publication_id
                   WHERE publication.library_id = ? AND holding.kind = 'physical'
                     AND holding.location IS NOT NULL
                   ORDER BY holding.location"#,
                library.get()
            )
            .fetch_all(&mut *conn)
            .await?;
            ranked(query, locations, String::as_str, Suggested::Location)
        }
    };
    suggestions.truncate(limit);
    Ok(suggestions)
}

/// Rank candidate values against the search query
fn ranked<T>(
    query: &str,
    items: Vec<T>,
    value: fn(&T) -> &str,
    wrap: fn(T) -> Suggested,
) -> Vec<Suggestion> {
    let order: Vec<usize> = if normalize(query).is_empty() {
        (0..items.len()).collect()
    } else {
        let values: Vec<&str> = items.iter().map(value).collect();
        rank(query, &values)
    };
    let mut items: Vec<Option<T>> = items.into_iter().map(Some).collect();
    let mut suggestions: Vec<Suggestion> = order
        .into_iter()
        .map(|i| {
            let item = items[i].take().expect("each index is ranked once");
            Suggestion {
                exact: normalize(value(&item)) == normalize(query),
                item: wrap(item),
            }
        })
        .collect();
    suggestions.sort_by_key(|suggestion| !suggestion.exact);
    suggestions
}

/// The searchable text from an entity
pub(crate) struct Candidate {
    pub id: i64,
    /// The title or name, which decides an exact match; empty for an untitled draft
    pub name: String,
    /// The name followed by every other word the entity is found by
    pub text: String,
}

fn ranked_candidates(query: &str, candidates: Vec<Candidate>, limit: usize) -> Vec<(i64, bool)> {
    let query_normalized = normalize(query);
    let texts: Vec<&str> = candidates
        .iter()
        .map(|candidate| candidate.text.as_str())
        .collect();
    let mut hits: Vec<(i64, bool)> = rank(query, &texts)
        .into_iter()
        .map(|i| {
            let candidate = &candidates[i];
            (candidate.id, normalize(&candidate.name) == query_normalized)
        })
        .collect();
    hits.sort_by_key(|&(_, exact)| !exact);
    hits.truncate(limit);
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EngineOptions;
    use crate::entity::Entity;
    use crate::holding::NewHolding;
    use crate::publication::ImportSeed;

    fn values(suggestions: &[Suggestion]) -> Vec<(&str, bool)> {
        suggestions
            .iter()
            .map(|suggestion| {
                let value = match &suggestion.item {
                    Suggested::Role(value) | Suggested::Publisher(value) => value.as_str(),
                    other => panic!("unexpected suggestion {other:?}"),
                };
                (value, suggestion.exact)
            })
            .collect()
    }

    fn numbers(suggestions: &[Suggestion]) -> Vec<(&str, bool)> {
        suggestions
            .iter()
            .map(|suggestion| match &suggestion.item {
                Suggested::Work {
                    number: Some(number),
                    ..
                } => (number.as_str(), suggestion.exact),
                other => panic!("unexpected suggestion {other:?}"),
            })
            .collect()
    }

    #[tokio::test]
    async fn vocabulary_lists_alphabetically_and_exact_first() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), EngineOptions::default())
            .await
            .unwrap();
        let library = engine.create_library("Sheet music", false).await.unwrap();
        for (title, publisher) in [
            ("Russian piano album", "Schirmer"),
            ("Rachmaninoff masterpieces", "Dover"),
            ("Pro Git", "Apress"),
        ] {
            let mut publication = library
                .create_publication(
                    ImportSeed::Title(title.into()),
                    NewHolding::Physical { location: None },
                )
                .await
                .unwrap();
            publication.set_publisher(Some(publisher)).await.unwrap();
        }
        let mut album = library.publications().await.unwrap().remove(0);
        album
            .create_contributor("Sergei Rachmaninoff", "Composer")
            .await
            .unwrap();

        let publishers = library
            .suggest(SuggestField::Publisher, "", 10)
            .await
            .unwrap();
        assert_eq!(
            values(&publishers),
            [("Apress", false), ("Dover", false), ("Schirmer", false)]
        );
        let publishers = library
            .suggest(SuggestField::Publisher, "dover", 10)
            .await
            .unwrap();
        assert_eq!(values(&publishers)[0], ("Dover", true));
        let publishers = library
            .suggest(SuggestField::Publisher, "", 2)
            .await
            .unwrap();
        assert_eq!(publishers.len(), 2);

        // The stored spelling replaces the seeded "composer" and sorts among the seeds
        let roles = library.suggest(SuggestField::Role, "", 10).await.unwrap();
        assert_eq!(
            values(&roles),
            [
                ("arranger", false),
                ("author", false),
                ("Composer", false),
                ("editor", false),
                ("translator", false),
            ]
        );
    }

    #[tokio::test]
    async fn entity_suggestions_put_exact_first_and_include_drafts() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), EngineOptions::default())
            .await
            .unwrap();
        let library = engine.create_library("Sheet music", false).await.unwrap();
        let mut nocturnes = library
            .create_publication(
                ImportSeed::Title("Chopin Nocturnes".into()),
                NewHolding::Physical { location: None },
            )
            .await
            .unwrap();
        let chopin = nocturnes
            .create_contributor("Frederic Chopin", "composer")
            .await
            .unwrap();
        let mut nocturne = nocturnes.create_work("Nocturne").await.unwrap();
        nocturne
            .add_contributor(chopin.id(), "composer")
            .await
            .unwrap();
        nocturne
            .add_catalog_number("Op. 9 No. 2".parse().unwrap())
            .await
            .unwrap();
        nocturnes.commit().await.unwrap();
        library
            .create_publication(
                ImportSeed::Title("Nocturne album".into()),
                NewHolding::Physical { location: None },
            )
            .await
            .unwrap();

        for query in ["nocturne", "op9no2"] {
            let works = library
                .suggest(SuggestField::Work, query, 10)
                .await
                .unwrap();
            let Some(Suggestion {
                exact,
                item: Suggested::Work { work, number },
            }) = works.first()
            else {
                panic!("no work suggested for {query:?}");
            };
            assert_eq!(work.title(), "Nocturne");
            assert_eq!(*exact, query == "nocturne");
            assert!(number.is_none());
        }

        let publications = library
            .suggest(SuggestField::Publication, "nocturne", 10)
            .await
            .unwrap();
        // Neither title is exact, so the order is by score; both must be offered
        let mut drafts: Vec<bool> = publications
            .iter()
            .map(|suggestion| match &suggestion.item {
                Suggested::Publication(publication) => publication.is_draft(),
                other => panic!("unexpected suggestion {other:?}"),
            })
            .collect();
        drafts.sort();
        assert_eq!(drafts, [false, true]);

        let people = library
            .suggest(SuggestField::Person, "Frederic Chopin", 10)
            .await
            .unwrap();
        let Some(Suggestion {
            exact: true,
            item: Suggested::Person(person),
        }) = people.first()
        else {
            panic!("Chopin not suggested exactly: {people:?}");
        };
        assert_eq!(person.id(), chopin.id());
    }

    #[tokio::test]
    async fn work_numbers_tier_and_narrow_by_composer() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), EngineOptions::default())
            .await
            .unwrap();
        let library = engine.create_library("Sheet music", false).await.unwrap();
        let mut album = library
            .create_publication(
                ImportSeed::Title("Romantic piano".into()),
                NewHolding::Physical { location: None },
            )
            .await
            .unwrap();
        let chopin = album
            .create_contributor("Frederic Chopin", "composer")
            .await
            .unwrap();
        let beethoven = album
            .create_contributor("Ludwig van Beethoven", "Composer")
            .await
            .unwrap();
        for (title, number, composer) in [
            ("Nocturne in E-flat major", "Op. 9 No. 2", chopin.id()),
            ("Nocturne in B-flat minor", "Op. 9 No. 1", chopin.id()),
            ("Ballade", "Op. 23", chopin.id()),
            ("Septet", "Op. 20", beethoven.id()),
        ] {
            let mut work = album.create_work(title).await.unwrap();
            work.add_contributor(composer, "Composer").await.unwrap();
            work.add_catalog_number(number.parse().unwrap())
                .await
                .unwrap();
            if title == "Ballade" {
                work.add_contributor(beethoven.id(), "arranger")
                    .await
                    .unwrap();
            }
        }

        let everyone = SuggestField::WorkNumber { composer: None };

        // Prefix tier only: the typed 9 allows no typos, so Op. 23 and Op. 20 are not reached
        let begun = library.suggest(everyone, "Op. 9", 10).await.unwrap();
        assert_eq!(
            numbers(&begun),
            [("Op. 9 No. 1", false), ("Op. 9 No. 2", false)]
        );

        let same = library.suggest(everyone, "Op. 9/2", 10).await.unwrap();
        assert_eq!(numbers(&same)[0], ("Op. 9 No. 2", true));

        // The "Composer" spelling counts; the arranger role on the Ballade does not
        let only_beethoven = SuggestField::WorkNumber {
            composer: Some(beethoven.id()),
        };
        let narrowed = library.suggest(only_beethoven, "op", 10).await.unwrap();
        assert_eq!(numbers(&narrowed), [("Op. 20", false)]);
    }
}
