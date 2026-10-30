use std::cmp::Reverse;
use std::io::Write;

use scorarium_engine::library::Library;

use crate::library::write_table;

/// List the tags in a library
pub(crate) async fn list(library: &Library, out: &mut impl Write) -> eyre::Result<()> {
    let mut counts = library.tags(true).await?;
    // The sort is stable, so tags with equal counts stay alphabetical.
    counts.sort_by_key(|(_, count)| Reverse(*count));
    let rows: Vec<[String; 2]> = counts
        .into_iter()
        .map(|(tag, count)| [tag.to_string(), count.to_string()])
        .collect();
    write_table(out, &rows)
}
