CREATE TABLE publication (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    library_id INTEGER NOT NULL REFERENCES library(id) ON DELETE CASCADE,
    title TEXT,
    publisher TEXT,
    year INTEGER,
    note TEXT,
    stars INTEGER CHECK (stars BETWEEN 1 AND 5),
    kind TEXT CHECK (kind IN ('music', 'prose')),
    draft INTEGER NOT NULL DEFAULT 0 CHECK (draft IN (0, 1)),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    last_modified_at INTEGER NOT NULL DEFAULT (unixepoch()),
    CHECK (draft = 1 OR title IS NOT NULL),
    UNIQUE (library_id, id)
) STRICT;

CREATE TABLE holding (
    id INTEGER PRIMARY KEY,
    publication_id INTEGER NOT NULL REFERENCES publication(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('physical', 'digital')),
    location TEXT,
    loaned_to TEXT
) STRICT;
CREATE INDEX holding_publication ON holding(publication_id);

CREATE TABLE work (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    library_id INTEGER NOT NULL REFERENCES library(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    key TEXT,
    time_signature TEXT,
    instrumentation TEXT,
    note TEXT,
    stars INTEGER CHECK (stars BETWEEN 1 AND 5),
    kind TEXT CHECK (kind IN ('music', 'prose')),
    draft INTEGER NOT NULL DEFAULT 0 CHECK (draft IN (0, 1)),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    last_modified_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (library_id, id)
) STRICT;

CREATE TABLE publication_work (
    id INTEGER PRIMARY KEY,
    library_id INTEGER NOT NULL,
    publication_id INTEGER NOT NULL,
    work_id INTEGER NOT NULL,
    FOREIGN KEY (publication_id, library_id) REFERENCES publication(id, library_id) ON DELETE CASCADE,
    FOREIGN KEY (work_id, library_id) REFERENCES work(id, library_id) ON DELETE CASCADE,
    UNIQUE (publication_id, work_id)
) STRICT;
CREATE INDEX publication_work_work ON publication_work(work_id);

CREATE TABLE person (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    library_id INTEGER NOT NULL REFERENCES library(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    sort_name TEXT NOT NULL,
    profile TEXT,
    draft INTEGER NOT NULL DEFAULT 0 CHECK (draft IN (0, 1)),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    last_modified_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (library_id, id)
) STRICT;

CREATE TABLE publication_contributor (
    id INTEGER PRIMARY KEY,
    library_id INTEGER NOT NULL,
    publication_id INTEGER NOT NULL,
    person_id INTEGER NOT NULL,
    role TEXT NOT NULL CHECK (role <> ''),
    FOREIGN KEY (publication_id, library_id) REFERENCES publication(id, library_id) ON DELETE CASCADE,
    FOREIGN KEY (person_id, library_id) REFERENCES person(id, library_id) ON DELETE CASCADE,
    UNIQUE (publication_id, person_id, role)
) STRICT;
CREATE INDEX publication_contributor_person ON publication_contributor(person_id);

CREATE TABLE work_contributor (
    id INTEGER PRIMARY KEY,
    library_id INTEGER NOT NULL,
    work_id INTEGER NOT NULL,
    person_id INTEGER NOT NULL,
    role TEXT NOT NULL CHECK (role <> ''),
    FOREIGN KEY (work_id, library_id) REFERENCES work(id, library_id) ON DELETE CASCADE,
    FOREIGN KEY (person_id, library_id) REFERENCES person(id, library_id) ON DELETE CASCADE,
    UNIQUE (work_id, person_id, role)
) STRICT;
CREATE INDEX work_contributor_person ON work_contributor(person_id);

CREATE TABLE tag (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    library_id INTEGER NOT NULL,
    publication_id INTEGER,
    work_id INTEGER,
    tag TEXT NOT NULL CHECK (tag <> ''),
    CHECK ((publication_id IS NULL) <> (work_id IS NULL)),
    FOREIGN KEY (publication_id, library_id) REFERENCES publication(id, library_id) ON DELETE CASCADE,
    FOREIGN KEY (work_id, library_id) REFERENCES work(id, library_id) ON DELETE CASCADE,
    UNIQUE (publication_id, tag),
    UNIQUE (work_id, tag)
) STRICT;
CREATE INDEX tag_library_tag ON tag(library_id, tag);

CREATE TABLE publication_identifier (
    id INTEGER PRIMARY KEY,
    publication_id INTEGER NOT NULL REFERENCES publication(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('isbn', 'ismn', 'publisher_number', 'plate_number')),
    value TEXT NOT NULL,
    UNIQUE (publication_id, kind, value)
) STRICT;
CREATE INDEX publication_identifier_value ON publication_identifier(value);

CREATE TABLE work_catalog_number (
    id INTEGER PRIMARY KEY,
    work_id INTEGER NOT NULL REFERENCES work(id) ON DELETE CASCADE,
    value TEXT NOT NULL CHECK (value <> ''),
    UNIQUE (work_id, value)
) STRICT;

CREATE TABLE publication_link (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    publication_id INTEGER NOT NULL REFERENCES publication(id) ON DELETE CASCADE,
    url TEXT NOT NULL,
    kind TEXT NOT NULL,
    external_id TEXT,
    CHECK ((kind = 'generic') = (external_id IS NULL)),
    UNIQUE (publication_id, url),
    UNIQUE (publication_id, kind, external_id)
) STRICT;
CREATE INDEX publication_link_kind_external_id ON publication_link(kind, external_id);

CREATE TABLE work_link (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    work_id INTEGER NOT NULL REFERENCES work(id) ON DELETE CASCADE,
    url TEXT NOT NULL,
    kind TEXT NOT NULL,
    external_id TEXT,
    CHECK ((kind = 'generic') = (external_id IS NULL)),
    UNIQUE (work_id, url),
    UNIQUE (work_id, kind, external_id)
) STRICT;
CREATE INDEX work_link_kind_external_id ON work_link(kind, external_id);

CREATE TABLE person_link (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    person_id INTEGER NOT NULL REFERENCES person(id) ON DELETE CASCADE,
    url TEXT NOT NULL,
    kind TEXT NOT NULL,
    external_id TEXT,
    CHECK ((kind = 'generic') = (external_id IS NULL)),
    UNIQUE (person_id, url),
    UNIQUE (person_id, kind, external_id)
) STRICT;
CREATE INDEX person_link_kind_external_id ON person_link(kind, external_id);
