ALTER TABLE publication_link ADD COLUMN kind TEXT NOT NULL DEFAULT 'generic';
ALTER TABLE publication_link ADD COLUMN external_id TEXT CHECK ((kind = 'generic') = (external_id IS NULL));
ALTER TABLE work_link ADD COLUMN kind TEXT NOT NULL DEFAULT 'generic';
ALTER TABLE work_link ADD COLUMN external_id TEXT CHECK ((kind = 'generic') = (external_id IS NULL));
ALTER TABLE person_link ADD COLUMN kind TEXT NOT NULL DEFAULT 'generic';
ALTER TABLE person_link ADD COLUMN external_id TEXT CHECK ((kind = 'generic') = (external_id IS NULL));

CREATE UNIQUE INDEX publication_link_external_id
    ON publication_link (publication_id, kind, external_id) WHERE external_id IS NOT NULL;
CREATE UNIQUE INDEX work_link_external_id
    ON work_link (work_id, kind, external_id) WHERE external_id IS NOT NULL;
CREATE UNIQUE INDEX person_link_external_id
    ON person_link (person_id, kind, external_id) WHERE external_id IS NOT NULL;

CREATE INDEX publication_link_kind ON publication_link (kind, external_id) WHERE external_id IS NOT NULL;
CREATE INDEX work_link_kind ON work_link (kind, external_id) WHERE external_id IS NOT NULL;
CREATE INDEX person_link_kind ON person_link (kind, external_id) WHERE external_id IS NOT NULL;
