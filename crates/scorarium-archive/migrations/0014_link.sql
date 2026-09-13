CREATE TABLE publication_link (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    publication_id INTEGER NOT NULL REFERENCES publication(id) ON DELETE CASCADE,
    url TEXT NOT NULL
) STRICT;

CREATE TABLE work_link (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    work_id INTEGER NOT NULL REFERENCES work(id) ON DELETE CASCADE,
    url TEXT NOT NULL
) STRICT;

CREATE TABLE person_link (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    person_id INTEGER NOT NULL REFERENCES person(id) ON DELETE CASCADE,
    url TEXT NOT NULL
) STRICT;

CREATE UNIQUE INDEX publication_link_url ON publication_link (publication_id, url);
CREATE UNIQUE INDEX work_link_url ON work_link (work_id, url);
CREATE UNIQUE INDEX person_link_url ON person_link (person_id, url);
