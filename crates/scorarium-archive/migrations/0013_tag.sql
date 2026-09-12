CREATE TABLE tag (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    library_id INTEGER NOT NULL,
    publication_id INTEGER,
    work_id INTEGER,
    tag TEXT NOT NULL,
    CHECK ((publication_id IS NULL) != (work_id IS NULL)),
    FOREIGN KEY (publication_id, library_id) REFERENCES publication(id, library_id) ON DELETE CASCADE,
    FOREIGN KEY (work_id, library_id) REFERENCES work(id, library_id) ON DELETE CASCADE
) STRICT;

CREATE UNIQUE INDEX tag_publication ON tag (publication_id, tag);
CREATE UNIQUE INDEX tag_work ON tag (work_id, tag);
CREATE INDEX tag_library ON tag (library_id, tag);
