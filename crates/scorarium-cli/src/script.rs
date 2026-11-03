use std::io::{BufRead, Write};
use std::time::Duration;

use eyre::WrapErr;
use scorarium_engine::Engine;

use crate::command::{Flow, Shell};
use crate::enrichment;

/// Run the given scorarium script, writing its output to the given writer.
pub async fn run_script(
    engine: &Engine,
    source: impl BufRead,
    out: &mut impl Write,
) -> eyre::Result<()> {
    let mut shell = Shell::new(engine.clone());
    for (index, line) in source.lines().enumerate() {
        let line = line?;
        let flow = shell
            .execute(&line, out)
            .await
            .wrap_err_with(|| format!("line {}", index + 1))?;
        if let Flow::Quit = flow {
            break;
        }
    }
    enrichment::wait(engine, Some(Duration::from_secs(10))).await;
    Ok(())
}
