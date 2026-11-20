mod audit;
mod command;
mod completer;
mod enrichment;
mod export;
mod interactive;
mod label;
mod library;
mod log;
mod password;
mod person;
mod publication;
mod reference;
mod review;
mod script;
mod search;
mod tag;
mod work;

use std::io::{BufReader, IsTerminal};
use std::path::PathBuf;

use scorarium_engine::Engine;

pub use crate::export::{ExportLevel, export_script};
pub use crate::log::LogWriter;
pub use crate::script::run_script;

#[derive(Debug, clap::Args)]
pub struct ShellArgs {
    /// A script to run
    pub script: Option<PathBuf>,

    /// Import without starting enrichment
    #[arg(long)]
    pub no_enrich: bool,
}

pub async fn shell(
    args: ShellArgs,
    engine: Engine,
    history: Option<PathBuf>,
    log: LogWriter,
) -> eyre::Result<()> {
    let mut stdout = std::io::stdout();
    let enrich = !args.no_enrich;
    match args.script {
        Some(path) => {
            let file = std::fs::File::open(&path)
                .map_err(|e| eyre::eyre!("cannot open {}: {e}", path.display()))?;
            run_script(&engine, BufReader::new(file), &mut stdout, enrich).await
        }
        None if std::io::stdin().is_terminal() => {
            interactive::run(&engine, history, &log, enrich).await
        }
        None => run_script(&engine, std::io::stdin().lock(), &mut stdout, enrich).await,
    }
}
