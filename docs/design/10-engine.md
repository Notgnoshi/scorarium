# scorarium-engine

The web server and `scorarium-archive` are too tightly coupled. Additionally, as indicated by the
[move-drafts-to-database](./09-move-drafts-to-database.md) design document, I've found that having a
parallel in-memory implementation of the exact same data model really complicated the
implementation, and added too much friction.

`scorarium-engine` will be a fresh attempt at redesigning `scorarium-archive` from scratch with a
few changes to the data model:

* draft entities
* enrichment proposals
* digital assets (this will require extending the `05-data-model.md` design doc)

## Principles

* `scorarium-engine` should work equally well for the web server and CLI tool
* `scorarium-engine` should operate on strong types that perform input validation via fallible
  constructors (e.g., `TryFrom` and `FromStr`)
* `scorarium-engine` is expected to completely replace `scorarium-archive`. While it's reasonable to
  use the archive as inspiration, the engine exists because of pain points in the archive design.
* Use the same type for both drafts and committed entities. One of the pain points in the archive
  was the proliferation of 4-5 types with the same name
* Entity types are live views of the database that allows editing the underlying entities
* Edits are per-field, each with an entry in the audit log. The audit log is not grouped in its
  storage, but the UI layer can group the audit log based on the entities edited.
* Enrichment never directly edits an entity. It makes proposals for the user's review. The exception
  is when the user selects an external result in the suggestion typeahead.
* The engine consumer never wants control over which specific external API source is used. The
  consumer only cares about enriching an entity, however possible.
* The "publication review" page will get removed. There are only the edit pages for an entity,
  whether or not that entity is a draft. The review queue will show all draft entities, as well as
  any entities with pending enrichment proposals.
* The engine will own the background enrichment tasks. The consumers never have to orchestrate this.
  But it should be done in a way that allows consumers to update their UI as external proposals come
  back.

## Transition

The current re-export facade will remain in the engine crate. It will move to
`scorarium_engine::legacy` to keep it isolated. There will be a number of APIs and functionalities
in the engine crate that aren't strictly SQL data model. I will use the `scorarium-archive`
implementations as inspiration, but I will take the opportunity to improve the type system and make
them feel like a more consistent API.

The CLI will migrate to the new engine as soon as possible.

## Crate layout

The root exports `Engine`, `EngineOptions`, `Error`, and `Result`, and nothing else.

```
scorarium_engine
  Engine, EngineOptions, Error, Result
  auth         PasswordState, Token
  audit        AuditEvent
  entity       AnyEntity, Entity, EntityKind, Timestamp, Proposal, ProposalId, ProposedValue
  library      Library, LibraryId, ReviewItem
  publication  Publication, PublicationId, Field, Identifier, IdentifierKind, Normalized,
               Holding, HoldingId, HoldingKind, ImportSeed
  work         Work, WorkId, Field, CatalogNumber
  person       Person, PersonId, Field
  contributor  Contributor
  link         Link, LinkId, LinkKind, Source
  value        Year, Stars, Tag, Classification
  search       SearchHit
  suggest      SuggestField, Suggested, Suggestion, ExternalCandidate
  enrich       Enrichment
  asset        AssetSource, NewHolding
  legacy       // the temporary archive facade
```

```rust
pub struct Engine(Arc<EngineInner>);
enum DataDir {
    Persistent(PathBuf),
    // A TempDir allows for digital asset management even for demo instances, as well as bypassing
    // sqlite's connection limit for in-memory databases.
    Temporary(tempfile::TempDir),
}
struct EngineInner {
    data_dir: DataDir,
    api_client: Client,
    pool: SqlitePool,
    tasks: (), // TODO
}

// TODO: Constructor takes in UserAgent for Client?
pub struct EngineOptions {
    migrate_on_startup: bool,
    // allows tests to dependency inject the replay client
    api_client: Client
}

impl Engine {
    pub async fn open<P>(data_dir: P, options: EngineOptions) -> Result<Engine>;
    pub async fn demo(options: EngineOptions) -> Result<Engine>;
}
```

### Errors

The engine returns its own error enum rather than an `eyre::Report`, because there are some errors
the engine consumers need to act on (form errors, 404 responses, etc).

```rust
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not found")]
    NotFound,
    /// A database write that would have broken a data model rule
    #[error("{0}")]
    Invariant(String),
    #[error("wrong password")]
    WrongPassword,
    #[error("too many login attempts; retry in {retry_after:?}")]
    Throttled { retry_after: Duration },

    /// Any error the consumer doesn't need to distinguish
    #[error(transparent)]
    Generic(#[from] eyre::Report),
}
```

Strong type validation errors will belong to the types' constructors with a different error type.

## Administrivia

```rust
pub enum PasswordState {
    Unclaimed,
    Claimed,
}

pub struct Token([u8; N]);

impl Engine {
    pub async fn password_state(&self) -> Result<PasswordState>;
    pub async fn login<S>(&self, password: S) -> Result<Token>;
    pub fn is_authenticated(&self, token: &Token) -> bool;
    pub fn logout(&self, token: &Token);
    /// Assumes the consumer has checked Engine::is_authenticated()
    pub async fn change_password<S, T>(&self, current: S, new: T) -> Result<()>;
}
```

there's also a paginated audit log.

```rust
impl Engine {
    pub async fn audit_log(&self, before: Option<AuditId>, limit: usize) -> Result<Vec<AuditEvent>>;
}

pub struct AuditEvent {
    pub id: AuditId,
    pub created_at: Timestamp,
    pub source: AuditSource, // User, OrphanCleanup, Merge, LinkRecognition
    pub action: Action,      // Created, Updated, Deleted, Merged, Committed, PasswordClaimed, PasswordChanged
    pub entity: Option<AuditSubject>,
    /// The entity's `Field` as text
    pub field: Option<String>,
}

pub struct AuditSubject {
    pub kind: EntityKind,
    pub id: i64,
    pub library: LibraryId,
    pub label: String,
}
```

there is also `Entity::audit_log()` to get the audit log of a particular entity.

## Entities

`Publication`, `Work`, and `Person` are instances of `Entity`

```rust
pub struct LibraryId(i64);

pub trait Entity: Sized {
    /// A strong type ID for this entity
    type Id: Copy + Eq + Hash + Display + FromStr;
    /// An enum over the fields of this entity
    type Field: Copy + Eq + Display + FromStr;

    async fn load(engine: &Engine, id: Self::Id) -> Result<Option<Self>>;
    async fn reload(&mut self) -> Result<()>;

    fn id(&self) -> Self::Id;
    fn library(&self) -> LibraryId;
    fn is_draft(&self) -> bool;
    fn created_at(&self) -> Timestamp;
    fn last_modified_at(&self) -> Timestamp;

    async fn audit_log(&self, before: Option<AuditId>, limit: usize) -> Result<Vec<AuditEvent>>;
    // Commit the draft. Has no effect if not a draft.
    async fn commit(&mut self) -> Result<()>;

    async fn enrich(&self) -> Result<Enrichment>;
    fn enrichment(&self) -> Option<Enrichment>;

    async fn proposals(&self) -> Result<Vec<Proposal<Self>>>;
    async fn accept(&mut self, proposal: &Proposal<Self>) -> Result<()>;
    async fn reject(&mut self, proposal: &Proposal<Self>) -> Result<()>;
    async fn reject_all(&mut self) -> Result<()>;
}

pub struct Proposal<E: Entity> {
    pub id: ProposalId,
    pub field: E::Field,
    pub source: Source,
    pub value: ProposedValue,
}
```

## Library

```rust
impl Engine {
    pub async fn libraries(&self) -> Result<Vec<Library>>;
    pub async fn library(&self, id: LibraryId) -> Result<Option<Library>>;
    pub async fn create_library<S>(&self, name: S, private: bool) -> Result<Library>;
}

impl Library {
    pub fn id(&self) -> LibraryId;
    pub fn name(&self) -> &str;
    pub fn is_private(&self) -> bool;

    pub async fn rename<S>(&mut self, name: S) -> Result<()>;
    pub async fn set_private(&mut self, private: bool) -> Result<()>;
    pub async fn delete(self) -> Result<()>; // cascades

    pub async fn publications(&self) -> Result<Vec<Publication>>;
    pub async fn works(&self) -> Result<Vec<Work>>;
    pub async fn people(&self, role: Option<&str>) -> Result<Vec<Person>>;
    pub async fn tags(&self) -> Result<Vec<(Tag, usize)>>;
    pub async fn tagged_with(&self, tag: &Tag) -> Result<(Vec<Publication>, Vec<Work>)>;

    pub async fn import(&self, seed: ImportSeed, holding: NewHolding<'_>) -> Result<Publication>;
}
```

## Read APIs

Each entity is loaded with all of its non-entity children, so reading never touches the database.

```rust
impl Publication {
    pub fn title(&self) -> Option<&str>; // None only possible while a draft
    pub fn publisher(&self) -> Option<&str>;
    pub fn year(&self) -> Option<Year>;
    pub fn stars(&self) -> Option<Stars>;
    pub fn note(&self) -> Option<&str>;
    pub fn tags(&self) -> &[Tag]; // alphabetical
    pub fn identifiers(&self) -> &[Identifier];
    pub fn contributors(&self) -> &[Contributor];
    pub fn holdings(&self) -> &[Holding];
    pub fn links(&self) -> &[Link];
    /// Absolute path, resolved against the data directory.
    pub fn cover(&self) -> Option<&Path>;
    /// None until enrichment's heuristics or the user set it.
    pub fn classification(&self) -> Option<Classification>;

    pub async fn works(&self) -> Result<Vec<Work>>;
}

impl Work {
    pub fn title(&self) -> &str;
    pub fn key(&self) -> Option<&str>;
    pub fn time_signature(&self) -> Option<&str>;
    pub fn instrumentation(&self) -> Option<&str>;
    pub fn stars(&self) -> Option<Stars>;
    pub fn note(&self) -> Option<&str>;
    pub fn tags(&self) -> &[Tag];
    pub fn catalog_numbers(&self) -> &[CatalogNumber]; // sorted by parsed scheme and number
    pub fn contributors(&self) -> &[Contributor];
    pub fn links(&self) -> &[Link];
    pub fn classification(&self) -> Option<Classification>;

    pub async fn publications(&self) -> Result<Vec<Publication>>;
}

impl Person {
    pub fn name(&self) -> &str;
    pub fn sort_name(&self) -> &str;
    pub fn links(&self) -> &[Link];
    pub fn profile(&self) -> Option<&Path>;

    pub async fn publications(&self) -> Result<Vec<Publication>>;
    pub async fn works(&self) -> Result<Vec<Work>>;
}

pub struct Identifier {
    pub kind: IdentifierKind, // Isbn, Ismn, PublisherNumber, PlateNumber
    pub value: Normalized,    // the canonical form; Display gives the hyphenated text
}

pub struct Holding {
    pub id: HoldingId,
    pub kind: HoldingKind, // holds the location
    pub loaned_to: Option<String>,
}

pub enum HoldingKind {
    Physical { location: Option<String> },
    // stored in the database relative to the data dir; converted to absolute when hydrated
    Digital { file: PathBuf },
}

pub struct Contributor {
    pub person: PersonId,
    pub name: String, // denormalized as a convenience at load so listings don't need a second query
    pub role: String,
}

pub enum Classification {
    Music,
    Prose,
}

pub struct Link {
    pub id: LinkId,
    pub url: Url,
    pub kind: LinkKind, // the recognized site, or Generic
    pub external_id: Option<String>,
}
```

## Edit APIs

Every edit is one method, one transaction, one audit entry naming the entity and the `Field` it
changed, and one `last_modified_at` bump. The method writes the database and then updates the value
it was called on, so the caller's view stays current without a reload.

```rust
impl Publication {
    pub async fn set_title<S>(&mut self, title: S) -> Result<()>;
    pub async fn set_publisher(&mut self, publisher: Option<&str>) -> Result<()>;
    pub async fn set_year(&mut self, year: Option<Year>) -> Result<()>;
    pub async fn set_stars(&mut self, stars: Option<Stars>) -> Result<()>;
    pub async fn set_note(&mut self, note: Option<&str>) -> Result<()>;
    pub async fn set_classification(&mut self, classification: Classification) -> Result<()>;

    pub async fn add_tag(&mut self, tag: Tag) -> Result<()>;
    pub async fn remove_tag(&mut self, tag: &Tag) -> Result<()>;
    pub async fn add_identifier(&mut self, identifier: Identifier) -> Result<()>;
    pub async fn remove_identifier(&mut self, identifier: &Identifier) -> Result<()>;
    pub async fn add_link(&mut self, url: Url) -> Result<()>;
    pub async fn remove_link(&mut self, link: LinkId) -> Result<()>;

    pub async fn add_contributor<S>(&mut self, person: PersonId, role: S) -> Result<()>;
    pub async fn create_contributor<S, T>(&mut self, name: S, role: T) -> Result<Person>;
    pub async fn remove_contributor<S>(&mut self, person: PersonId, role: S) -> Result<()>;

    pub async fn add_work(&mut self, work: WorkId) -> Result<()>;
    pub async fn create_work<S>(&mut self, title: S) -> Result<Work>;
    pub async fn remove_work(&mut self, work: WorkId) -> Result<()>;

    pub async fn add_holding(&mut self, holding: NewHolding<'_>) -> Result<HoldingId>;
    pub async fn remove_holding(&mut self, holding: HoldingId) -> Result<()>; // Err when deleting the last holding
    pub async fn replace_holding_file(&mut self, holding: HoldingId, source: AssetSource<'_>) -> Result<()>;
    pub async fn set_holding_loaned_to(&mut self, holding: HoldingId, loaned_to: Option<&str>) -> Result<()>;

    // Must decode as an image, or `Error::Invariant`. Replacing deletes the old file.
    pub async fn set_cover(&mut self, source: AssetSource<'_>) -> Result<()>;
    pub async fn clear_cover(&mut self) -> Result<()>;

    pub async fn delete(self) -> Result<()>;
}

impl Work {
    pub async fn set_title<S>(&mut self, title: S) -> Result<()>;
    pub async fn set_key(&mut self, key: Option<&str>) -> Result<()>;
    pub async fn set_time_signature(&mut self, time_signature: Option<&str>) -> Result<()>;
    pub async fn set_instrumentation(&mut self, instrumentation: Option<&str>) -> Result<()>;
    pub async fn set_stars(&mut self, stars: Option<Stars>) -> Result<()>;
    pub async fn set_note(&mut self, note: Option<&str>) -> Result<()>;
    pub async fn set_classification(&mut self, classification: Classification) -> Result<()>;

    pub async fn add_tag(&mut self, tag: Tag) -> Result<()>;
    pub async fn remove_tag(&mut self, tag: &Tag) -> Result<()>;
    pub async fn add_catalog_number(&mut self, number: CatalogNumber) -> Result<()>;
    pub async fn remove_catalog_number(&mut self, number: &CatalogNumber) -> Result<()>;
    pub async fn add_link(&mut self, url: Url) -> Result<()>;
    pub async fn remove_link(&mut self, link: LinkId) -> Result<()>;

    pub async fn add_contributor<S>(&mut self, person: PersonId, role: S) -> Result<()>;
    pub async fn create_contributor<S, T>(&mut self, name: S, role: T) -> Result<Person>;
    pub async fn remove_contributor<S>(&mut self, person: PersonId, role: S) -> Result<()>;
}

impl Person {
    pub async fn rename<S, T>(&mut self, name: S, sort_name: T) -> Result<()>;
    pub async fn add_link(&mut self, url: Url) -> Result<()>;
    pub async fn remove_link(&mut self, link: LinkId) -> Result<()>;
    pub async fn set_profile(&mut self, source: AssetSource<'_>) -> Result<()>;
    pub async fn clear_profile(&mut self) -> Result<()>;
}
```

## Import

When a user imports a new publication, there's two ways they can do so:

1. By entering a title
2. By entering an ISBN

For the title search, scorarium will provide title suggestions against external APIs, and if the
user selects one, the corresponding API data will pre-seed the generated draft. The ISBN API lookup
is too long to make synchronous though, so if a user enters an ISBN, a draft is created for that
ISBN, but the API results from the initial ISBN lookup are treated as background enrichment (and
thus become proposals rather than pre-seeding the draft). This implies that the `title IS NOT NULL`
constraint is lifted for drafts.

An import is the creation of a draft publication together with its first holding.

```rust
pub enum ImportSeed {
    /// A manually entered title with no external candidates
    Title(String),
    /// The user entered a title, and selected a candidate from the external title search
    Candidate(ExternalCandidate),
    /// A complete, valid ISBN or ISMN
    Identifier(Identifier),
}

impl Library {
    // Automatically triggers enrichment on the imported publication.
    pub async fn import(&self, seed: ImportSeed, holding: NewHolding<'_>) -> Result<Publication>;
}
```

The import query's typeahead suggestions will use the `Library::suggest()` API to query the local
library for potential duplicate publications (and prompt to add a holding rather than add a
duplicate publication).

It will use `Engine::external_suggest()` to do an external title search to pre-seed the import
draft.

## Review queue

The review queue provides the user visibility into entities waiting on user feedback and revision.
It exists so the user can enter titles / ISBNs in bulk, and Scorarium can queue background
enrichment jobs to run at a polite rate-limited pace.

An `Entity` is added to the review queue if:

* It is a draft of any kind (publication, work, or person). Committing a draft entity commits any
  referenced entities.
* It has waiting proposals to apply or reject
* It has an enrichment run that is in-progress or has failed

```rust
pub struct ReviewItem {
    pub entity: AnyEntity,
    /// The timestamp the item was added to the review queue
    pub queued_at: Timestamp,
    pub status: EnrichmentStatus, // TODO: maybe needs to be an Option for a review queue constructed after fresh server startup?
}

pub enum AnyEntity {
    Publication(Publication),
    Work(Work),
    Person(Person),
}

impl Engine {
    // Ordered by time added to the queue
    pub async fn review_queue(&self) -> Result<Vec<ReviewItem>>;
    pub async fn review_count(&self) -> Result<usize>;
}
```

For each entry, it is possible to learn:

* What entity is it (name, title, type, etc)
* Is it a draft or committed?
* What's its enrichment state (in-progress, completed, or failed)
  * It should be possible to see the number of requests performed, pending, and failed
  * It should be possible to see the number of field proposals
  * It does not need to be possible to see the proposals themselves
* How long the entry has been in the queue

Note that every entity in the review queue will have enrichment results, because enrichment happens
automatically during import.

For each entry, it's possible to take the following actions:

* Open the entity for review and edit
* Cancel an in-progress enrichment task
* Retry a failed enrichment task
* Discard the entry. Discarding a draft deletes the draft. Discarding a committed entity with
  pending proposals discards the proposals and cancels any pending enrichment tasks.

An entry is removed from the queue when:

* It is explicitly discarded
* All of the three reasons to add the entity to the queue do not apply.

Committing a draft entity with pending proposals discards the proposals.

TODO: I am uncertain if committing a draft entity leaves any pending enrichment tasks active, which
will likely result in the entity being added back into the queue once it's complete.

TODO: Supposed a server with pending drafts and/or proposals is restarted. The database has the
drafts and proposals, but the enrichment tasks and statuses disappear. What do we do? And how do we
handle the review queue?

## Enrichment detail

### Classification

A publication or work is sheet music or prose, and the run uses that to pick its enrichment sources.

Classification happens inside the run, as its first fact-dependent step: when the primary source's
record arrives, heuristics read it and set the value if it was null, and the run decides its
remaining steps from the result.

The run never overwrites a value once set, so a wrong guess stays until the user corrects it with
`set_classification`.

### Status

```rust
pub struct EnrichmentStatus {
    pub successful: usize,
    /// Waiting on a rate limit or in flight. Grows while a run learns what else to ask.
    pub pending: usize,
    pub failed: usize,
}

impl EnrichmentStatus {
    pub fn state(&self) -> EnrichmentState;
}

pub enum EnrichmentState {
    /// pending > 0
    InProgress,
    /// pending == 0 and failed > 0
    Failed,
    /// pending == 0 and successful > 0
    Completed,
}
```

### Run handle

```rust
/// A handle to the enrichment requests for one entity
#[derive(Clone)]
pub struct Enrichment { /* watch::Receiver<EnrichmentStatus>, plus which entity it is about */ }

impl Enrichment {
    /// The entity's status as of now.
    pub fn status(&self) -> EnrichmentStatus;
    // Awaiting finishes the next time a request returns, fails, or the run finishes / is cancelled.
    // Resolves immediately if the task is finished.
    pub async fn changed(&mut self);
    /// Stop pending requests and forget the run.
    pub fn cancel(self);
}
```

## Search

Search is cross-library, best match first, ranked in Rust over every candidate the same way the
archive does it. Search never returns drafts.

```rust
impl Engine {
    pub async fn search(&self, query: &str) -> Result<Vec<SearchHit>>;
}

pub struct SearchHit {
    pub library: LibraryId,
    pub library_name: String,
    // the consumer is expected to filter out private results if necessary.
    pub private: bool,
    pub entity: AnyEntity,
}
```

## Suggestions

The suggestions API is intended for typeahead suggestions while editing a field.

```rust
/// Which field to provide suggestions for
pub enum SuggestField {
    Publication,
    Work,
    /// Catalog numbers, filtered to a given composer's works when present
    WorkNumber { composer: Option<PersonId> },
    Person,
    Role,
    Publisher,
    Tag,
    Key,
    TimeSignature,
    Instrumentation,
    Location,
}

// May return drafts. If drafts aren't wanted, filter them out
pub enum Suggested {
    Publication(Publication),
    Work { work: Work, number: Option<CatalogNumber> },
    Person(Person),
    Role(String),
    Publisher(String),
    Tag { tag: Tag, count: usize },
    Key(String),
    TimeSignature(String),
    Instrumentation(String),
    Location(String),
}

pub struct Suggestion {
    /// The query text equals the suggestion after normalization.
    pub exact: bool,
    pub item: Suggested,
}

impl Library {
    pub async fn suggest(&self, field: SuggestField, query: &str) -> Result<Vec<Suggestion>>;
}
```

### External candidates

```rust
pub enum EntityKind {
    Publication, Work, Person
}
impl Engine {
    pub async fn external_suggest(&self, kind: EntityKind, query: &str) -> Result<Vec<ExternalCandidate>>;
}

pub struct ExternalCandidate {
    pub kind: EntityKind,
    pub source: Source,
    pub name: String,
    pub context: Option<String>, // disambiguation context
    pub record: Link, // a link to the record itself to do future lookups on
}
```

## Digital assets

The storage rules are defined by the `05-data-model.md` document.

```rust
pub enum AssetSource<'a> {
    Upload { data: Box<dyn AsyncRead + Send + Unpin + 'a>, filename: &'a str },
    Url(Url),
}

pub enum NewHolding<'a> {
    Physical { location: Option<&'a str> },
    Digital(AssetSource<'a>),
}

impl Library {
    pub async fn files(&self) -> Result<Vec<PathBuf>>;
}
```

Paths out of the engine are absolute, joined with the data directory, because that is what a
consumer opens or serves. Paths stored are relative to the data directory.

Cover and profile images must decode as an image, tested with a MIME type crate on the bytes rather
than the extension, because a URL can promise `.jpg` and actually resolve to an HTML error page.
