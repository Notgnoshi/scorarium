use scorarium_archive::fuzzy::normalize;
use scorarium_client::open_library::Edition;
use scorarium_client::{Priority, Source};

use crate::enrich::SubTaskQueue;
use crate::entity::{Entity, EntityKind};
use crate::link::{ExternalRecord, LinkKind};
use crate::person::{self, Person};
use crate::publication::{Identifier, IdentifierKind, Proposal, Publication, PublicationId};
use crate::value::{Classification, Year};
use crate::{Engine, Error, Result};

/// Start Open Library enrichment for the given [IdentifierKind::Isbn] [Identifier]
pub(crate) async fn edition(
    engine: Engine,
    publication: PublicationId,
    isbn: Identifier,
    queue: SubTaskQueue,
) -> Result<()> {
    if queue.is_cancelled() {
        return Ok(());
    }
    log::debug!("enriching publication {publication} from Open Library by ISBN {isbn}");
    let Some(edition) = engine
        .client()
        .open_library()
        .edition_by_isbn(isbn.as_str(), Priority::Background)
        .await?
    else {
        // A 404 is a definite answer, so there is nothing to propose.
        return Ok(());
    };
    let Some(publication) = Publication::load(&engine, publication).await? else {
        queue.cancel();
        return Ok(());
    };

    let title = match &edition.subtitle {
        Some(subtitle) => format!("{}: {subtitle}", edition.title),
        None => edition.title.clone(),
    };
    let mut proposals = vec![Proposal::Title(title)];
    proposals.extend(
        edition
            .publishers
            .iter()
            .map(|p| Proposal::Publisher(p.clone())),
    );
    if let Some(year) = edition.publish_date.as_deref().and_then(year) {
        proposals.push(Proposal::Year(year));
    }
    for raw in edition.isbn_13.iter().chain(&edition.isbn_10) {
        match Identifier::new(IdentifierKind::Isbn, raw) {
            Ok(identifier) => proposals.push(Proposal::Identifier(identifier)),
            Err(error) => log::warn!(
                "Open Library edition {}: ISBN {raw:?}: {error}",
                edition.olid
            ),
        }
    }
    proposals.push(Proposal::Link(edition.url().parse()?));

    let (mut accepted, mut present) = (0, 0);
    for proposal in proposals {
        if queue.is_cancelled() {
            return Ok(());
        }
        match publication.propose(Source::OpenLibrary, proposal).await {
            Ok(Some(_)) => accepted += 1,
            Ok(None) => present += 1,
            // A retry cannot fix a value the engine rejects as invalid
            Err(Error::InvalidValue(reason)) => {
                log::warn!("Open Library edition {} invalid: {reason}", edition.olid);
            }
            Err(error) => return Err(error),
        }
    }
    log::debug!(
        "Open Library edition {}: proposed {accepted}, already present {present}",
        edition.olid
    );
    if let Some(classification) = classify(&edition, publication.identifiers()) {
        if queue.is_cancelled() {
            return Ok(());
        }
        if publication.classify_if_unset(classification).await? {
            log::debug!(
                "Open Library edition {}: classified as {classification}",
                edition.olid
            );
        }
    }
    for olid in edition.authors {
        queue.enqueue(author(
            engine.clone(),
            publication.id(),
            olid,
            queue.clone(),
        ));
    }
    Ok(())
}

/// Publishers that print only sheet music, as normalized phrases
///
/// Dover is deliberately absent because it publishes both music and prose.
const MUSIC_PUBLISHERS: [&str; 12] = [
    "schirmer",
    "alfred music",
    "alfred publishing",
    "kalmus",
    "boosey hawkes",
    "faber music",
    "schott",
    "henle",
    "barenreiter",
    "peters",
    "breitkopf",
    "hal leonard",
];

/// Musical forms and instruments that mark a title as sheet music, as normalized words
///
/// Some prose titles contain these, but scores use them far more often
const MUSIC_TERMS: [&str; 41] = [
    "sonata",
    "sonatas",
    "etude",
    "etudes",
    "nocturne",
    "nocturnes",
    "prelude",
    "preludes",
    "fugue",
    "fugues",
    "concerto",
    "concertos",
    "symphony",
    "symphonies",
    "quartet",
    "quartets",
    "bagatelle",
    "bagatelles",
    "rondo",
    "rondos",
    "waltz",
    "waltzes",
    "mazurka",
    "mazurkas",
    "polonaise",
    "polonaises",
    "scherzo",
    "scherzi",
    "cantata",
    "cantatas",
    "piano",
    "violin",
    "viola",
    "cello",
    "guitar",
    "flute",
    "clarinet",
    "oboe",
    "organ",
    "harp",
    "clavier",
];

/// Catalog abbreviations that mark a title as sheet music when a number follows, as normalized words
const CATALOG_PREFIXES: [&str; 7] = ["op", "opus", "bwv", "k", "kv", "hob", "d"];

/// Guess whether the edition is sheet music
fn classify(edition: &Edition, identifiers: &[Identifier]) -> Option<Classification> {
    let ismn = identifiers
        .iter()
        .any(|identifier| identifier.kind() == IdentifierKind::Ismn);
    let publisher = edition.publishers.iter().any(|publisher| {
        let publisher = normalize(publisher);
        MUSIC_PUBLISHERS
            .iter()
            .any(|house| contains_phrase(&publisher, house))
    });
    let title = normalize(&format!(
        "{} {}",
        edition.title,
        edition.subtitle.as_deref().unwrap_or_default()
    ));
    let words: Vec<&str> = title.split(' ').collect();
    let term = words.iter().any(|word| MUSIC_TERMS.contains(word));
    // Normalizing "Op. 27" and "K.331" yields the prefix and the number as separate words.
    let catalog = words.windows(2).any(|pair| {
        CATALOG_PREFIXES.contains(&pair[0]) && pair[1].starts_with(|c: char| c.is_ascii_digit())
    });
    (ismn || publisher || term || catalog).then_some(Classification::Music)
}

/// Whether the words of `needle` occur consecutively in `haystack`, both already normalized
fn contains_phrase(haystack: &str, needle: &str) -> bool {
    let needle: Vec<&str> = needle.split(' ').collect();
    haystack
        .split(' ')
        .collect::<Vec<_>>()
        .windows(needle.len())
        .any(|run| run == needle)
}

/// Map Open Library's API ID keys to scorarium's [LinkKind] enum
const AUTHOR_IDS: [(&str, LinkKind); 10] = [
    ("wikidata", LinkKind::Wikidata),
    ("viaf", LinkKind::Viaf),
    ("isni", LinkKind::Isni),
    ("gnd", LinkKind::Gnd),
    ("lc_naf", LinkKind::Loc),
    ("musicbrainz", LinkKind::MusicBrainz),
    ("goodreads", LinkKind::Goodreads),
    ("librarything", LinkKind::LibraryThing),
    ("project_gutenberg", LinkKind::Gutenberg),
    ("librivox", LinkKind::Librivox),
];

/// Look up Open Library's author record for `olid` for enrichment proposals
pub(crate) async fn author(
    engine: Engine,
    publication: PublicationId,
    olid: String,
    queue: SubTaskQueue,
) -> Result<()> {
    if queue.is_cancelled() {
        return Ok(());
    }
    log::debug!("enriching publication {publication} from Open Library author {olid}");
    let Some(record) = engine
        .client()
        .open_library()
        .author(&olid, Priority::Background)
        .await?
    else {
        return Ok(());
    };
    let Some(publication) = Publication::load(&engine, publication).await? else {
        queue.cancel();
        return Ok(());
    };

    let this = ExternalRecord::new(LinkKind::OpenLibrary, &olid);
    let mut matches = person::find_by_record(&engine, publication.library(), &this).await?;
    if matches.is_empty() {
        matches = person::find_by_name(&engine, publication.library(), &record.name).await?;
        // A namesake linked to a different Open Library author is a different person.
        matches.retain(|person| {
            !person.links().iter().any(|link| {
                link.record
                    .as_ref()
                    .is_some_and(|other| other.kind() == LinkKind::OpenLibrary && *other != this)
            })
        });
    }

    if queue.is_cancelled() {
        return Ok(());
    }
    let person = match best_match(matches) {
        Some(person) => {
            let proposal = Proposal::Contributor {
                person: person.clone(),
                role: "author".into(),
            };
            publication.propose(Source::OpenLibrary, proposal).await?;
            person
        }
        None => match publication
            .propose_new_contributor(Source::OpenLibrary, &record.name, "author")
            .await
        {
            Ok(person) => person,
            Err(Error::InvalidValue(reason)) => {
                log::warn!("Open Library author {olid}: {reason}");
                return Ok(());
            }
            Err(error) => return Err(error),
        },
    };

    // Links are proposals too; the person may be committed, and the user decides.
    let mut links = vec![this];
    for (key, kind) in AUTHOR_IDS {
        if let Some(id) = record.remote_ids.get(key) {
            links.push(ExternalRecord::new(kind, id));
        }
    }
    for link in links {
        if queue.is_cancelled() {
            return Ok(());
        }
        let Some(url) = link.url(EntityKind::Person) else {
            continue;
        };
        match person
            .propose(Source::OpenLibrary, person::Proposal::Link(url))
            .await
        {
            Ok(_) => {}
            Err(Error::InvalidValue(reason)) => log::warn!("Open Library author {olid}: {reason}"),
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Choose a committed person over a draft, and among equals the lowest id
fn best_match(matches: Vec<Person>) -> Option<Person> {
    matches
        .into_iter()
        .min_by_key(|person| (person.is_draft(), person.id().get()))
}

/// The first run of exactly four ASCII digits in a free-text date
fn year(publish_date: &str) -> Option<Year> {
    publish_date
        .split(|c: char| !c.is_ascii_digit())
        .find(|run| run.len() == 4)
        .and_then(|run| run.parse().ok())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::enrich::EnrichmentStatus;
    use crate::entity;
    use crate::holding::NewHolding;
    use crate::publication::ImportSeed;

    fn edition_titled(title: &str, publisher: &str) -> Edition {
        Edition {
            olid: "OL1M".into(),
            title: title.into(),
            subtitle: None,
            authors: vec![],
            contributions: vec![],
            by_statement: None,
            publishers: vec![publisher.into()],
            publish_date: None,
            isbn_10: vec![],
            isbn_13: vec![],
            number_of_pages: None,
            covers: vec![],
            works: vec![],
        }
    }

    #[test]
    fn a_music_signal_classifies_and_silence_does_not() {
        let cases = [
            ("Werke", "G. Henle Verlag", Some(Classification::Music)),
            (
                "Fifty-one exercises",
                "G. Schirmer, Inc.",
                Some(Classification::Music),
            ),
            ("Sonata for Two", "Somebody", Some(Classification::Music)),
            ("Pro Git", "Apress", None),
            ("Collected Essays", "Dover Publications", None),
        ];
        for (title, publisher, expected) in cases {
            assert_eq!(
                classify(&edition_titled(title, publisher), &[]),
                expected,
                "{title} / {publisher}"
            );
        }
    }

    /// Render a proposal as "<field> <value>", since Proposal cannot derive PartialEq
    fn describe(proposal: &entity::Proposal<Publication>) -> String {
        match &proposal.value {
            Proposal::Title(title) => format!("title {title}"),
            Proposal::Publisher(publisher) => format!("publisher {publisher}"),
            Proposal::Year(year) => format!("year {year}"),
            Proposal::Identifier(identifier) => format!("identifier {identifier}"),
            Proposal::Link(url) => format!("link {url}"),
            Proposal::Contributor { person, role } => {
                format!("contributor {} {role}", person.name())
            }
            Proposal::Contents(work) => format!("contents {}", work.title()),
        }
    }

    /// Render a person link proposal as its URL
    fn link_of(proposal: &entity::Proposal<Person>) -> String {
        match &proposal.value {
            person::Proposal::Link(url) => url.to_string(),
            person::Proposal::Name(_) => panic!("expected a link proposal"),
        }
    }

    #[tokio::test]
    async fn an_isbn_import_proposes_the_edition_facts() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), crate::test::options())
            .await
            .unwrap();
        let library = engine.create_library("Sheet music", false).await.unwrap();
        let isbn = Identifier::new(IdentifierKind::Isbn, "978-0-486-25392-3").unwrap();
        let publication = library
            .create_publication(
                ImportSeed::Identifier {
                    identifier: isbn,
                    record: None,
                },
                NewHolding::Physical { location: None },
            )
            .await
            .unwrap();

        tokio::time::timeout(Duration::from_secs(5), engine.wait_for_enrichment())
            .await
            .unwrap();
        let status = publication.enrichment().unwrap().status();
        assert_eq!(
            status,
            EnrichmentStatus {
                successful: 2,
                pending: 0,
                failed: 0,
                cancelled: false
            }
        );

        let publication = Publication::load(&engine, publication.id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(publication.classification(), Some(Classification::Music));

        let proposals: Vec<String> = publication
            .proposals()
            .await
            .unwrap()
            .iter()
            .map(describe)
            .collect();
        assert_eq!(
            proposals,
            [
                "title Bagatelles, Rondos and Other Shorter Works for Piano",
                "publisher Dover Publications",
                "year 1987",
                "link https://openlibrary.org/books/OL7636066M",
                "contributor Ludwig van Beethoven author",
            ]
        );

        let proposals = publication.proposals().await.unwrap();
        let Proposal::Contributor { person, .. } = &proposals[4].value else {
            panic!("expected a contributor proposal");
        };
        assert!(person.is_draft());
        let links: Vec<String> = person
            .proposals()
            .await
            .unwrap()
            .iter()
            .map(link_of)
            .collect();
        assert_eq!(
            links,
            [
                "https://openlibrary.org/authors/OL127077A",
                "https://www.wikidata.org/wiki/Q255",
                "https://viaf.org/viaf/32182557",
                "https://isni.org/isni/0000000121268987",
                "https://id.loc.gov/authorities/names/n79107741",
                "https://musicbrainz.org/artist/1f9df192-a621-4f54-8850-2c5373b7eac9",
                "https://www.librarything.com/author/beethovenludwigvan",
                "https://www.gutenberg.org/ebooks/author/75",
                "https://librivox.org/author/4437",
            ]
        );
    }

    #[tokio::test]
    async fn an_author_already_in_the_library_is_proposed_not_duplicated() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), crate::test::options())
            .await
            .unwrap();
        let library = engine.create_library("Sheet music", false).await.unwrap();
        let mut sonatas = library
            .create_publication(
                ImportSeed::Title {
                    title: "Piano sonatas".into(),
                    record: None,
                },
                NewHolding::Physical { location: None },
            )
            .await
            .unwrap();
        let beethoven = sonatas
            .create_contributor("Ludwig van Beethoven", "composer")
            .await
            .unwrap();
        sonatas.commit().await.unwrap();

        let isbn = Identifier::new(IdentifierKind::Isbn, "978-0-486-25392-3").unwrap();
        let publication = library
            .create_publication(
                ImportSeed::Identifier {
                    identifier: isbn,
                    record: None,
                },
                NewHolding::Physical { location: None },
            )
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), engine.wait_for_enrichment())
            .await
            .unwrap();

        let proposals = publication.proposals().await.unwrap();
        let Proposal::Contributor { person, role } = &proposals[4].value else {
            panic!("expected a contributor proposal");
        };
        assert_eq!(person.id(), beethoven.id());
        assert_eq!(role, "author");
        assert_eq!(library.people(None).await.unwrap().len(), 1);
    }
}
