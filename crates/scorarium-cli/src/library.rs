use std::io::Write;

use scorarium_archive::Archive;

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

fn visibility(private: bool) -> &'static str {
    if private { "private" } else { "public" }
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
