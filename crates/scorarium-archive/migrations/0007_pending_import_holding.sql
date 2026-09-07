-- An import can carry several copies, so the copy moves off the import into a child table.
CREATE TABLE pending_import_holding (
    id INTEGER PRIMARY KEY,
    pending_import_id INTEGER NOT NULL REFERENCES pending_import(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('physical', 'digital')),
    location TEXT
) STRICT;

-- Carry whatever is pending across the upgrade
INSERT INTO pending_import_holding (pending_import_id, kind, location)
    SELECT id, kind, location FROM pending_import;

ALTER TABLE pending_import DROP COLUMN kind;
ALTER TABLE pending_import DROP COLUMN location;
