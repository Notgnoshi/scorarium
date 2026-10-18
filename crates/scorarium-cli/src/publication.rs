use std::io::Write;

use scorarium_engine::archive::{Library, Tag};

use crate::library::write_table;

/// List a library's publications, keeping only those carrying every given tag.
pub(crate) async fn list(
    library: &Library,
    tags: &[Tag],
    out: &mut impl Write,
) -> eyre::Result<()> {
    let rows: Vec<[String; 5]> = library
        .publications()
        .await?
        .into_iter()
        .filter(|publication| {
            tags.iter()
                .all(|tag| publication.tags.iter().any(|t| t == tag.as_str()))
        })
        .map(|publication| {
            let contributors: Vec<&str> = publication
                .contributors
                .iter()
                .map(|contributor| contributor.name.as_str())
                .collect();
            let holdings: Vec<&str> = publication
                .holdings
                .iter()
                .map(|holding| holding.kind.as_str())
                .collect();
            [
                publication.id.to_string(),
                publication.title,
                contributors.join(", "),
                publication
                    .year
                    .map(|year| year.to_string())
                    .unwrap_or_default(),
                holdings.join(", "),
            ]
        })
        .collect();
    write_table(out, &rows)
}
