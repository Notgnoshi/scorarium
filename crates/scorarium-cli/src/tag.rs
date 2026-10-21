use std::cmp::Reverse;
use std::io::Write;

use scorarium_engine::archive::Library;

use crate::library::write_table;

/// List the tags in a library
pub(crate) async fn list(library: &Library, out: &mut impl Write) -> eyre::Result<()> {
    let mut counts = library.tag_counts().await?;
    // The sort is stable, so tags with equal counts stay alphabetical.
    counts.sort_by_key(|count| Reverse(count.uses));
    let rows: Vec<[String; 2]> = counts
        .into_iter()
        .map(|count| [count.tag, count.uses.to_string()])
        .collect();
    write_table(out, &rows)
}
