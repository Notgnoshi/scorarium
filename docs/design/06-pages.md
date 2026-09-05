# scorarium pages

The web UI is a thin wrapper around the data model: every page is a URL, every action is an HTML
form, and pages basically match the database. JavaScript is used sparingly, where it makes something
meaningfully easier, not as a rule. No listing paginates: a personal library is a few hundred
publications, and one long page is scannable and ctrl-F-able.

## Conventions

* **Shared header** on every page: a search box (scoped to the current library when inside one, all
  libraries elsewhere) and a login link, or, when logged in, a logout button and links to the review
  queue (with a count of imports awaiting review), the loans page, and the settings page.
  Breadcrumbs render horizontally beneath the header, reflecting the URL hierarchy.
* **View and edit modes.** Entity pages render in view mode with empty fields hidden. An edit button
  (logged in) exposes the full form, empty fields included, with save and cancel buttons: save
  commits all the changes at once, cancel discards them, and both return to view mode.
* **Destructive confirmation.** Destructive actions (deleting a library, a publication, or a
  holding) confirm via a JavaScript dialog before submitting, and the dialog states what cascades:
  deleting a last holding deletes its publication, and deleting a publication garbage-collects its
  orphaned works and persons along with the notes and familiarity on them.
* **Sortable listings.** Tabular listings sort by any displayed column via links in the column
  headers (a `?sort=` query parameter).
* **Autocomplete.** Entry fields for works, persons, and tags suggest existing values, so importing
  the right thing is easier than fixing duplicates later.
* **Tags are links.** Wherever a tag is displayed, it links to that library's page for the tag.
* **Visibility** follows the access control rules in the data model document. "Logged in" below
  marks pages or fragments hidden from logged-out visitors.

## URL scheme

URLs are nested to match the data hierarchy. There are no cross-library pages other than the home
page, the all-libraries search, and the loans page.

```
/                                   home: all-library search, library list
/search?q=                          all-library search results
/login                              login (first login claims the password)
/loans                              loaned-out holdings across all libraries
/review                             review queue for pending imports
/settings                           change password, predefined operations, debugging views
/library/{id}                       publications listing
/library/{id}/search?q=             single-library search results
/library/{id}/import                import entry page
/library/{id}/import/{pending_id}   review page for a pending import
/library/{id}/publication/{id}      publication detail
/library/{id}/work/{id}             work detail
/library/{id}/person/{id}           person detail
/library/{id}/composers             persons with a composer role on any work
/library/{id}/authors               persons with an author role on any publication
/library/{id}/tags                  tag listing
/library/{id}/tag/{name}            everything tagged {name}
```

## Pages

### Home: /

Public. Shows the all-library search box and the list of libraries, each with its publication count
and author/composer count. Logged in, the page also offers a create-library form and rename/delete
for each library.

### Search results: /search and /library/{id}/search

Public (results respect publication privacy). One merged, relevance-ranked list across publications,
works, and persons; each row carries a tag or icon naming its class. Work rows show title, catalog
number, and composer; publication rows show title, people, year, and holding kinds; person rows show
the name. All-library results also name the library each row belongs to; the single-library page
omits it.

### Library: /library/{id}

Public. A sortable table of the library's publications: title, people (via the role heuristic,
below), year, and holding kinds. Default sort is composer/author surname then title, matching how a
shelf is scanned. Links to the composers and authors listings and, when logged in, the tags listing
and an import button leading to the import flow.

### Import: /library/{id}/import and /library/{id}/import/{pending_id}

Both pages require login. The entry page takes an identifier or a title, plus a physical/digital
toggle, a location or file field, and an import-more checkbox for batch entry. Local matches are
offered before external ones, so the page suggests adding a holding to an existing publication
instead of importing a duplicate; picked candidates become pending imports that enrich in the
background. The review page for a pending import shares its implementation with the publication
page's edit mode; accepting creates the publication, its works, and its first holding in one
transaction. The import document designs the full flow.

### Review queue: /review

A logged-in, cross-library worklist of pending imports, each with a progress indicator, a cancel
button, and a link to its review page. The header links here with a count of imports awaiting
review. The import document has the details.

### Publication: /library/{id}/publication/{id}

Public except where noted. Shows cover art (or a generated placeholder with title and
composer/author), title, publisher, year, identifiers, external links, contributors with roles,
stars, the contained works (sorted per the data model's contents rules), and the holdings with their
kinds. When logged in, holdings also show location and loaned-to, digital holdings link to
open/download the file, and note, tags, and the private flag appear.

Edit mode covers every field, plus: add/remove contained works and contributors (with autocomplete),
manage identifiers, upload cover art or supply a URL to fetch it from (fetched once and stored, not
hotlinked), add a holding including file upload for digital ones, and delete the publication or
individual holdings, with the deletion and garbage-collection semantics from the data model
document. Edit mode also offers the on-demand enrich action from the import document, which fetches
metadata and (for sheet music) contents suggestions with the same accept/reject affordances as
import review; this is how a publication accepted without works gains them later.

### Work: /library/{id}/work/{id}

Public except where noted. Shows title, catalog numbers, key, time signature, instrumentation,
contributors with roles, external links, and stars; logged in, also tags, familiarity, and note. The
page lists the publications containing the work, each with its holdings inline (kinds always;
locations when logged in), so "do I have the sheet music for this piece?" is answered on this page
alone. Edit mode covers the work's own fields, catalog numbers, contributors, and external ids, and
offers the same on-demand enrich action as publications; works are created and removed through
publications, not here.

### Person: /library/{id}/person/{id}

Public except private-publication filtering. Shows name, aliases, and external links, then the
person's publications with the person's own works nested beneath each one. The publication is the
unit that matters ("go find it on a shelf"), so an anthology appears once with only this person's
pieces nested under it; a work in several publications appears under each. A person credited only at
the publication level (an editor, the author of a technique book) shows the publication with their
role and nothing nested. Edit mode covers name, aliases, and external ids.

### Composers and authors: /library/{id}/composers and /library/{id}/authors

Public. The role heuristic: the composers page lists persons with a `composer` role on any work in
the library; the authors page lists persons with an `author` role on any publication. Arrangers,
editors, and translators get person pages but no listing page. Rows show the name and a count of
works (composers) or publications (authors), linking to person pages. A person can appear on both
lists.

### Tags: /library/{id}/tags and /library/{id}/tag/{name}

Logged in (tags are private). The tags listing shows the library's distinct tags with counts, each
linking to its tag page. A tag page shows everything carrying the tag, publications and works in one
merged list with type tags on each row, search-results style. Tags are per-library; there is no
cross-library tag view.

### Loans: /loans

Logged in. One table of every loaned-out holding across all libraries: publication title, library,
holding kind, and who has it, with rows linking to the publication. This is the page that answers
"who did I lend it to?".

### Login: /login

Public (that is the point). One password field; when no password is stored yet, the first login sets
it, per the access control design. After login, return to the page the user came from. Logout is a
button in the header. Changing the password happens on the settings page.

### Settings: /settings

Logged in. Everything about the deployment rather than the catalog, per the features document: the
change-password form (requires the current password alongside the new one), predefined operations
that go through the application (starting with rebuild-the-search-index), and read-only debugging
views (raw table contents, row counts, orphan checks, search index status). Future advanced
operations (merging duplicate works or persons, bulk retagging) land here. The page splits into
child pages if it grows noisy.
