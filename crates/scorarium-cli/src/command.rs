use std::io::Write;

use clap::error::ErrorKind;
use clap::{CommandFactory, Parser, Subcommand};
use scorarium_engine::archive::{Archive, Library, Tag, Work};

use crate::library::{self, Visibility};
use crate::{person, publication, reference, work};

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
    /// Select a library for the commands that follow
    #[command(override_usage = "library <library> select")]
    Select,
    /// Clear the selected library
    Deselect,
    /// Manage publications
    #[command(
        subcommand_value_name = "command",
        override_usage = "library <library> publication <publication> <command>\n       library <library> publication list [--tag <tag>]..."
    )]
    Publication {
        /// The publication title or id
        #[arg(value_name = "publication", hide = true)]
        publication: Option<String>,
        #[command(subcommand)]
        command: PublicationCommand,
    },
    /// Manage works
    #[command(
        subcommand_value_name = "command",
        override_usage = "library <library> work <work> <command>"
    )]
    Work {
        /// The work title or id
        #[arg(value_name = "work", hide = true)]
        work: Option<String>,
        #[command(subcommand)]
        command: WorkCommand,
    },
    /// Manage persons
    #[command(
        subcommand_value_name = "command",
        override_usage = "library <library> person <person> <command>\n       library <library> person list [--role <role>]"
    )]
    Person {
        /// The person name or id
        #[arg(value_name = "person", hide = true)]
        person: Option<String>,
        #[command(subcommand)]
        command: PersonCommand,
    },
}

#[derive(Debug, Subcommand)]
enum PublicationCommand {
    /// List the publications in a library
    #[command(override_usage = "library <library> publication list [--tag <tag>]...")]
    List {
        /// Keep only publications carrying this tag. Repeatable.
        #[arg(long, value_name = "tag")]
        tag: Vec<String>,
    },
    /// Show a publication
    #[command(override_usage = "library <library> publication <publication> show")]
    Show,
}

#[derive(Debug, Subcommand)]
enum WorkCommand {
    /// Show a work
    #[command(override_usage = "library <library> work <work> show")]
    Show,
}

#[derive(Debug, Subcommand)]
enum PersonCommand {
    /// List everyone credited in a library
    #[command(override_usage = "library <library> person list [--role <role>]")]
    List {
        /// Keep only persons credited with this role
        #[arg(long, value_name = "role")]
        role: Option<String>,
    },
    /// Show a person
    #[command(override_usage = "library <library> person <person> show")]
    Show,
}

/// Whether the shell keeps reading after a command.
pub(crate) enum Flow {
    Continue,
    Quit,
}

pub(crate) struct Shell<'a> {
    archive: &'a Archive,
    /// The selected library's id.
    ///
    /// The library name is queried before the prompt is displayed to capture any library renames.
    selected: Option<i64>,
    tree: clap::Command,
}

impl Shell<'_> {
    /// Start a session with no library selected.
    pub(crate) fn new(archive: &Archive) -> Shell<'_> {
        let mut tree = Line::command();
        tree.build();
        Shell {
            archive,
            selected: None,
            tree,
        }
    }

    /// Look up the selected library, or nothing when none is selected or it has since been deleted.
    pub(crate) async fn selected(&self) -> eyre::Result<Option<Library>> {
        match self.selected {
            Some(id) => Ok(self.archive.library(id).await?),
            None => Ok(None),
        }
    }

    pub(crate) async fn execute(&mut self, line: &str, out: &mut impl Write) -> eyre::Result<Flow> {
        let Some(mut words) = shlex::split(line) else {
            eyre::bail!("unterminated quote");
        };
        if words.is_empty() {
            return Ok(Flow::Continue);
        }
        if let Some(id) = self.selected
            && self.tree.find_subcommand(&words[0]).is_none()
        {
            if self.archive.library(id).await?.is_none() {
                self.selected = None;
                eyre::bail!("selected library {id} no longer exists; selection cleared");
            }
            // These verbs take no library, so prefixing the selection cannot make them valid.
            if matches!(words[0].as_str(), "list" | "create" | "deselect") {
                eyre::bail!(
                    "'{0}' does not act on the selected library; use 'library {0}'",
                    words[0]
                );
            }
            words.splice(0..0, ["library".to_string(), id.to_string()]);
        }
        let line = match Line::try_parse_from(&words) {
            Ok(line) => line,
            Err(error) if error.kind() == ErrorKind::DisplayHelp => {
                write!(out, "{error}")?;
                return Ok(Flow::Continue);
            }
            Err(error)
                if error.kind() == ErrorKind::InvalidSubcommand
                    && self.selected.is_none()
                    && self.tree.find_subcommand(&words[0]).is_none() =>
            {
                eyre::bail!(
                    "{}\nselect a library to run its commands directly",
                    error.to_string().trim_end()
                );
            }
            Err(error) => return Err(error.into()),
        };
        match line.command {
            Command::Library { library, command } => match command {
                LibraryCommand::List => {
                    reject(&library, "library", "list")?;
                    library::list(self.archive, out).await?
                }
                LibraryCommand::Create { name, visibility } => {
                    reject(&library, "library", "create")?;
                    library::create(self.archive, &name, visibility).await?
                }
                LibraryCommand::Show => {
                    library::show(self.archive, require(&library, "library")?, out).await?
                }
                LibraryCommand::Rename { new_name } => {
                    library::rename(self.archive, require(&library, "library")?, &new_name).await?
                }
                LibraryCommand::Delete => {
                    library::delete(self.archive, require(&library, "library")?).await?;
                    if self.selected.is_some() && self.selected().await?.is_none() {
                        self.selected = None;
                    }
                }
                LibraryCommand::Select => {
                    self.selected = Some(
                        library::resolve(self.archive, require(&library, "library")?)
                            .await?
                            .id,
                    );
                }
                LibraryCommand::Deselect => {
                    reject(&library, "library", "deselect")?;
                    self.selected = None;
                }
                LibraryCommand::Publication {
                    publication,
                    command,
                } => {
                    let library =
                        library::resolve(self.archive, require(&library, "library")?).await?;
                    match command {
                        PublicationCommand::List { tag } => {
                            reject(&publication, "publication", "list")?;
                            let tags = tag
                                .iter()
                                .map(|tag| tag.parse::<Tag>().map_err(|e| eyre::eyre!("{e}")))
                                .collect::<eyre::Result<Vec<_>>>()?;
                            publication::list(&library, &tags, out).await?
                        }
                        PublicationCommand::Show => {
                            let reference = require(&publication, "publication")?;
                            let publication = reference::resolve(
                                "publication",
                                reference,
                                library.publications().await?,
                                |p| p.id,
                                |p| &p.title,
                            )?;
                            publication::show(publication, out).await?
                        }
                    }
                }
                LibraryCommand::Work { work, command } => {
                    let library =
                        library::resolve(self.archive, require(&library, "library")?).await?;
                    match command {
                        WorkCommand::Show => {
                            let reference = require(&work, "work")?;
                            let mut works = Vec::new();
                            for publication in library.publications().await? {
                                for candidate in publication.works().await? {
                                    if !works.iter().any(|w: &Work| w.id == candidate.id) {
                                        works.push(candidate);
                                    }
                                }
                            }
                            let work = reference::resolve(
                                "work",
                                reference,
                                works,
                                |w| w.id,
                                |w| &w.title,
                            )?;
                            work::show(work, out).await?
                        }
                    }
                }
                LibraryCommand::Person { person, command } => {
                    let library =
                        library::resolve(self.archive, require(&library, "library")?).await?;
                    match command {
                        PersonCommand::List { role } => {
                            reject(&person, "person", "list")?;
                            person::list(&library, role.as_deref(), out).await?
                        }
                        PersonCommand::Show => {
                            let reference = require(&person, "person")?;
                            let summary = reference::resolve(
                                "person",
                                reference,
                                library.person_summaries(None).await?,
                                |p| p.id,
                                |p| &p.name,
                            )?;
                            let Some(person) = library.person(summary.id).await? else {
                                eyre::bail!("no person '{reference}'");
                            };
                            person::show(person, out).await?
                        }
                    }
                }
            },
            Command::Quit => return Ok(Flow::Quit),
        }
        Ok(Flow::Continue)
    }
}

/// Return the subject, or fail when the command was given none.
fn require<'a>(subject: &'a Option<String>, kind: &str) -> eyre::Result<&'a str> {
    subject
        .as_deref()
        .ok_or_else(|| eyre::eyre!("{kind} name required"))
}

/// Fail when the command was given a subject it cannot use.
fn reject(subject: &Option<String>, kind: &str, verb: &str) -> eyre::Result<()> {
    if subject.is_some() {
        eyre::bail!("'{verb}' takes no {kind}");
    }
    Ok(())
}
