use std::io::{BufRead, Write};

use eyre::WrapErr;
use scorarium_archive::Archive;

use crate::command::{self, Flow};

/// Run the given scorarium script, writing its output to the given writer.
pub async fn run_script(
    archive: &Archive,
    source: impl BufRead,
    out: &mut impl Write,
) -> eyre::Result<()> {
    for (index, line) in source.lines().enumerate() {
        let line = line?;
        let flow = command::execute(archive, &line, out)
            .await
            .wrap_err_with(|| format!("line {}", index + 1))?;
        if let Flow::Quit = flow {
            break;
        }
    }
    Ok(())
}
