use std::io::Write;

use scorarium_engine::archive::{Archive, Library};

use crate::reference;

pub(crate) async fn list(archive: &Archive, out: &mut impl Write) -> eyre::Result<()> {
    let rows: Vec<[String; 3]> = archive
        .libraries()
        .await?
        .into_iter()
        .map(|library| {
            [
                library.id.to_string(),
                library.name,
                visibility(library.private).to_string(),
            ]
        })
        .collect();
    write_table(out, &rows)
}

/// Find a library by name or id
///
/// Fail if the given query is ambiguous, or a matching library does not exist.
pub(crate) async fn resolve(archive: &Archive, reference: &str) -> eyre::Result<Library> {
    reference::resolve(
        "library",
        reference,
        archive.libraries().await?,
        |l| l.id,
        |l| &l.name,
    )
}

pub(crate) async fn show(
    archive: &Archive,
    reference: &str,
    out: &mut impl Write,
) -> eyre::Result<()> {
    let library = resolve(archive, reference).await?;
    let publications = library.publication_count().await?;
    let rows = [
        ["id".to_string(), library.id.to_string()],
        ["name".to_string(), library.name],
        [
            "visibility".to_string(),
            visibility(library.private).to_string(),
        ],
        ["publications".to_string(), publications.to_string()],
    ];
    write_table(out, &rows)
}

fn visibility(private: bool) -> &'static str {
    if private { "private" } else { "public" }
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub(crate) enum Visibility {
    Public,
    Private,
}

fn validate_name(raw: &str) -> eyre::Result<&str> {
    let name = raw.trim();
    if name.is_empty() {
        eyre::bail!("library name is empty");
    }
    Ok(name)
}

pub(crate) async fn create(
    archive: &Archive,
    raw: &str,
    visibility: Visibility,
) -> eyre::Result<()> {
    let private = matches!(visibility, Visibility::Private);
    let name = validate_name(raw)?;
    let mut edit = archive.edit().await?;
    edit.create_library(name, private).await?;
    edit.commit().await?;
    Ok(())
}

/// Rename a library, keeping its visibility.
pub(crate) async fn rename(archive: &Archive, reference: &str, raw: &str) -> eyre::Result<()> {
    let id = resolve(archive, reference).await?.id;
    let name = validate_name(raw)?;
    let mut edit = archive.edit().await?;
    edit.library(id).await?.rename(name).await?;
    edit.commit().await?;
    Ok(())
}

pub(crate) async fn delete(archive: &Archive, reference: &str) -> eyre::Result<()> {
    let id = resolve(archive, reference).await?.id;
    let mut edit = archive.edit().await?;
    edit.library(id).await?.delete().await?;
    edit.commit().await?;
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
