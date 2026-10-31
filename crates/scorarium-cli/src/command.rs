use std::io::Write;

use clap::error::ErrorKind;
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use scorarium_engine::Engine;
use scorarium_engine::entity::Entity;
use scorarium_engine::library::{Library, LibraryId};
use scorarium_engine::value::Tag;

use crate::library::{self, Visibility};
use crate::{audit, person, publication, reference, search, tag, work};

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
    #[command(subcommand_value_name = "command")]
    Library {
        /// The library name or id
        #[arg(value_name = "library", hide = true)]
        library: Option<String>,
        #[command(subcommand)]
        command: LibraryCommand,
    },
    /// Search every library
    Search {
        /// The words to search for; quote more than one
        #[arg(value_name = "query")]
        query: String,
    },
    /// Print the global audit log of every library
    Audit(AuditArgs),
    /// Exit the shell
    #[command(alias = "exit")]
    Quit,
}

#[derive(Debug, Args)]
struct AuditArgs {
    /// How many audit events to print; use 0 to print the full audit log
    #[arg(long, value_name = "n", default_value_t = 10)]
    limit: usize,
}

impl AuditArgs {
    fn limit(&self) -> usize {
        if self.limit == 0 {
            usize::MAX
        } else {
            self.limit
        }
    }
}

/// The verbs that take no subject under any noun.
pub(crate) const VERBS_WITHOUT_SUBJECT: [&str; 3] = ["list", "create", "deselect"];

#[derive(Debug, Subcommand)]
enum LibraryCommand {
    /// List every library
    List,
    /// Show the contents of a library
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
    Rename {
        #[arg(value_name = "new-name")]
        new_name: String,
    },
    /// Delete a library and everything in it
    Delete,
    /// Select a library for the commands that follow
    Select,
    /// Clear the selected library
    Deselect,
    /// Manage publications
    #[command(subcommand_value_name = "command")]
    Publication {
        /// The publication title or id
        #[arg(value_name = "publication", hide = true)]
        publication: Option<String>,
        #[command(subcommand)]
        command: PublicationCommand,
    },
    /// Manage works
    #[command(subcommand_value_name = "command")]
    Work {
        /// The work title or id
        #[arg(value_name = "work", hide = true)]
        work: Option<String>,
        #[command(subcommand)]
        command: WorkCommand,
    },
    /// Manage persons
    #[command(subcommand_value_name = "command")]
    Person {
        /// The person name or id
        #[arg(value_name = "person", hide = true)]
        person: Option<String>,
        #[command(subcommand)]
        command: PersonCommand,
    },
    /// Manage tags
    #[command(subcommand_value_name = "command")]
    Tag {
        #[command(subcommand)]
        command: TagCommand,
    },
}

#[derive(Debug, Subcommand)]
enum PublicationCommand {
    /// List the publications in a library
    List {
        /// Keep only publications carrying this tag. Repeatable.
        #[arg(long, value_name = "tag")]
        tag: Vec<String>,
    },
    /// Show a publication
    Show,
    /// Print a publication's audit log
    Audit(AuditArgs),
}

#[derive(Debug, Subcommand)]
enum WorkCommand {
    /// Show a work
    Show,
    /// Print a work's audit log
    Audit(AuditArgs),
}

#[derive(Debug, Subcommand)]
enum PersonCommand {
    /// List everyone credited in a library
    List {
        /// Keep only persons credited with this role
        #[arg(long, value_name = "role")]
        role: Option<String>,
    },
    /// Show a person
    Show,
    /// Print a person's audit log
    Audit(AuditArgs),
}

#[derive(Debug, Subcommand)]
enum TagCommand {
    /// List the tags in a library with their use counts
    List,
}

/// Build the command tree with every usage line computed from its path.
///
/// Clap omits a parent's positionals from a subcommand's usage, so the subject placeholders are
/// inserted here rather than written by hand on each command.
pub(crate) fn tree() -> clap::Command {
    let mut tree = Line::command();
    tree.build();
    for command in tree.get_subcommands_mut() {
        usage(command, "");
    }
    tree
}

fn usage(command: &mut clap::Command, prefix: &str) {
    if command.get_name() == "help" {
        return;
    }
    let subject = command
        .has_subcommands()
        .then(|| command.get_positionals().next())
        .flatten()
        .map(|positional| {
            positional
                .get_value_names()
                .and_then(|names| names.first())
                .map_or_else(|| positional.get_id().to_string(), |name| name.to_string())
        });
    let verbs: Vec<String> = if subject.is_some() {
        command
            .get_subcommands()
            .map(clap::Command::get_name)
            .filter(|name| VERBS_WITHOUT_SUBJECT.contains(name))
            .map(str::to_string)
            .collect()
    } else {
        Vec::new()
    };

    let mut path = format!("{prefix}{}", command.get_name());
    if let Some(subject) = &subject {
        path = format!("{path} <{subject}>");
    }
    let own_prefix = format!("{path} ");
    let subjectless_prefix = format!("{prefix}{} ", command.get_name());
    let bin_name = command.get_bin_name().unwrap_or_default().to_string();
    let rendered = command.render_usage().to_string();
    let arguments = rendered
        .trim_start()
        .trim_start_matches("Usage:")
        .trim_start()
        .trim_start_matches(bin_name.as_str())
        .trim();
    let mut line = format!("{path} {arguments}").trim().to_string();
    if !verbs.is_empty() {
        line.push_str(&format!(
            "\n       {prefix}{} {} ...",
            command.get_name(),
            verbs.join("|")
        ));
    }

    for sub in command.get_subcommands_mut() {
        if subject.is_some() && VERBS_WITHOUT_SUBJECT.contains(&sub.get_name()) {
            usage(sub, &subjectless_prefix);
        } else {
            usage(sub, &own_prefix);
        }
    }
    *command = std::mem::take(command).override_usage(line);
}

/// Whether the shell keeps reading after a command.
pub(crate) enum Flow {
    Continue,
    Quit,
}

pub(crate) struct Shell {
    engine: Engine,
    /// The selected library's id.
    ///
    /// The library name is queried before the prompt is displayed to capture any library renames.
    selected: Option<LibraryId>,
    tree: clap::Command,
}

impl Shell {
    /// Start a session with no library selected.
    pub(crate) fn new(engine: Engine) -> Shell {
        Shell {
            engine,
            selected: None,
            tree: tree(),
        }
    }

    /// Return the id of the selected library, if any.
    pub(crate) fn selection(&self) -> Option<LibraryId> {
        self.selected
    }

    /// Look up the selected library, or nothing when none is selected or it has since been deleted.
    pub(crate) async fn selected(&self) -> eyre::Result<Option<Library>> {
        match self.selected {
            Some(id) => Ok(self.engine.library(id).await?),
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
            if self.engine.library(id).await?.is_none() {
                self.selected = None;
                eyre::bail!("selected library {id} no longer exists; selection cleared");
            }
            // These verbs take no library, so prefixing the selection cannot make them valid.
            if VERBS_WITHOUT_SUBJECT.contains(&words[0].as_str()) {
                eyre::bail!(
                    "'{0}' does not act on the selected library; use 'library {0}'",
                    words[0]
                );
            }
            words.splice(0..0, ["library".to_string(), id.to_string()]);
        }
        let line = match self
            .tree
            .try_get_matches_from_mut(&words)
            .and_then(|matches| Line::from_arg_matches(&matches))
        {
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
                    library::list(&self.engine, out).await?
                }
                LibraryCommand::Create { name, visibility } => {
                    reject(&library, "library", "create")?;
                    library::create(&self.engine, &name, visibility).await?
                }
                LibraryCommand::Show => {
                    library::show(&self.engine, require(&library, "library")?, out).await?
                }
                LibraryCommand::Rename { new_name } => {
                    library::rename(&self.engine, require(&library, "library")?, &new_name).await?
                }
                LibraryCommand::Delete => {
                    library::delete(&self.engine, require(&library, "library")?).await?;
                    if self.selected.is_some() && self.selected().await?.is_none() {
                        self.selected = None;
                    }
                }
                LibraryCommand::Select => {
                    self.selected = Some(
                        library::resolve(&self.engine, require(&library, "library")?)
                            .await?
                            .id(),
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
                        library::resolve(&self.engine, require(&library, "library")?).await?;
                    match command {
                        PublicationCommand::List { tag } => {
                            reject(&publication, "publication", "list")?;
                            let tags = tag
                                .iter()
                                .map(|tag| tag.parse())
                                .collect::<Result<Vec<Tag>, _>>()?;
                            publication::list(&library, &tags, out).await?
                        }
                        PublicationCommand::Show | PublicationCommand::Audit(_) => {
                            let reference = require(&publication, "publication")?;
                            let publication = reference::resolve(
                                "publication",
                                reference,
                                library.publications().await?,
                                |p| p.id(),
                                |p| p.title().unwrap_or_default(),
                            )?;
                            if let PublicationCommand::Audit(args) = command {
                                audit::entity_log(&publication, args.limit(), out).await?
                            } else {
                                publication::show(publication, out).await?
                            }
                        }
                    }
                }
                LibraryCommand::Work { work, command } => {
                    let library =
                        library::resolve(&self.engine, require(&library, "library")?).await?;
                    match command {
                        WorkCommand::Show | WorkCommand::Audit(_) => {
                            let reference = require(&work, "work")?;
                            let work = reference::resolve(
                                "work",
                                reference,
                                library.works().await?,
                                |w| w.id(),
                                |w| w.title(),
                            )?;
                            if let WorkCommand::Audit(args) = command {
                                audit::entity_log(&work, args.limit(), out).await?
                            } else {
                                work::show(work, out).await?
                            }
                        }
                    }
                }
                LibraryCommand::Person { person, command } => {
                    let library =
                        library::resolve(&self.engine, require(&library, "library")?).await?;
                    match command {
                        PersonCommand::List { role } => {
                            reject(&person, "person", "list")?;
                            person::list(&library, role.as_deref(), out).await?
                        }
                        PersonCommand::Show | PersonCommand::Audit(_) => {
                            let reference = require(&person, "person")?;
                            let person = reference::resolve(
                                "person",
                                reference,
                                library.people(None).await?,
                                |p| p.id(),
                                |p| p.name(),
                            )?;
                            if let PersonCommand::Audit(args) = command {
                                audit::entity_log(&person, args.limit(), out).await?
                            } else {
                                person::show(person, out).await?
                            }
                        }
                    }
                }
                LibraryCommand::Tag { command } => {
                    let library =
                        library::resolve(&self.engine, require(&library, "library")?).await?;
                    match command {
                        TagCommand::List => tag::list(&library, out).await?,
                    }
                }
            },
            Command::Search { query } => search::run(&self.engine, &query, out).await?,
            Command::Audit(args) => audit::log(&self.engine, args.limit(), out).await?,
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
