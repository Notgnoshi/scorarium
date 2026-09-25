use crate::catalog::CatalogNumber;
use crate::holding::{HoldingInput, HoldingKind};
use crate::identifier::{self, Kind, Normalized};
use crate::input::{ContributorInput, PersonRef};
use crate::library::Library;
use crate::person::PersonRawInput;
use crate::publication::{Publication, PublicationInput};
use crate::work::{self, WorkInput};
use crate::{Action, Archive, Event, Field, Result, Source};

const RACHMANINOFF: &str = "Sergei Rachmaninoff";
const SATIE: &str = "Erik Satie";
const BIERCE: &str = "Ambrose Bierce";

pub(crate) async fn populate(archive: &Archive) -> Result<()> {
    // One of each, so the demo shows what a logged-out visitor does and does not see
    let books = archive.create_library("Books", true).await?;
    let sheet_music = archive.create_library("Sheet music", false).await?;

    create(
        &books,
        PublicationInput {
            title: "Practical Vim".into(),
            publisher: Some("Pragmatic Bookshelf".into()),
            year: Some(2015),
            stars: Some(5),
            note: Some(
                "The chapter on the dot command is the one to reread.\n\nLent to Sam once".into(),
            ),
            tags: vec!["editor".into(), "reference".into()],
            holdings: vec![physical(Some("Desk"))],
            identifiers: vec![normalized(Kind::Isbn, "978-1-68050-127-8")?],
            contributors: vec![contributor("Drew Neil", "author")],
            links: links(&[
                "https://openlibrary.org/books/OL27196589M/Practical_Vim",
                "https://www.goodreads.com/book/show/42854052-practical-vim",
            ]),
            contents: Vec::new(),
        },
    )
    .await?;

    create(
        &books,
        PublicationInput {
            title: "Pro Git".into(),
            publisher: Some("Apress".into()),
            year: Some(2014),
            stars: Some(3),
            note: None,
            tags: Vec::new(),
            holdings: vec![physical(None), digital("pro-git.pdf")],
            identifiers: vec![normalized(Kind::Isbn, "978-1-4842-0077-3")?],
            contributors: vec![
                contributor("Scott Chacon", "author"),
                contributor("Ben Straub", "author"),
            ],
            // A site the table does not know, so the page falls back on its favicon
            links: links(&[
                "https://openlibrary.org/books/OL26372169M/Pro_Git",
                "https://git-scm.com/book/en/v2",
            ]),
            contents: Vec::new(),
        },
    )
    .await?;

    // A book with works, so pages show works without any music-specific fields
    let mut dictionary = writing("The Devil's Dictionary");
    dictionary.links = links(&["https://www.gutenberg.org/ebooks/972"]);
    create(
        &books,
        PublicationInput {
            title: "The Collected Writings of Ambrose Bierce".into(),
            publisher: Some("Citadel Press".into()),
            year: Some(1979),
            stars: None,
            note: None,
            tags: Vec::new(),
            holdings: vec![physical(None)],
            identifiers: vec![normalized(Kind::Isbn, "0-8065-0180-4")?],
            contributors: vec![contributor(BIERCE, "author")],
            links: Vec::new(),
            contents: vec![
                writing("In the Midst of Life"),
                // Project Gutenberg is named by the site table but drawn by its own favicon
                dictionary,
                writing("The Parenticide Club"),
            ],
        },
    )
    .await?;

    // An anthology: every composer is credited on the publication, but only Rachmaninoff's pieces
    // are entered as works.
    let mut composers: Vec<ContributorInput> = [
        "Dmitri Kabalevsky",
        "Modest Mussorgsky",
        "Sergei Prokofiev",
        "Dmitri Shostakovich",
        "Pyotr Ilyich Tchaikovsky",
    ]
    .into_iter()
    .map(|name| contributor(name, "composer"))
    .collect();
    composers.push(contributor(RACHMANINOFF, "composer"));
    let mut prelude = piano_piece(
        "Prelude in C-sharp minor",
        "C-sharp minor",
        None,
        &["Op. 3 No. 2"],
    );
    prelude.stars = Some(5);
    prelude.note = Some("The big chords at the end need the whole arm, not the fingers.".into());
    // Shared with its publication below, so browsing a tag has a case that spans both kinds
    prelude.tags = vec!["learning".into(), "russian".into()];
    let russian_album = create(
        &sheet_music,
        PublicationInput {
            title: "Russian piano album".into(),
            publisher: Some("Schirmer".into()),
            year: None,
            stars: Some(4),
            note: None,
            tags: vec!["anthology".into(), "russian".into()],
            holdings: vec![physical(None)],
            identifiers: vec![
                normalized(Kind::Isbn, "978-1-4950-0871-9")?,
                normalized(Kind::PublisherNumber, "Vol 2115")?,
            ],
            contributors: composers,
            links: Vec::new(),
            contents: vec![
                prelude,
                piano_piece("Etude-Tableau", "A minor", None, &["Op. 39 No. 2"]),
            ],
        },
    )
    .await?;

    let masterpieces = create(
        &sheet_music,
        PublicationInput {
            title: "Rachmaninoff masterpieces for solo piano".into(),
            publisher: Some("Dover".into()),
            year: None,
            stars: None,
            note: None,
            tags: Vec::new(),
            holdings: vec![physical(None)],
            identifiers: vec![normalized(Kind::Isbn, "0-486-43122-3")?],
            contributors: vec![contributor(RACHMANINOFF, "composer")],
            links: Vec::new(),
            contents: vec![piano_piece(
                "Polichinelle",
                "F-sharp minor",
                None,
                &["Op. 3 No. 4"],
            )],
        },
    )
    .await?;

    // A transcription published on its own, one work with two contributors, identified by a plate number
    let mut tone_poem = piano_piece("The Isle of the Dead", "A minor", Some("5/8"), &["Op. 29"]);
    tone_poem
        .contributors
        .push(contributor("Georgy Kirkor", "arranger"));
    tone_poem.stars = Some(4);
    tone_poem.tags = vec!["transcription".into(), "want-to-learn".into()];
    tone_poem.links = links(&[
        "https://imslp.org/wiki/Isle_of_the_Dead,_Op.29_(Rachmaninoff,_Sergei)",
        "https://musicbrainz.org/work/ab65bc19-0079-31a9-9521-5f6ea4c1c637",
        "https://en.wikipedia.org/wiki/Isle_of_the_Dead_(Rachmaninoff)",
        "https://www.wikidata.org/wiki/Q629711",
    ]);
    create(
        &sheet_music,
        PublicationInput {
            title: "The Isle of the Dead".into(),
            publisher: Some("State Music Publishers".into()),
            year: None,
            stars: None,
            note: None,
            tags: Vec::new(),
            holdings: vec![physical(None)],
            identifiers: vec![normalized(Kind::PlateNumber, "M 26277")?],
            contributors: vec![
                contributor(RACHMANINOFF, "composer"),
                contributor("Georgy Kirkor", "arranger"),
            ],
            links: Vec::new(),
            contents: vec![tone_poem],
        },
    )
    .await?;

    create(
        &sheet_music,
        PublicationInput {
            title: "Three gymnopedies for the piano".into(),
            publisher: Some("Schirmer".into()),
            year: None,
            stars: Some(4),
            note: None,
            tags: Vec::new(),
            holdings: vec![physical(Some("Piano bench"))],
            identifiers: vec![
                normalized(Kind::Isbn, "978-0-7935-2590-4")?,
                normalized(Kind::PublisherNumber, "Vol 1869")?,
            ],
            contributors: vec![contributor(SATIE, "composer")],
            links: links(&["https://imslp.org/wiki/3_Gymnop%C3%A9dies_(Satie,_Erik)"]),
            contents: ["D major", "C major", "A minor"]
                .into_iter()
                .enumerate()
                .map(|(i, key)| gymnopedie(i + 1, key))
                .collect(),
        },
    )
    .await?;

    link_person(
        &sheet_music,
        "composer",
        RACHMANINOFF,
        &[
            "https://imslp.org/wiki/Category:Rachmaninoff,_Sergei",
            "https://en.wikipedia.org/wiki/Sergei_Rachmaninoff",
            "https://www.wikidata.org/wiki/Q131861",
        ],
    )
    .await?;
    link_person(
        &sheet_music,
        "arranger",
        "Georgy Kirkor",
        &[
            "https://imslp.org/wiki/Category:Kirkor,_Georgy",
            "https://www.wikidata.org/wiki/Q23656067",
        ],
    )
    .await?;

    let prelude_id = russian_album.works().await?[0].id;
    // The same work in two publications, so work pages list more than one
    let event = Event {
        fields: vec![Field::Contents],
        ..Event::about(Action::Updated, masterpieces.entity_ref())
    };
    let mut audited = archive.shared.begin_audit(Source::User, event).await?;
    work::link_work_to_publication(&mut audited, sheet_music.id, masterpieces.id, prelude_id)
        .await?;
    audited.commit().await?;

    Ok(())
}

/// Create a publication, crediting the persons the library already has by name
async fn create(library: &Library, mut input: PublicationInput) -> Result<Publication> {
    let persons = library.person_summaries(None).await?;
    for contributor in input.contributors_mut() {
        contributor.resolve_by_name(&persons);
    }
    library.create_publication(&input).await
}

/// Give a credited person their links, leaving their name as it is.
async fn link_person(library: &Library, role: &str, name: &str, urls: &[&str]) -> Result<()> {
    let mut person = library
        .persons_with_role(role)
        .await?
        .into_iter()
        .find(|person| person.name == name)
        .expect("the demo credits this person on a publication above");
    let input = PersonRawInput {
        name: name.into(),
        links: links(urls),
    };
    let input = input.parse().expect("the demo's links are valid");
    person.update(&input).await
}

/// The demo's URLs are written already normalized, so they skip the parser the web forms use
fn links(urls: &[&str]) -> Vec<String> {
    urls.iter().map(|url| url.to_string()).collect()
}

fn contributor(name: &str, role: &str) -> ContributorInput {
    ContributorInput {
        name: name.into(),
        role: role.into(),
        person: PersonRef::Unresolved,
    }
}

fn physical(location: Option<&str>) -> HoldingInput {
    HoldingInput {
        id: None,
        kind: HoldingKind::Physical,
        location: location.map(str::to_string),
    }
}

fn digital(location: &str) -> HoldingInput {
    HoldingInput {
        id: None,
        kind: HoldingKind::Digital,
        location: Some(location.into()),
    }
}

fn normalized(kind: Kind, value: &str) -> Result<(Kind, Normalized)> {
    Ok((kind, identifier::normalize(kind, value)?))
}

/// A piece for solo piano credited to Rachmaninoff, which is most of the demo's sheet music
fn piano_piece(
    title: &str,
    key: &str,
    time_signature: Option<&str>,
    catalog_numbers: &[&str],
) -> WorkInput {
    WorkInput {
        id: None,
        title: title.into(),
        key: Some(key.into()),
        time_signature: time_signature.map(str::to_string),
        instrumentation: Some("piano".into()),
        stars: None,
        note: None,
        tags: Vec::new(),
        contributors: vec![contributor(RACHMANINOFF, "composer")],
        catalog_numbers: catalog_numbers
            .iter()
            .map(|number| CatalogNumber::parse(number))
            .collect(),
        links: Vec::new(),
    }
}

fn gymnopedie(number: usize, key: &str) -> WorkInput {
    WorkInput {
        id: None,
        title: format!("Gymnopedie No. {number}"),
        key: Some(key.into()),
        time_signature: Some("3/4".into()),
        instrumentation: Some("piano".into()),
        stars: (number == 1).then_some(5),
        note: None,
        tags: match number {
            1 => vec!["memorized".into()],
            _ => vec!["want-to-learn".into()],
        },
        contributors: vec![contributor(SATIE, "composer")],
        catalog_numbers: Vec::new(),
        links: Vec::new(),
    }
}

/// A prose work, with none of the fields a piece of music has
fn writing(title: &str) -> WorkInput {
    WorkInput {
        id: None,
        title: title.into(),
        key: None,
        time_signature: None,
        instrumentation: None,
        stars: None,
        note: None,
        tags: Vec::new(),
        contributors: vec![contributor(BIERCE, "author")],
        catalog_numbers: Vec::new(),
        links: Vec::new(),
    }
}
