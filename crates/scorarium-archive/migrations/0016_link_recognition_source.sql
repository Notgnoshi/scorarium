-- no-transaction
PRAGMA foreign_keys = OFF;

BEGIN;

-- Rebuilt to admit the link_recognition source; SQLite cannot alter a CHECK constraint
CREATE TABLE audit_entry_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    group_id INTEGER REFERENCES audit_entry(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    source TEXT NOT NULL CHECK (source IN ('user', 'orphan_cleanup', 'merge', 'link_recognition')),
    action TEXT NOT NULL CHECK (action IN (
        'created', 'updated', 'deleted', 'merged', 'renamed',
        'import_started', 'import_accepted', 'import_discarded',
        'password_claimed', 'password_changed'
    )),
    entity_kind TEXT CHECK (entity_kind IN ('publication', 'work', 'person', 'library', 'import')),
    entity_id INTEGER,
    entity_library_id INTEGER,
    entity_label TEXT,
    fields TEXT
) STRICT;
INSERT INTO audit_entry_new (id, group_id, created_at, source, action, entity_kind, entity_id, entity_library_id, entity_label, fields)
    SELECT id, group_id, created_at, source, action, entity_kind, entity_id, entity_library_id, entity_label, fields FROM audit_entry;
DROP TABLE audit_entry;
ALTER TABLE audit_entry_new RENAME TO audit_entry;
CREATE INDEX audit_entry_group ON audit_entry (COALESCE(group_id, id) DESC, id ASC);

COMMIT;

PRAGMA foreign_keys = ON;
