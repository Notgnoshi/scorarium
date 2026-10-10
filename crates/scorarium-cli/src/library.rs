use std::io::Write;

use scorarium_engine::archive::{Archive, Library};

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

/// Find a library by its name, or by its id when no library has that name.
pub(crate) async fn resolve(archive: &Archive, reference: &str) -> eyre::Result<Library> {
    let lowercase = reference.to_lowercase();
    let mut named: Vec<Library> = archive
        .libraries()
        .await?
        .into_iter()
        .filter(|library| library.name.to_lowercase() == lowercase)
        .collect();
    if named.len() > 1 {
        let ids: Vec<String> = named.iter().map(|library| library.id.to_string()).collect();
        eyre::bail!(
            "{} libraries are named '{reference}': ids {}",
            named.len(),
            ids.join(", ")
        );
    }
    if let Some(library) = named.pop() {
        return Ok(library);
    }
    let Ok(id) = reference.parse::<i64>() else {
        eyre::bail!("no library named '{reference}'");
    };
    archive
        .library(id)
        .await?
        .ok_or_else(|| eyre::eyre!("no library {id}"))
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
    archive.create_library(validate_name(raw)?, private).await?;
    Ok(())
}

/// Rename a library, keeping its visibility.
pub(crate) async fn rename(archive: &Archive, reference: &str, raw: &str) -> eyre::Result<()> {
    let mut library = resolve(archive, reference).await?;
    let private = library.private;
    library.update(validate_name(raw)?, private).await
}

pub(crate) async fn delete(archive: &Archive, reference: &str) -> eyre::Result<()> {
    resolve(archive, reference).await?.delete().await
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
