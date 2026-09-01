-- The single-user login password, hashed and salted with a standard password KDF.
CREATE TABLE password (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    password_hash TEXT NOT NULL
) STRICT;
