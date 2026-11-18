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
    enrich: bool,
) -> eyre::Result<()> {
    let mut shell = Shell::new(engine.clone(), false, enrich);
    let mut command = String::new();
    let mut first = 0;
    for (index, line) in source.lines().enumerate() {
        if command.is_empty() {
            first = index + 1;
        }
        command.push_str(&line?);
        if shlex::split(&command).is_none() {
            command.push('\n');
            continue;
        }
        let flow = shell
            .execute(&command, out)
            .await
            .wrap_err_with(|| format!("line {first}"))?;
        command.clear();
        if let Flow::Quit = flow {
            break;
        }
    }
    if !command.is_empty() {
        eyre::bail!("line {first}: unterminated quote or trailing backslash");
    }
    enrichment::wait(engine, Some(Duration::from_secs(10))).await;
    Ok(())
}
