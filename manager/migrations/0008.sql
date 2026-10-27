CREATE TABLE manager_identity (id INTEGER PRIMARY KEY CHECK(id = 1), identity TEXT NOT NULL UNIQUE) STRICT;
CREATE TABLE owner_devices (
    id TEXT PRIMARY KEY,
    credential_hash BLOB NOT NULL UNIQUE CHECK(length(credential_hash) = 32),
    label TEXT NOT NULL CHECK(length(label) BETWEEN 1 AND 100),
    role TEXT NOT NULL CHECK(role = 'owner'),
    epoch INTEGER NOT NULL DEFAULT 1 CHECK(epoch > 0),
    revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0,1)),
    created_at INTEGER NOT NULL
) STRICT;
CREATE TABLE owner_invitations (
    token_hash BLOB PRIMARY KEY CHECK(length(token_hash) = 32),
    manager TEXT NOT NULL,
    role TEXT NOT NULL CHECK(role = 'owner'),
    credential_hash BLOB NOT NULL CHECK(length(credential_hash) = 32),
    label TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    consumed_at INTEGER
) STRICT;
CREATE TABLE remote_attempts (id INTEGER PRIMARY KEY CHECK(id = 1), window_start INTEGER NOT NULL, attempts INTEGER NOT NULL) STRICT;
INSERT INTO remote_attempts VALUES(1, 0, 0);
DELETE FROM worker_invitations;
ALTER TABLE worker_invitations ADD COLUMN manager TEXT;
ALTER TABLE worker_invitations ADD COLUMN role TEXT NOT NULL DEFAULT 'worker' CHECK(role = 'worker');
PRAGMA user_version = 8;
