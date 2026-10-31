use std::io::Write;

use scorarium_engine::Engine;
use scorarium_engine::entity::Entity;
use scorarium_engine::holding::NewHolding;
use scorarium_engine::library::Library;
use scorarium_engine::publication::{Identifier, IdentifierKind, ImportSeed};

use crate::reference;

pub(crate) async fn list(engine: &Engine, out: &mut impl Write) -> eyre::Result<()> {
    let rows: Vec<[String; 3]> = engine
        .libraries()
        .await?
        .into_iter()
        .map(|library| {
            [
                library.id().to_string(),
                library.name().to_string(),
                visibility(library.is_private()).to_string(),
            ]
        })
        .collect();
    write_table(out, &rows)
}

/// Find a library by name or id
///
/// Fail if the given query is ambiguous, or a matching library does not exist.
pub(crate) async fn resolve(engine: &Engine, reference: &str) -> eyre::Result<Library> {
    reference::resolve(
        "library",
        reference,
        engine.libraries().await?,
        |l| l.id(),
        |l| l.name(),
    )
}

pub(crate) async fn show(
    engine: &Engine,
    reference: &str,
    out: &mut impl Write,
) -> eyre::Result<()> {
    let library = resolve(engine, reference).await?;
    let publications = library.publications().await?.len();
    let rows = [
        ["id".to_string(), library.id().to_string()],
        ["name".to_string(), library.name().to_string()],
        [
            "visibility".to_string(),
            visibility(library.is_private()).to_string(),
        ],
        ["publications".to_string(), publications.to_string()],
    ];
    write_table(out, &rows)
}

/// Create a draft publication from a title or an ISBN/ISMN, and print its id
///
/// The seed is a title unless it parses as an ISBN or an ISMN.
pub(crate) async fn import(
    engine: &Engine,
    reference: &str,
    seed: &str,
    location: Option<&str>,
    out: &mut impl Write,
) -> eyre::Result<()> {
    let library = resolve(engine, reference).await?;
    let seed = Identifier::new(IdentifierKind::Isbn, seed)
        .or_else(|_| Identifier::new(IdentifierKind::Ismn, seed))
        .map_or_else(
            |_| ImportSeed::Title(seed.to_string()),
            ImportSeed::Identifier,
        );
    let publication = library
        .create_publication(seed, NewHolding::Physical { location })
        .await?;
    writeln!(out, "{}", publication.id())?;
    Ok(())
}

pub(crate) fn visibility(private: bool) -> &'static str {
    if private { "private" } else { "public" }
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub(crate) enum Visibility {
    Public,
    Private,
}

pub(crate) async fn create(engine: &Engine, raw: &str, visibility: Visibility) -> eyre::Result<()> {
    let private = matches!(visibility, Visibility::Private);
    engine.create_library(raw, private).await?;
    Ok(())
}

/// Rename a library, keeping its visibility.
pub(crate) async fn set_name(engine: &Engine, reference: &str, raw: &str) -> eyre::Result<()> {
    resolve(engine, reference).await?.rename(raw).await?;
    Ok(())
}

/// Make a library public or private.
pub(crate) async fn set_visibility(
    engine: &Engine,
    reference: &str,
    visibility: Visibility,
) -> eyre::Result<()> {
    let private = matches!(visibility, Visibility::Private);
    resolve(engine, reference)
        .await?
        .set_private(private)
        .await?;
    Ok(())
}

pub(crate) async fn delete(engine: &Engine, reference: &str) -> eyre::Result<()> {
    resolve(engine, reference).await?.delete().await?;
    Ok(())
}

pub(crate) fn write_table<const N: usize>(
    out: &mut impl Write,
    rows: &[[String; N]],
) -> eyre::Result<()> {
    let mut widths = [0; N];
    for row in rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    for row in rows {
        let (last, padded) = row.split_last().expect("N > 0");
        for (cell, width) in padded.iter().zip(widths) {
            write!(out, "{cell:<width$}\t")?;
        }
        writeln!(out, "{last}")?;
    }
    Ok(())
}
