use crate::holding::NewHolding;
use crate::library::Library;
use crate::publication::{ImportSeed, Publication};
use crate::value::Classification::{self, Music, Prose};
use crate::value::{Stars, Year};
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

    let mut russian_album = create(&sheet_music, "Russian piano album", None, Music).await?;
    russian_album.set_publisher(Some("Schirmer")).await?;
    russian_album.set_stars(Some(Stars::try_from(4)?)).await?;

    let mut masterpieces = create(
        &sheet_music,
        "Rachmaninoff masterpieces for solo piano",
        None,
        Music,
    )
    .await?;
    masterpieces.set_publisher(Some("Dover")).await?;

    let mut isle_of_the_dead = create(&sheet_music, "The Isle of the Dead", None, Music).await?;
    isle_of_the_dead
        .set_publisher(Some("State Music Publishers"))
        .await?;

    let mut gymnopedies = create(
        &sheet_music,
        "Three gymnopedies for the piano",
        Some("Piano bench"),
        Music,
    )
    .await?;
    gymnopedies.set_publisher(Some("Schirmer")).await?;
    gymnopedies.set_stars(Some(Stars::try_from(4)?)).await?;

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
