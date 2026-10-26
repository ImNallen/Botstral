CREATE TABLE workers (
    id TEXT PRIMARY KEY,
    fingerprint TEXT NOT NULL UNIQUE CHECK(length(fingerprint) = 64),
    revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0, 1)),
    generation INTEGER NOT NULL DEFAULT 0 CHECK(generation BETWEEN 0 AND 9007199254740991)
) STRICT;
CREATE TABLE worker_invitations (
    token_hash BLOB PRIMARY KEY CHECK(length(token_hash) = 32),
    fingerprint TEXT NOT NULL CHECK(length(fingerprint) = 64),
    expires_at INTEGER NOT NULL,
    consumed_at INTEGER
) STRICT;
CREATE TABLE computer_sessions (
    id TEXT PRIMARY KEY,
    bot_id TEXT NOT NULL UNIQUE REFERENCES bots(id),
    worker_id TEXT NOT NULL REFERENCES workers(id),
    owner TEXT NOT NULL CHECK(owner IN ('bot', 'owner')),
    revision INTEGER NOT NULL CHECK(revision BETWEEN 1 AND 9007199254740991)
) STRICT;
CREATE INDEX computer_sessions_worker ON computer_sessions(worker_id);
PRAGMA user_version = 2;
