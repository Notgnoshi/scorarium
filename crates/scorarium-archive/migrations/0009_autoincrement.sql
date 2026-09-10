-- no-transaction
PRAGMA foreign_keys = OFF;

BEGIN;

CREATE TABLE library_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL
) STRICT;
INSERT INTO library_new (id, name) SELECT id, name FROM library;
DROP TABLE library;
ALTER TABLE library_new RENAME TO library;

CREATE TABLE publication_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    library_id INTEGER NOT NULL REFERENCES library(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    publisher TEXT,
    year INTEGER,
    private INTEGER NOT NULL DEFAULT 0 CHECK (private IN (0, 1)),
    cover TEXT,
    note TEXT,
    stars INTEGER CHECK (stars BETWEEN 1 AND 5),
    -- Lets link tables reference (id, library_id) so their parents must share a library
    UNIQUE (library_id, id)
) STRICT;
INSERT INTO publication_new (id, library_id, title, publisher, year, private, cover, note, stars)
    SELECT id, library_id, title, publisher, year, private, cover, note, stars FROM publication;
DROP TABLE publication;
ALTER TABLE publication_new RENAME TO publication;

CREATE TABLE person_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    library_id INTEGER NOT NULL REFERENCES library(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    sort_name TEXT NOT NULL,
    UNIQUE (library_id, id)
) STRICT;
INSERT INTO person_new (id, library_id, name, sort_name)
    SELECT id, library_id, name, sort_name FROM person;
DROP TABLE person;
ALTER TABLE person_new RENAME TO person;

CREATE TABLE work_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    library_id INTEGER NOT NULL REFERENCES library(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    "key" TEXT,
    time_signature TEXT,
    instrumentation TEXT,
    UNIQUE (library_id, id)
) STRICT;
INSERT INTO work_new (id, library_id, title, "key", time_signature, instrumentation)
    SELECT id, library_id, title, "key", time_signature, instrumentation FROM work;
DROP TABLE work;
ALTER TABLE work_new RENAME TO work;

COMMIT;

PRAGMA foreign_keys = ON;
