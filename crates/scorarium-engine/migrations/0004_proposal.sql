CREATE TABLE publication_proposal (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    library_id INTEGER NOT NULL,
    publication_id INTEGER NOT NULL,
    work_id INTEGER,
    person_id INTEGER,
    field TEXT NOT NULL CHECK (
        field IN ('title', 'publisher', 'year', 'identifier', 'contributor', 'contents', 'link', 'cover')
    ),
    source TEXT NOT NULL,
    proposed_value TEXT CHECK (proposed_value <> ''),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    CHECK (
        CASE field
            WHEN 'contents' THEN work_id IS NOT NULL AND person_id IS NULL AND proposed_value IS NULL
            WHEN 'contributor' THEN person_id IS NOT NULL AND work_id IS NULL AND proposed_value IS NOT NULL
            ELSE proposed_value IS NOT NULL AND work_id IS NULL AND person_id IS NULL
        END
    ),
    FOREIGN KEY (publication_id, library_id) REFERENCES publication(id, library_id) ON DELETE CASCADE,
    FOREIGN KEY (work_id, library_id) REFERENCES work(id, library_id) ON DELETE CASCADE,
    FOREIGN KEY (person_id, library_id) REFERENCES person(id, library_id) ON DELETE CASCADE
) STRICT;
CREATE UNIQUE INDEX publication_proposal_fact ON publication_proposal(
    publication_id, field, source, COALESCE(proposed_value, ''), COALESCE(work_id, 0), COALESCE(person_id, 0)
);
CREATE INDEX publication_proposal_work ON publication_proposal(work_id);
CREATE INDEX publication_proposal_person ON publication_proposal(person_id);

CREATE TABLE work_proposal (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    library_id INTEGER NOT NULL,
    work_id INTEGER NOT NULL,
    person_id INTEGER,
    field TEXT NOT NULL CHECK (
        field IN ('title', 'key', 'time_signature', 'instrumentation', 'catalog_number', 'contributor', 'link')
    ),
    source TEXT NOT NULL,
    proposed_value TEXT NOT NULL CHECK (proposed_value <> ''),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    CHECK ((field = 'contributor') = (person_id IS NOT NULL)),
    FOREIGN KEY (work_id, library_id) REFERENCES work(id, library_id) ON DELETE CASCADE,
    FOREIGN KEY (person_id, library_id) REFERENCES person(id, library_id) ON DELETE CASCADE
) STRICT;
CREATE UNIQUE INDEX work_proposal_fact ON work_proposal(
    work_id, field, source, proposed_value, COALESCE(person_id, 0)
);
CREATE INDEX work_proposal_person ON work_proposal(person_id);

CREATE TABLE person_proposal (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    library_id INTEGER NOT NULL,
    person_id INTEGER NOT NULL,
    field TEXT NOT NULL CHECK (field IN ('name', 'link', 'profile')),
    source TEXT NOT NULL,
    proposed_value TEXT NOT NULL CHECK (proposed_value <> ''),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    FOREIGN KEY (person_id, library_id) REFERENCES person(id, library_id) ON DELETE CASCADE
) STRICT;
CREATE UNIQUE INDEX person_proposal_fact ON person_proposal(person_id, field, source, proposed_value);

-- Committing a draft means the user is done reviewing, so facts learned about the draft before then
-- are dropped rather than carried onto the committed entity.
CREATE TRIGGER publication_committed AFTER UPDATE OF draft ON publication
WHEN OLD.draft = 1 AND NEW.draft = 0
BEGIN
    DELETE FROM publication_proposal WHERE publication_id = NEW.id;
END;

CREATE TRIGGER work_committed AFTER UPDATE OF draft ON work
WHEN OLD.draft = 1 AND NEW.draft = 0
BEGIN
    DELETE FROM work_proposal WHERE work_id = NEW.id;
END;

CREATE TRIGGER person_committed AFTER UPDATE OF draft ON person
WHEN OLD.draft = 1 AND NEW.draft = 0
BEGIN
    DELETE FROM person_proposal WHERE person_id = NEW.id;
END;
