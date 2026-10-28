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
