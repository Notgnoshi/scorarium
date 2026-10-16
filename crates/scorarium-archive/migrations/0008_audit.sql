CREATE TABLE audit_entry (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    source TEXT NOT NULL CHECK (source IN ('user', 'orphan_cleanup', 'merge', 'link_recognition')),
    action TEXT NOT NULL CHECK (action IN (
        'created', 'updated', 'deleted', 'merged', 'import_accepted',
        'password_claimed', 'password_changed'
    )),
    entity_kind TEXT CHECK (entity_kind IN ('publication', 'work', 'person', 'library')),
    entity_id INTEGER,
    entity_library_id INTEGER,
    -- The entity's name as it read at the time, so a deleted entity still renders as something
    entity_label TEXT,
    -- NULL unless the action is 'updated'
    field TEXT
) STRICT;
