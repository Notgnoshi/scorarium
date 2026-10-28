use crate::holding::NewHolding;
use crate::library::Library;
use crate::publication::{ImportSeed, Publication};
use crate::value::Classification::{self, Music, Prose};
use crate::value::{Stars, Year};
use crate::work::Work;
use crate::{Engine, Result};

/// Populate a demo database intended to exercise several different aspects of Scorarium's data model
pub(crate) async fn populate(engine: &Engine) -> Result<()> {
    let books = engine.create_library("Books", true).await?;
    let sheet_music = engine.create_library("Sheet music", false).await?;

    let mut practical_vim = create(&books, "Practical Vim", Some("Desk"), Prose).await?;
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

    let mut pro_git = create(&books, "Pro Git", None, Prose).await?;
    pro_git.set_publisher(Some("Apress")).await?;
    pro_git.set_year(Some(Year::try_from(2014)?)).await?;
    pro_git.set_stars(Some(Stars::try_from(3)?)).await?;

    let mut bierce = create(
        &books,
        "The Collected Writings of Ambrose Bierce",
        None,
        Prose,
    )
    .await?;
    bierce.set_publisher(Some("Citadel Press")).await?;
    bierce.set_year(Some(Year::try_from(1979)?)).await?;
    bierce.create_work("In the Midst of Life").await?;
    bierce.create_work("The Devil's Dictionary").await?;
    bierce.create_work("The Parenticide Club").await?;

    let mut russian_album = create(&sheet_music, "Russian piano album", None, Music).await?;
    russian_album.set_publisher(Some("Schirmer")).await?;
    russian_album.set_stars(Some(Stars::try_from(4)?)).await?;
    let mut prelude = piano_piece(
        &mut russian_album,
        "Prelude in C-sharp minor",
        "C-sharp minor",
        None,
    )
    .await?;
    prelude.set_stars(Some(Stars::try_from(5)?)).await?;
    prelude
        .set_note(Some(
            "The big chords at the end need the whole arm, not the fingers.",
        ))
        .await?;
    piano_piece(&mut russian_album, "Etude-Tableau", "A minor", None).await?;

    let mut masterpieces = create(
        &sheet_music,
        "Rachmaninoff masterpieces for solo piano",
        None,
        Music,
    )
    .await?;
    masterpieces.set_publisher(Some("Dover")).await?;
    piano_piece(&mut masterpieces, "Polichinelle", "F-sharp minor", None).await?;

    let mut isle_of_the_dead = create(&sheet_music, "The Isle of the Dead", None, Music).await?;
    isle_of_the_dead
        .set_publisher(Some("State Music Publishers"))
        .await?;
    let mut tone_poem = piano_piece(
        &mut isle_of_the_dead,
        "The Isle of the Dead",
        "A minor",
        Some("5/8"),
    )
    .await?;
    tone_poem.set_stars(Some(Stars::try_from(4)?)).await?;

    let mut gymnopedies = create(
        &sheet_music,
        "Three gymnopedies for the piano",
        Some("Piano bench"),
        Music,
    )
    .await?;
    gymnopedies.set_publisher(Some("Schirmer")).await?;
    gymnopedies.set_stars(Some(Stars::try_from(4)?)).await?;
    for (number, key) in [(1, "D major"), (2, "C major"), (3, "A minor")] {
        let title = format!("Gymnopedie No. {number}");
        let mut gymnopedie = piano_piece(&mut gymnopedies, &title, key, Some("3/4")).await?;
        if number == 1 {
            gymnopedie.set_stars(Some(Stars::try_from(5)?)).await?;
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

/// Create a piece for solo piano in `publication`
async fn piano_piece(
    publication: &mut Publication,
    title: &str,
    key: &str,
    time_signature: Option<&str>,
) -> Result<Work> {
    let mut work = publication.create_work(title).await?;
    work.set_key(Some(key)).await?;
    work.set_time_signature(time_signature).await?;
    work.set_instrumentation(Some("piano")).await?;
    Ok(work)
}
