CREATE TABLE owner (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    name TEXT NOT NULL,
    password_hash TEXT NOT NULL
) STRICT;
CREATE TABLE vault (
    id INTEGER PRIMARY KEY CHECK (id = 1) REFERENCES owner(id),
    format_version INTEGER NOT NULL CHECK (format_version = 1),
    identity_ciphertext BLOB NOT NULL
) STRICT;
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    token_hash BLOB NOT NULL UNIQUE CHECK (length(token_hash) = 32),
    owner_id INTEGER NOT NULL CHECK (owner_id = 1) REFERENCES owner(id),
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL CHECK (expires_at > created_at)
) STRICT;
CREATE TABLE bots (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    created_at INTEGER NOT NULL
) STRICT;
CREATE TABLE provider_keys (
    provider TEXT PRIMARY KEY CHECK (provider = 'mistral'),
    format_version INTEGER NOT NULL CHECK (format_version = 1),
    ciphertext BLOB NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;
PRAGMA user_version = 1;

