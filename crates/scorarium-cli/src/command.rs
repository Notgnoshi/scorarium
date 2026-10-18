use std::io::Write;

use clap::error::ErrorKind;
use clap::{Parser, Subcommand};
use scorarium_engine::archive::Archive;

use crate::library::{self, Visibility};

#[derive(Debug, Parser)]
#[command(
    multicall = true,
    subcommand_value_name = "command",
    help_template = "{about-section}{usage-heading} {usage}\n\n{all-args}"
)]
pub(crate) struct Line {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Manage libraries
    #[command(
        subcommand_value_name = "command",
        override_usage = "library <library> <command>\n       library list|create ..."
    )]
    Library {
        /// The library name or id
        #[arg(value_name = "library", hide = true)]
        library: Option<String>,
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
    #[command(override_usage = "library <library> show")]
    Show,
    /// Create a library
    Create {
        #[arg(value_name = "name")]
        name: String,
        /// The library's visibility
        #[arg(value_enum, value_name = "visibility", default_value_t = Visibility::Private)]
        visibility: Visibility,
    },
    /// Rename a library
    #[command(override_usage = "library <library> rename <new-name>")]
    Rename {
        #[arg(value_name = "new-name")]
        new_name: String,
    },
    /// Delete a library and everything in it
    #[command(override_usage = "library <library> delete")]
    Delete,
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
        Command::Library { library, command } => match command {
            LibraryCommand::List => {
                reject(&library, "list")?;
                library::list(archive, out).await?
            }
            LibraryCommand::Create { name, visibility } => {
                reject(&library, "create")?;
                library::create(archive, &name, visibility).await?
            }
            LibraryCommand::Show => library::show(archive, require(&library)?, out).await?,
            LibraryCommand::Rename { new_name } => {
                library::rename(archive, require(&library)?, &new_name).await?
            }
            LibraryCommand::Delete => library::delete(archive, require(&library)?).await?,
        },
        Command::Quit => return Ok(Flow::Quit),
    }
    Ok(Flow::Continue)
}

fn require(library: &Option<String>) -> eyre::Result<&str> {
    library
        .as_deref()
        .ok_or_else(|| eyre::eyre!("library name required"))
}

fn reject(library: &Option<String>, verb: &str) -> eyre::Result<()> {
    if library.is_some() {
        eyre::bail!("'{verb}' takes no library");
    }
    Ok(())
}
