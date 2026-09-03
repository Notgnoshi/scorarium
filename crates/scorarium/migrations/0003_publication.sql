CREATE TABLE publication (
    id INTEGER PRIMARY KEY,
    library_id INTEGER NOT NULL REFERENCES library(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    publisher TEXT,
    year INTEGER,
    private INTEGER NOT NULL DEFAULT 0 CHECK (private IN (0, 1)),
    cover TEXT,
    note TEXT,
    stars INTEGER CHECK (stars BETWEEN 1 AND 5)
) STRICT;

CREATE TABLE publication_identifier (
    id INTEGER PRIMARY KEY,
    publication_id INTEGER NOT NULL REFERENCES publication(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('isbn', 'ismn', 'publisher_number', 'plate_number')),
    -- value is normalized
    value TEXT NOT NULL,
    UNIQUE (publication_id, kind, value)
) STRICT;

CREATE TABLE holding (
    id INTEGER PRIMARY KEY,
    publication_id INTEGER NOT NULL REFERENCES publication(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('physical', 'digital')),
    location TEXT,
    loaned_to TEXT
) STRICT;
