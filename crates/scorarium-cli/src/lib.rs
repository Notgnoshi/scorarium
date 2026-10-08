mod command;
mod library;
mod script;

use std::io::BufReader;
use std::path::PathBuf;

use scorarium_archive::Archive;

pub use crate::script::run_script;

#[derive(Debug, clap::Args)]
pub struct ShellArgs {
    /// A script to run
    pub script: Option<PathBuf>,
}

pub async fn shell(args: ShellArgs, archive: Archive) -> eyre::Result<()> {
    let mut stdout = std::io::stdout();
    match args.script {
        Some(path) => {
            let file = std::fs::File::open(&path)
                .map_err(|e| eyre::eyre!("cannot open {}: {e}", path.display()))?;
            run_script(&archive, BufReader::new(file), &mut stdout).await
        }
        None => run_script(&archive, std::io::stdin().lock(), &mut stdout).await,
    }
}
