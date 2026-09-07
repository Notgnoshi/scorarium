CREATE TABLE person (
    id INTEGER PRIMARY KEY,
    library_id INTEGER NOT NULL REFERENCES library(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    sort_name TEXT NOT NULL,
    UNIQUE (library_id, id)
) STRICT;

-- Roles are conventional text (author, composer, editor, ...), not an enumeration.
CREATE TABLE publication_contributor (
    id INTEGER PRIMARY KEY,
    -- Both composite keys include library_id, so a publication cannot be linked to a person from
    -- another library.
    library_id INTEGER NOT NULL,
    publication_id INTEGER NOT NULL,
    person_id INTEGER NOT NULL,
    role TEXT NOT NULL,
    FOREIGN KEY (publication_id, library_id) REFERENCES publication(id, library_id) ON DELETE CASCADE,
    FOREIGN KEY (person_id, library_id) REFERENCES person(id, library_id) ON DELETE CASCADE,
    UNIQUE (publication_id, person_id, role)
) STRICT;
