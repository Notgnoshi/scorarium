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
