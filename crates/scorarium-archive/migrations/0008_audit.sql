CREATE TABLE audit_entry (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    -- NULL on the root entry of a group; otherwise the root entry's id
    group_id INTEGER REFERENCES audit_entry(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    source TEXT NOT NULL CHECK (source IN ('user', 'orphan_cleanup', 'merge')),
    action TEXT NOT NULL CHECK (action IN (
        'created', 'updated', 'deleted', 'merged', 'renamed',
        'import_started', 'import_accepted', 'import_discarded', 'password_changed'
    )),
    entity_kind TEXT CHECK (entity_kind IN ('publication', 'work', 'person', 'library', 'import')),
    entity_id INTEGER,
    entity_library_id INTEGER,
    -- The entity's name as it read at the time, so a deleted entity still renders as something
    entity_label TEXT,
    -- JSON array of field names, NULL unless the action is 'updated'
    fields TEXT
) STRICT;

CREATE INDEX audit_entry_group ON audit_entry (COALESCE(group_id, id) DESC, id ASC);
