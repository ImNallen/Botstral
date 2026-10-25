CREATE TABLE owner (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  username TEXT NOT NULL CHECK (length(username) BETWEEN 3 AND 64),
  password_hash TEXT NOT NULL CHECK (length(password_hash) > 0),
  created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE sessions (
  id TEXT PRIMARY KEY CHECK (length(id) = 32),
  owner_id INTEGER NOT NULL DEFAULT 1 CHECK (owner_id = 1) REFERENCES owner(id),
  token_hash BLOB NOT NULL UNIQUE CHECK (length(token_hash) = 32),
  created_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL CHECK (expires_at > created_at)
) STRICT;
CREATE INDEX sessions_expiry ON sessions(expires_at);

CREATE TABLE bots (
  id TEXT PRIMARY KEY CHECK (length(id) = 32),
  owner_id INTEGER NOT NULL DEFAULT 1 CHECK (owner_id = 1) REFERENCES owner(id),
  name TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 80),
  created_at INTEGER NOT NULL
) STRICT;
CREATE INDEX bots_created ON bots(created_at, id);
