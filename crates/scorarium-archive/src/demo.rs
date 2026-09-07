use crate::holding::{HoldingInput, HoldingKind};
use crate::identifier::{self, Kind, Normalized};
use crate::input::ContributorInput;
use crate::publication::PublicationInput;
use crate::work::{self, WorkInput};
use crate::{Archive, Result};

const RACHMANINOFF: &str = "Sergei Rachmaninoff";
const SATIE: &str = "Erik Satie";
const BIERCE: &str = "Ambrose Bierce";

pub(crate) async fn populate(archive: &Archive) -> Result<()> {
    let books = archive.create_library("Books").await?;
    let sheet_music = archive.create_library("Sheet music").await?;

    books
        .create_publication(&PublicationInput {
            title: "Practical Vim".into(),
            publisher: Some("Pragmatic Bookshelf".into()),
            year: Some(2015),
            holdings: vec![physical(Some("Desk"))],
            identifiers: vec![normalized(Kind::Isbn, "978-1-68050-127-8")?],
            contributors: vec![contributor("Drew Neil", "author")],
            contents: Vec::new(),
        })
        .await?;

    books
        .create_publication(&PublicationInput {
            title: "Pro Git".into(),
            publisher: Some("Apress".into()),
            year: Some(2014),
            holdings: vec![physical(None), digital("pro-git.pdf")],
            identifiers: vec![normalized(Kind::Isbn, "978-1-4842-0077-3")?],
            contributors: vec![
                contributor("Scott Chacon", "author"),
                contributor("Ben Straub", "author"),
            ],
            contents: Vec::new(),
        })
        .await?;

    // A book with works, so pages show works without any music-specific fields
    books
        .create_publication(&PublicationInput {
            title: "The Collected Writings of Ambrose Bierce".into(),
            publisher: Some("Citadel Press".into()),
            year: Some(1979),
            holdings: vec![physical(None)],
            identifiers: vec![normalized(Kind::Isbn, "0-8065-0180-4")?],
            contributors: vec![contributor(BIERCE, "author")],
            contents: vec![
                writing("In the Midst of Life"),
                writing("The Devil's Dictionary"),
                writing("The Parenticide Club"),
            ],
        })
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
    let russian_album = sheet_music
        .create_publication(&PublicationInput {
            title: "Russian piano album".into(),
            publisher: Some("Schirmer".into()),
            year: None,
            holdings: vec![physical(None)],
            identifiers: vec![
                normalized(Kind::Isbn, "978-1-4950-0871-9")?,
                normalized(Kind::PublisherNumber, "Vol 2115")?,
            ],
            contributors: composers,
            contents: vec![
                piano_piece("Prelude in C-sharp minor", "C-sharp minor", None),
                piano_piece("Etude-Tableau", "A minor", None),
            ],
        })
        .await?;

    let masterpieces = sheet_music
        .create_publication(&PublicationInput {
            title: "Rachmaninoff masterpieces for solo piano".into(),
            publisher: Some("Dover".into()),
            year: None,
            holdings: vec![physical(None)],
            identifiers: vec![normalized(Kind::Isbn, "0-486-43122-3")?],
            contributors: vec![contributor(RACHMANINOFF, "composer")],
            contents: vec![piano_piece("Polichinelle", "F-sharp minor", None)],
        })
        .await?;

    // A transcription published on its own, one work with two contributors, identified by a plate number
    let mut tone_poem = piano_piece("The Isle of the Dead", "A minor", Some("5/8"));
    tone_poem
        .contributors
        .push(contributor("Georgy Kirkor", "arranger"));
    let isle_of_the_dead = sheet_music
        .create_publication(&PublicationInput {
            title: "The Isle of the Dead".into(),
            publisher: Some("State Music Publishers".into()),
            year: None,
            holdings: vec![physical(None)],
            identifiers: vec![normalized(Kind::PlateNumber, "M 26277")?],
            contributors: vec![
                contributor(RACHMANINOFF, "composer"),
                contributor("Georgy Kirkor", "arranger"),
            ],
            contents: vec![tone_poem],
        })
        .await?;

    sheet_music
        .create_publication(&PublicationInput {
            title: "Three gymnopedies for the piano".into(),
            publisher: Some("Schirmer".into()),
            year: None,
            holdings: vec![physical(Some("Piano bench"))],
            identifiers: vec![
                normalized(Kind::Isbn, "978-0-7935-2590-4")?,
                normalized(Kind::PublisherNumber, "Vol 1869")?,
            ],
            contributors: vec![contributor(SATIE, "composer")],
            contents: ["D major", "C major", "A minor"]
                .into_iter()
                .enumerate()
                .map(|(i, key)| gymnopedie(i + 1, key))
                .collect(),
        })
        .await?;

    let album_works = russian_album.works().await?;
    let masterpiece_works = masterpieces.works().await?;
    let isle_works = isle_of_the_dead.works().await?;
    let prelude = &album_works[0];
    let mut conn = archive.shared.pool.acquire().await?;
    for (work_id, value) in [
        (prelude.id, "Op. 3 No. 2"),
        (album_works[1].id, "Op. 39 No. 2"),
        (masterpiece_works[0].id, "Op. 3 No. 4"),
        (isle_works[0].id, "Op. 29"),
    ] {
        work::add_work_catalog_number(&mut conn, work_id, value).await?;
    }
    // The same work in two publications, so work pages list more than one
    work::link_work_to_publication(&mut conn, sheet_music.id, masterpieces.id, prelude.id).await?;

    Ok(())
}

fn contributor(name: &str, role: &str) -> ContributorInput {
    ContributorInput {
        name: name.into(),
        role: role.into(),
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
fn piano_piece(title: &str, key: &str, time_signature: Option<&str>) -> WorkInput {
    WorkInput {
        id: None,
        title: title.into(),
        key: Some(key.into()),
        time_signature: time_signature.map(str::to_string),
        instrumentation: Some("piano".into()),
        contributors: vec![contributor(RACHMANINOFF, "composer")],
    }
}

fn gymnopedie(number: usize, key: &str) -> WorkInput {
    WorkInput {
        id: None,
        title: format!("Gymnopedie No. {number}"),
        key: Some(key.into()),
        time_signature: Some("3/4".into()),
        instrumentation: Some("piano".into()),
        contributors: vec![contributor(SATIE, "composer")],
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
        contributors: vec![contributor(BIERCE, "author")],
    }
}
