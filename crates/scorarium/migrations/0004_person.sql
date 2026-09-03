CREATE TABLE person (
    id INTEGER PRIMARY KEY,
    library_id INTEGER NOT NULL REFERENCES library(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    sort_name TEXT NOT NULL
) STRICT;

-- Roles are conventional text (author, composer, editor, ...), not an enumeration.
CREATE TABLE publication_contributor (
    id INTEGER PRIMARY KEY,
    publication_id INTEGER NOT NULL REFERENCES publication(id) ON DELETE CASCADE,
    person_id INTEGER NOT NULL REFERENCES person(id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    UNIQUE (publication_id, person_id, role)
) STRICT;
