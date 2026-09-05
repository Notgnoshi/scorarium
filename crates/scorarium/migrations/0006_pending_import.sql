CREATE TABLE pending_import (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    library_id INTEGER NOT NULL REFERENCES library(id) ON DELETE CASCADE,
    query TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('physical', 'digital')),
    location TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
