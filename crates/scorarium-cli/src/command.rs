use std::io::Write;

use clap::error::ErrorKind;
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use scorarium_engine::Engine;
use scorarium_engine::holding::HoldingId;
use scorarium_engine::library::{Library, LibraryId};
use scorarium_engine::link::HttpUrl;
use scorarium_engine::value::{Stars, Tag, Year};
use scorarium_engine::work::CatalogNumber;

use crate::library::{self, Visibility};
use crate::publication::{HoldingKind, IdentifierKind};
use crate::{audit, person, publication, review, search, tag, work};

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
    /// List the review queue
    Review,
    /// Exit the shell
    #[command(alias = "exit")]
    Quit,
}

#[derive(Debug, Args)]
pub(crate) struct AuditArgs {
    /// How many audit events to print; use 0 to print the full audit log
    #[arg(long, value_name = "n", default_value_t = 10)]
    limit: usize,
}

impl AuditArgs {
    pub(crate) fn limit(&self) -> usize {
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
enum LibrarySet {
    /// The library's name
    Name {
        #[arg(value_name = "name")]
        name: String,
    },
    /// Who can see the library
    Visibility {
        #[arg(value_enum, value_name = "visibility")]
        visibility: Visibility,
    },
}

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
    /// Assign a library field
    #[command(subcommand_value_name = "field")]
    Set {
        #[command(subcommand)]
        field: LibrarySet,
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
pub(crate) enum PublicationCommand {
    /// List the publications in a library
    List {
        /// Keep only publications carrying this tag. Repeatable.
        #[arg(long, value_name = "tag")]
        tag: Vec<String>,
    },
    /// Show a publication
    Show,
    /// Set a publication field
    #[command(subcommand_value_name = "field")]
    Set {
        #[command(subcommand)]
        field: PublicationSet,
    },
    /// Clear an optional publication field
    #[command(subcommand_value_name = "field")]
    Clear {
        #[command(subcommand)]
        field: PublicationClear,
    },
    /// Add a value to a publication
    #[command(subcommand_value_name = "field")]
    Add {
        #[command(subcommand)]
        field: PublicationAdd,
    },
    /// Remove a value from a publication
    #[command(subcommand_value_name = "field")]
    Remove {
        #[command(subcommand)]
        field: PublicationRemove,
    },
    /// Delete a publication and its holdings
    Delete,
    /// Print a publication's audit log
    Audit(AuditArgs),
}

#[derive(Debug, Subcommand)]
pub(crate) enum PublicationSet {
    /// The publication's title
    Title {
        #[arg(value_name = "title")]
        title: String,
    },
    /// The publisher
    Publisher {
        #[arg(value_name = "publisher")]
        publisher: String,
    },
    /// The year of publication
    Year {
        #[arg(value_name = "year")]
        year: Year,
    },
    /// A rating from 1 to 5 stars
    Stars {
        #[arg(value_name = "stars")]
        stars: Stars,
    },
    /// A free-form note
    Note {
        #[arg(value_name = "note")]
        note: String,
    },
    /// Whether the publication is music or prose
    Classification {
        #[arg(value_name = "classification", value_enum)]
        classification: Classification,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum PublicationClear {
    /// The publisher
    Publisher,
    /// The year of publication
    Year,
    /// The star rating
    Stars,
    /// The free-form note
    Note,
}

#[derive(Debug, Subcommand)]
pub(crate) enum PublicationAdd {
    /// A tag on the publication
    Tag {
        #[arg(value_name = "tag")]
        tag: Tag,
    },
    /// An identifier printed on the publication, such as an ISBN
    Identifier {
        #[arg(value_name = "kind", value_enum)]
        kind: IdentifierKind,
        #[arg(value_name = "text")]
        text: String,
    },
    /// A web address about the publication
    Link {
        #[arg(value_name = "url")]
        url: HttpUrl,
    },
    /// A person's contribution to the publication in a role
    Contributor {
        #[arg(value_name = "person")]
        person: String,
        #[arg(value_name = "role")]
        role: String,
    },
    /// A work the publication contains
    Work {
        #[arg(value_name = "work")]
        work: String,
    },
    /// A copy of the publication that the library owns
    Holding {
        #[arg(value_name = "kind", value_enum)]
        kind: HoldingKind,
        /// Where the copy is kept
        #[arg(long, value_name = "location")]
        location: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum PublicationRemove {
    /// A tag on the publication
    Tag {
        #[arg(value_name = "tag")]
        tag: Tag,
    },
    /// An identifier printed on the publication
    Identifier {
        #[arg(value_name = "kind", value_enum)]
        kind: IdentifierKind,
        #[arg(value_name = "text")]
        text: String,
    },
    /// A link, by id or by URL
    Link {
        #[arg(value_name = "url|id")]
        link: String,
    },
    /// A person's contribution, in one role or in every role
    Contributor {
        #[arg(value_name = "person")]
        person: String,
        #[arg(value_name = "role")]
        role: Option<String>,
    },
    /// A work the publication contains
    Work {
        #[arg(value_name = "work")]
        work: String,
    },
    /// A copy of the publication, by id
    Holding {
        #[arg(value_name = "id")]
        holding: HoldingId,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum WorkCommand {
    /// Show a work
    Show,
    /// Set a work field
    #[command(subcommand_value_name = "field")]
    Set {
        #[command(subcommand)]
        field: WorkSet,
    },
    /// Clear an optional work field
    #[command(subcommand_value_name = "field")]
    Clear {
        #[command(subcommand)]
        field: WorkClear,
    },
    /// Add a value to a work
    #[command(subcommand_value_name = "field")]
    Add {
        #[command(subcommand)]
        field: WorkAdd,
    },
    /// Remove a value from a work
    #[command(subcommand_value_name = "field")]
    Remove {
        #[command(subcommand)]
        field: WorkRemove,
    },
    /// Print a work's audit log
    Audit(AuditArgs),
}

#[derive(Debug, Subcommand)]
pub(crate) enum WorkSet {
    /// The work's title
    Title {
        #[arg(value_name = "title")]
        title: String,
    },
    /// The musical key
    Key {
        #[arg(value_name = "key")]
        key: String,
    },
    /// The time signature
    TimeSignature {
        #[arg(value_name = "time-signature")]
        time_signature: String,
    },
    /// The instruments the work is scored for
    Instrumentation {
        #[arg(value_name = "instrumentation")]
        instrumentation: String,
    },
    /// A rating from 1 to 5 stars
    Stars {
        #[arg(value_name = "stars")]
        stars: Stars,
    },
    /// A free-form note
    Note {
        #[arg(value_name = "note")]
        note: String,
    },
    /// Whether the work is music or prose
    Classification {
        #[arg(value_name = "classification", value_enum)]
        classification: Classification,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum WorkClear {
    /// The musical key
    Key,
    /// The time signature
    TimeSignature,
    /// The instruments the work is scored for
    Instrumentation,
    /// The star rating
    Stars,
    /// The free-form note
    Note,
}

#[derive(Debug, Subcommand)]
pub(crate) enum WorkAdd {
    /// A tag on the work
    Tag {
        #[arg(value_name = "tag")]
        tag: Tag,
    },
    /// A catalog number, such as "Op. 29"
    CatalogNumber {
        #[arg(value_name = "number")]
        number: CatalogNumber,
    },
    /// A web address about the work
    Link {
        #[arg(value_name = "url")]
        url: HttpUrl,
    },
    /// A person's contribution to the work in a role
    Contributor {
        #[arg(value_name = "person")]
        person: String,
        #[arg(value_name = "role")]
        role: String,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum WorkRemove {
    /// A tag on the work
    Tag {
        #[arg(value_name = "tag")]
        tag: Tag,
    },
    /// A catalog number
    CatalogNumber {
        #[arg(value_name = "number")]
        number: CatalogNumber,
    },
    /// A link, by id or by URL
    Link {
        #[arg(value_name = "url|id")]
        link: String,
    },
    /// A person's contribution, in one role or in every role
    Contributor {
        #[arg(value_name = "person")]
        person: String,
        #[arg(value_name = "role")]
        role: Option<String>,
    },
}

/// Whether a publication or work is sheet music or prose.
#[derive(Clone, Copy, Debug, clap::ValueEnum)] // dang orphan rule strikes again
pub(crate) enum Classification {
    Music,
    Prose,
}

impl From<Classification> for scorarium_engine::value::Classification {
    fn from(classification: Classification) -> Self {
        match classification {
            Classification::Music => Self::Music,
            Classification::Prose => Self::Prose,
        }
    }
}

#[derive(Debug, Subcommand)]
pub(crate) enum PersonCommand {
    /// List everyone credited in a library
    List {
        /// Keep only persons credited with this role
        #[arg(long, value_name = "role")]
        role: Option<String>,
    },
    /// Show a person
    Show,
    /// Set a person field
    #[command(subcommand_value_name = "field")]
    Set {
        #[command(subcommand)]
        field: PersonSet,
    },
    /// Add a value to a person
    #[command(subcommand_value_name = "field")]
    Add {
        #[command(subcommand)]
        field: PersonAdd,
    },
    /// Remove a value from a person
    #[command(subcommand_value_name = "field")]
    Remove {
        #[command(subcommand)]
        field: PersonRemove,
    },
    /// Print a person's audit log
    Audit(AuditArgs),
}

#[derive(Debug, Subcommand)]
pub(crate) enum PersonSet {
    /// The person's name
    Name {
        #[arg(value_name = "name")]
        name: String,
    },
    /// The name to sort by
    SortName {
        #[arg(value_name = "sort-name")]
        sort_name: String,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum PersonAdd {
    /// A web address about the person
    Link {
        #[arg(value_name = "url")]
        url: HttpUrl,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum PersonRemove {
    /// A link, by id or by URL
    Link {
        #[arg(value_name = "url|id")]
        link: String,
    },
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
                    reject(library.as_deref(), "library", "list")?;
                    library::list(&self.engine, out).await?
                }
                LibraryCommand::Create { name, visibility } => {
                    reject(library.as_deref(), "library", "create")?;
                    library::create(&self.engine, &name, visibility).await?
                }
                LibraryCommand::Show => {
                    library::show(&self.engine, require(library.as_deref(), "library")?, out)
                        .await?
                }
                LibraryCommand::Set { field } => {
                    let reference = require(library.as_deref(), "library")?;
                    match field {
                        LibrarySet::Name { name } => {
                            library::set_name(&self.engine, reference, &name).await?
                        }
                        LibrarySet::Visibility { visibility } => {
                            library::set_visibility(&self.engine, reference, visibility).await?
                        }
                    }
                }
                LibraryCommand::Delete => {
                    library::delete(&self.engine, require(library.as_deref(), "library")?).await?;
                    if self.selected.is_some() && self.selected().await?.is_none() {
                        self.selected = None;
                    }
                }
                LibraryCommand::Select => {
                    self.selected = Some(
                        library::resolve(&self.engine, require(library.as_deref(), "library")?)
                            .await?
                            .id(),
                    );
                }
                LibraryCommand::Deselect => {
                    reject(library.as_deref(), "library", "deselect")?;
                    self.selected = None;
                }
                LibraryCommand::Publication {
                    publication,
                    command,
                } => {
                    let library =
                        library::resolve(&self.engine, require(library.as_deref(), "library")?)
                            .await?;
                    publication::run(&library, publication.as_deref(), command, out).await?
                }
                LibraryCommand::Work { work, command } => {
                    let library =
                        library::resolve(&self.engine, require(library.as_deref(), "library")?)
                            .await?;
                    work::run(&library, work.as_deref(), command, out).await?
                }
                LibraryCommand::Person { person, command } => {
                    let library =
                        library::resolve(&self.engine, require(library.as_deref(), "library")?)
                            .await?;
                    person::run(&library, person.as_deref(), command, out).await?
                }
                LibraryCommand::Tag { command } => {
                    let library =
                        library::resolve(&self.engine, require(library.as_deref(), "library")?)
                            .await?;
                    match command {
                        TagCommand::List => tag::list(&library, out).await?,
                    }
                }
            },
            Command::Search { query } => search::run(&self.engine, &query, out).await?,
            Command::Audit(args) => audit::log(&self.engine, args.limit(), out).await?,
            Command::Review => review::run(&self.engine, out).await?,
            Command::Quit => return Ok(Flow::Quit),
        }
        Ok(Flow::Continue)
    }
}

/// Return the subject, or fail when the command was given none.
pub(crate) fn require<'a>(subject: Option<&'a str>, kind: &str) -> eyre::Result<&'a str> {
    subject.ok_or_else(|| eyre::eyre!("{kind} name required"))
}

/// Fail when the command was given a subject it cannot use.
pub(crate) fn reject(subject: Option<&str>, kind: &str, verb: &str) -> eyre::Result<()> {
    if subject.is_some() {
        eyre::bail!("'{verb}' takes no {kind}");
    }
    Ok(())
}
