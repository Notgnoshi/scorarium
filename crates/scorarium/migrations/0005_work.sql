CREATE TABLE work (
    id INTEGER PRIMARY KEY,
    library_id INTEGER NOT NULL REFERENCES library(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    "key" TEXT,
    time_signature TEXT,
    instrumentation TEXT,
    UNIQUE (library_id, id)
) STRICT;

CREATE TABLE work_catalog_number (
    id INTEGER PRIMARY KEY,
    work_id INTEGER NOT NULL REFERENCES work(id) ON DELETE CASCADE,
    value TEXT NOT NULL,
    UNIQUE (work_id, value)
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

CREATE TABLE work_contributor (
    id INTEGER PRIMARY KEY,
    library_id INTEGER NOT NULL,
    work_id INTEGER NOT NULL,
    person_id INTEGER NOT NULL,
    role TEXT NOT NULL,
    FOREIGN KEY (work_id, library_id) REFERENCES work(id, library_id) ON DELETE CASCADE,
    FOREIGN KEY (person_id, library_id) REFERENCES person(id, library_id) ON DELETE CASCADE,
    UNIQUE (work_id, person_id, role)
) STRICT;
