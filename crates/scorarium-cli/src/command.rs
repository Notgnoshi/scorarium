use std::io::Write;

use clap::error::ErrorKind;
use clap::{Parser, Subcommand};
use scorarium_archive::Archive;

use crate::library::{self, Visibility};

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
    /// Show the contents of a library
    Show {
        /// The library name or id
        #[arg(value_name = "library")]
        library: String,
    },
    /// Create a library
    Create {
        #[arg(value_name = "name")]
        name: String,
        /// The library's visibility
        #[arg(value_enum, value_name = "visibility", default_value_t = Visibility::Private)]
        visibility: Visibility,
    },
    /// Rename a library
    Rename {
        /// The library name or id
        #[arg(value_name = "library")]
        library: String,
        #[arg(value_name = "new-name")]
        new_name: String,
    },
    /// Delete a library and everything in it
    Delete {
        /// The library name or id
        #[arg(value_name = "library")]
        library: String,
    },
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
            LibraryCommand::Show { library } => library::show(archive, &library, out).await?,
            LibraryCommand::Create { name, visibility } => {
                library::create(archive, &name, visibility).await?
            }
            LibraryCommand::Rename { library, new_name } => {
                library::rename(archive, &library, &new_name).await?
            }
            LibraryCommand::Delete { library } => library::delete(archive, &library).await?,
        },
        Command::Quit => return Ok(Flow::Quit),
    }
    Ok(Flow::Continue)
}
