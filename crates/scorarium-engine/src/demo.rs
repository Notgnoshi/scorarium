use crate::entity::Entity;
use crate::holding::NewHolding;
use crate::library::Library;
use crate::person::PersonId;
use crate::publication::IdentifierKind::{Isbn, PlateNumber, PublisherNumber};
use crate::publication::{Identifier, ImportSeed, Publication};
use crate::value::Classification::{self, Music, Prose};
use crate::value::{Stars, Year};
use crate::work::Work;
use crate::{Engine, Error, Result};

/// Populate a demo database intended to exercise several different aspects of Scorarium's data model
pub(crate) async fn populate(engine: &Engine) -> Result<()> {
    let books = engine.create_library("Books", true).await?;
    let sheet_music = engine.create_library("Sheet music", false).await?;

    let mut practical_vim = create(&books, "Practical Vim", Some("Desk"), Prose).await?;
    practical_vim
        .add_identifier(Identifier::new(Isbn, "978-1-68050-127-8")?)
        .await?;
    practical_vim
        .set_publisher(Some("Pragmatic Bookshelf"))
        .await?;
    practical_vim.set_year(Some(Year::try_from(2015)?)).await?;
    practical_vim.set_stars(Some(Stars::try_from(5)?)).await?;
    practical_vim
        .set_note(Some(
            "The chapter on the dot command is the one to reread.\n\nLent to Sam once",
        ))
        .await?;
    practical_vim
        .create_contributor("Drew Neil", "author")
        .await?;
    for tag in ["editor", "reference"] {
        practical_vim.add_tag(tag.parse()?).await?;
    }
    for url in [
        "https://openlibrary.org/books/OL27196589M/Practical_Vim",
        "https://www.goodreads.com/book/show/42854052-practical-vim",
    ] {
        practical_vim.add_link(url.parse()?).await?;
    }

    let mut pro_git = create(&books, "Pro Git", None, Prose).await?;
    pro_git
        .add_identifier(Identifier::new(Isbn, "978-1-4842-0077-3")?)
        .await?;
    pro_git.set_publisher(Some("Apress")).await?;
    pro_git.set_year(Some(Year::try_from(2014)?)).await?;
    pro_git.set_stars(Some(Stars::try_from(3)?)).await?;
    pro_git.create_contributor("Scott Chacon", "author").await?;
    pro_git.create_contributor("Ben Straub", "author").await?;
    for url in [
        "https://openlibrary.org/books/OL26372169M/Pro_Git",
        "https://git-scm.com/book/en/v2",
    ] {
        pro_git.add_link(url.parse()?).await?;
    }

    let mut bierce = create(
        &books,
        "The Collected Writings of Ambrose Bierce",
        None,
        Prose,
    )
    .await?;
    bierce
        .add_identifier(Identifier::new(Isbn, "0-8065-0180-4")?)
        .await?;
    bierce.set_publisher(Some("Citadel Press")).await?;
    bierce.set_year(Some(Year::try_from(1979)?)).await?;
    let mut ambrose_bierce = bierce
        .create_contributor("Ambrose Bierce", "author")
        .await?;
    for url in [
        "https://www.wikidata.org/wiki/Q191050",
        "https://d-nb.info/gnd/115511474",
        "https://www.goodreads.com/author/show/14403",
    ] {
        ambrose_bierce.add_link(url.parse()?).await?;
    }
    for title in [
        "In the Midst of Life",
        "The Devil's Dictionary",
        "The Parenticide Club",
    ] {
        let mut writing = bierce.create_work(title).await?;
        writing
            .add_contributor(ambrose_bierce.id(), "author")
            .await?;
        if title == "The Devil's Dictionary" {
            writing
                .add_link("https://www.gutenberg.org/ebooks/972".parse()?)
                .await?;
        }
    }

    let mut russian_album = create(&sheet_music, "Russian piano album", None, Music).await?;
    russian_album
        .add_identifier(Identifier::new(Isbn, "978-1-4950-0871-9")?)
        .await?;
    russian_album
        .add_identifier(Identifier::new(PublisherNumber, "Vol 2115")?)
        .await?;
    russian_album.set_publisher(Some("Schirmer")).await?;
    russian_album.set_stars(Some(Stars::try_from(4)?)).await?;
    for tag in ["anthology", "russian"] {
        russian_album.add_tag(tag.parse()?).await?;
    }
    for composer in [
        "Dmitri Kabalevsky",
        "Modest Mussorgsky",
        "Sergei Prokofiev",
        "Dmitri Shostakovich",
        "Pyotr Ilyich Tchaikovsky",
        "Sergei Rachmaninoff",
    ] {
        russian_album
            .create_contributor(composer, "composer")
            .await?;
    }
    let mut rachmaninoff = sheet_music
        .people(Some("composer"))
        .await?
        .into_iter()
        .find(|person| person.name() == "Sergei Rachmaninoff")
        .ok_or(Error::NotFound)?;
    for url in [
        "https://imslp.org/wiki/Category:Rachmaninoff,_Sergei",
        "https://en.wikipedia.org/wiki/Sergei_Rachmaninoff",
        "https://www.wikidata.org/wiki/Q131861",
    ] {
        rachmaninoff.add_link(url.parse()?).await?;
    }
    let mut prelude = piano_piece(
        &mut russian_album,
        rachmaninoff.id(),
        "Prelude in C-sharp minor",
        "C-sharp minor",
        None,
    )
    .await?;
    prelude.add_catalog_number("Op. 3 No. 2".parse()?).await?;
    prelude.set_stars(Some(Stars::try_from(5)?)).await?;
    prelude
        .set_note(Some(
            "The big chords at the end need the whole arm, not the fingers.",
        ))
        .await?;
    for tag in ["learning", "russian"] {
        prelude.add_tag(tag.parse()?).await?;
    }
    let mut etude_tableau = piano_piece(
        &mut russian_album,
        rachmaninoff.id(),
        "Etude-Tableau",
        "A minor",
        None,
    )
    .await?;
    etude_tableau
        .add_catalog_number("Op. 39 No. 2".parse()?)
        .await?;

    let mut masterpieces = create(
        &sheet_music,
        "Rachmaninoff masterpieces for solo piano",
        None,
        Music,
    )
    .await?;
    masterpieces
        .add_identifier(Identifier::new(Isbn, "0-486-43122-3")?)
        .await?;
    masterpieces.set_publisher(Some("Dover")).await?;
    masterpieces
        .add_contributor(rachmaninoff.id(), "composer")
        .await?;
    masterpieces.add_work(prelude.id()).await?;
    let mut polichinelle = piano_piece(
        &mut masterpieces,
        rachmaninoff.id(),
        "Polichinelle",
        "F-sharp minor",
        None,
    )
    .await?;
    polichinelle
        .add_catalog_number("Op. 3 No. 4".parse()?)
        .await?;

    let mut isle_of_the_dead = create(&sheet_music, "The Isle of the Dead", None, Music).await?;
    isle_of_the_dead
        .add_identifier(Identifier::new(PlateNumber, "M 26277")?)
        .await?;
    isle_of_the_dead
        .set_publisher(Some("State Music Publishers"))
        .await?;
    isle_of_the_dead
        .add_contributor(rachmaninoff.id(), "composer")
        .await?;
    let mut kirkor = isle_of_the_dead
        .create_contributor("Georgy Kirkor", "arranger")
        .await?;
    for url in [
        "https://imslp.org/wiki/Category:Kirkor,_Georgy",
        "https://www.wikidata.org/wiki/Q23656067",
    ] {
        kirkor.add_link(url.parse()?).await?;
    }
    let mut tone_poem = piano_piece(
        &mut isle_of_the_dead,
        rachmaninoff.id(),
        "The Isle of the Dead",
        "A minor",
        Some("5/8"),
    )
    .await?;
    tone_poem.add_contributor(kirkor.id(), "arranger").await?;
    tone_poem.add_catalog_number("Op. 29".parse()?).await?;
    tone_poem.set_stars(Some(Stars::try_from(5)?)).await?;
    for tag in ["transcription", "want-to-learn"] {
        tone_poem.add_tag(tag.parse()?).await?;
    }
    for url in [
        "https://imslp.org/wiki/Isle_of_the_Dead,_Op.29_(Rachmaninoff,_Sergei)",
        "https://musicbrainz.org/work/ab65bc19-0079-31a9-9521-5f6ea4c1c637",
        "https://en.wikipedia.org/wiki/Isle_of_the_Dead_(Rachmaninoff)",
        "https://www.wikidata.org/wiki/Q629711",
    ] {
        tone_poem.add_link(url.parse()?).await?;
    }

    let mut gymnopedies = create(
        &sheet_music,
        "Three gymnopedies for the piano",
        Some("Piano bench"),
        Music,
    )
    .await?;
    gymnopedies
        .add_identifier(Identifier::new(Isbn, "978-0-7935-2590-4")?)
        .await?;
    gymnopedies
        .add_identifier(Identifier::new(PublisherNumber, "Vol 1869")?)
        .await?;
    gymnopedies.set_publisher(Some("Schirmer")).await?;
    gymnopedies.set_stars(Some(Stars::try_from(4)?)).await?;
    gymnopedies
        .add_link("https://imslp.org/wiki/3_Gymnop%C3%A9dies_(Satie,_Erik)".parse()?)
        .await?;
    let satie = gymnopedies
        .create_contributor("Erik Satie", "composer")
        .await?
        .id();
    for (number, key) in [(1, "D major"), (2, "C major"), (3, "A minor")] {
        let title = format!("Gymnopedie No. {number}");
        let mut gymnopedie = piano_piece(&mut gymnopedies, satie, &title, key, Some("3/4")).await?;
        if number == 1 {
            gymnopedie.set_stars(Some(Stars::try_from(5)?)).await?;
            gymnopedie.add_tag("memorized".parse()?).await?;
        } else {
            gymnopedie.add_tag("want-to-learn".parse()?).await?;
        }
    }

    for library in [&books, &sheet_music] {
        for mut publication in library.publications().await? {
            publication.commit().await?;
        }
    }

    Ok(())
}

/// Create a classified publication with one physical holding
async fn create(
    library: &Library,
    title: &str,
    location: Option<&str>,
    classification: Classification,
) -> Result<Publication> {
    let mut publication = library
        .create_publication(
            ImportSeed::Title(title.into()),
            NewHolding::Physical { location },
        )
        .await?;
    publication.set_classification(classification).await?;
    Ok(publication)
}

/// Create a piece for solo piano by `composer` in `publication`
async fn piano_piece(
    publication: &mut Publication,
    composer: PersonId,
    title: &str,
    key: &str,
    time_signature: Option<&str>,
) -> Result<Work> {
    let mut work = publication.create_work(title).await?;
    work.set_key(Some(key)).await?;
    work.set_time_signature(time_signature).await?;
    work.set_instrumentation(Some("piano")).await?;
    work.add_contributor(composer, "composer").await?;
    Ok(work)
}
