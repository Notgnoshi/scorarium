# Move in-memory drafts into the database

Currently, pending imports (a.k.a., drafts) are saved in a `HashMap<i64, Draft>` in memory. As I've
been working on the `scorarium-engine` API design, I've encountered quite a bit of friction trying
to paper over the in-memory draft store, the database, and then especially figuring out how
background metadata enrichment results get applied and deconflicted.

I think that this design might be flawed, and it'd be easier to move the drafts and pending
enrichment results into the database.

## Principles

* Drafts, enrichment proposals, and committed entities all share one data model. A draft person is
  in the people table, marked as a draft.
* Committing drafts is a state change, removing the draft mark. Discarding a draft is a deletion
  from the database. Committing a draft cascades to any draft child entities.
* A proposal is one fact about an entity from an external API adapted to the Scorarium data model
* A proposed new entity is a draft entity
* The `scorarium-engine` APIs should take strong types that can only be constructed with valid
  values

## Data model

The data model design document describes the draft mark, the entity timestamps, the proposal tables,
and the deletion and visibility rules for drafts and proposals.

## Import and enrichment lifecycle

Draft publications are created immediately with the title, ISBN, or ISMN from the import UI.
Ideally, the external title search suggestions enable the user to select a suggestion, in which case
the draft publication is seeded with the external results. This is not considered enrichment,
because the user has the opportunity to make an explicit choice, so the seeding happens directly on
the draft, not in the proposal tables.

Enrichment does not directly edit the enriched entity. Rather, enrichment generates proposed field
values in a table for the UI layer to display to the user for selection and deconflict. Proposals
that propose the creation of a new entity (works or people) create a new draft entity that the
proposal refers to. These drafts are not considered orphans, because they're referred to by a
proposal.

An enrichment run is triggered from a single root entity. It may generate proposals for that entity,
or any other entities linked to from the root. An enrichment task tracks how many sources it has
asked, which have answered, and is cancellable.

## Entity visibility

Draft entities are considered private, even if they're drafts on a public library. Once logged in,
draft entities are only visible through the review surfaces, not the various listings or search
results.

Drafts _are_ included in typeahead suggestions, allowing the user to share the same draft entity
between multiple other entities.

## Validation

Where possible, strong types will be used to represent validated values. The only way to create a
strong type will be with a validated constructor like `TryFrom` or `FromStr`.

## scorarium-engine vs scorarium-archive

`scorarium-engine` does not build on top of `scorarium-archive`; it's an attempt to redesign
`scorarium-archive` with a new data model. Once the web server migrates to the engine crate,
`scorarium-archive` can be deleted.

## Review queue

The review queue displays draft publications, and any entity with pending proposals.
