# scorarium features

## Libraries

Support multiple named libraries. Each library has its own catalog of publications and works. Search
can be limited to a single library, or across all libraries. Contents of libraries are public by
default, but individual publications can be marked private. Downloadable assets (PDFs, EPUBs) are
always private.

There are no links or relationships between libraries; they're intended to be separate instances of
the database. It's expected that searching can be done per-library, or across all libraries.

## Cataloging

Publications have titles, publisher, year, and ISBN/ISMN identifiers. Publications have contributors
with roles (composer, arranger, editor, translator, etc). Works are first-class with titles,
contributors, catalog numbers, key, time signature, instrumentation. A publication includes a list
of works. A given work can appear in multiple publications. Work identity is resolved at import
time: it should be easy to import the right thing rather than fix duplicates later. Publications may
also have holdings associated with them - physical copies, along with digital copies.

Publications can have metadata like notes or tags. It should be possible to use tags to build
user-defined collections.

Works also have user-relationship metadata - tags, notes, and familiarity (want-to-learn, learning,
learned, memorized, sight-read, read). It should be possible to search for works that the user has a
particular relationship with.

Publications can also have additional metadata like "this publication has been loaned to ...". Loans
are private.

Importing publications should attempt to match them against online ISBN / ISMN databases, and
metadata should be imported if possible. It should be possible to override imported metadata, and to
add works that are not found in the online databases. It should be possible to edit the metadata
after importing a publication. If possible, importing publications should attempt to define and
import the works contained in the publication. If not possible to auto-import, the user should be
able to define the works contained by a publication during or after time of import.

Duplicate prevention happens at import time rather than through after-the-fact merging. Entry fields
for works and contributors offer autocomplete suggestions from the existing library (achievable
without JavaScript via native HTML datalist). Auto-imported works and contributors are matched
against existing records and presented for confirmation before anything is created.

Uploading digital assets should be possible from the web UI. PDF/EPUB are primary formats, but
scorarium should not enforce particular formats. It should be possible to have multiple digital
holdings of a given publication (e.g., a PDF and EPUB of the same publication).

If possible, works should include links to online databases (IMSLP & MusicBrainz).

## Search

It should be easy to search for publications and works. Advanced search is not required. It should
be able to search by title, composer, opus number, and user-relationships.

## Access control

A single-user login is required to access private data (notes, tags, relationships, digital assets)
or modify any data.

The password is stored hashed in the database, not in the configuration file. When no password is
stored, the first login sets it. The first-to-claim race is acceptable for a small self-hosted
deployment where the operator also has backend access; forgotten passwords are reset by deleting the
stored row and claiming again, so no reset flow is needed.

## Settings

A settings page, accessible while logged in, for everything about the deployment rather than the
catalog. It hosts the change-password form, a small set of predefined operations that go through the
application so invariants hold (e.g., rebuild the search index), and read-only debugging views: raw
table contents, row counts, orphan checks, and search index status, as a substitute for inspecting
the database with SQL directly. Advanced data manipulation (merging duplicate works or persons, bulk
retagging) may be added later as further predefined operations. Raw row editing is out of scope. The
page can split into child pages if it grows noisy.

## Self hosting

It should be easy to self-host. A `docker-compose.yml` is suggested, with the ability to easily
update the deployment.

## UI

The UI should be lightweight, and not attempt to be appealing. Raw HTML with little-to-no CSS except
where absolutely necessary is desired. The focus is on what scorarium does, not how good it looks
doing it.

The use of JavaScript should be minimized to as little as possible, or possibly even less. The
backend will be written in Rust, with SQLite as the database.

## forScore

forScore integration is likely in the future, but deferred until I understand what this actually
means.
