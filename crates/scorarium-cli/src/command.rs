use std::io::Write;

use clap::error::ErrorKind;
use clap::{Parser, Subcommand};
use scorarium_archive::Archive;

use crate::library;

#[derive(Debug, Parser)]
#[command(
    multicall = true,
    disable_help_flag = true,
    subcommand_value_name = "command",
    help_template = "{about-section}{usage-heading} {usage}\n\n{all-args}"
)]
struct Line {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Manage libraries
    #[command(subcommand_value_name = "command")]
    Library {
        #[command(subcommand)]
        command: LibraryCommand,
    },
    /// Exit the shell
    #[command(alias = "exit")]
    Quit,
}

#[derive(Debug, Subcommand)]
enum LibraryCommand {
    /// List every library
    List,
}

/// Whether the shell keeps reading after a command.
pub(crate) enum Flow {
    Continue,
    Quit,
}

pub(crate) async fn execute(
    archive: &Archive,
    line: &str,
    out: &mut impl Write,
) -> eyre::Result<Flow> {
    let Some(words) = shlex::split(line) else {
        eyre::bail!("unterminated quote");
    };
    if words.is_empty() {
        return Ok(Flow::Continue);
    }
    let line = match Line::try_parse_from(&words) {
        Ok(line) => line,
        Err(error) if error.kind() == ErrorKind::DisplayHelp => {
            write!(out, "{error}")?;
            return Ok(Flow::Continue);
        }
        Err(error) => return Err(error.into()),
    };
    match line.command {
        Command::Library { command } => match command {
            LibraryCommand::List => library::list(archive, out).await?,
        },
        Command::Quit => return Ok(Flow::Quit),
    }
    Ok(Flow::Continue)
}
