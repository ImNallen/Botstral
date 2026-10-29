CREATE TABLE credential_items (
    id TEXT PRIMARY KEY,
    view TEXT NOT NULL
) STRICT;
CREATE TABLE credential_epochs (
    item TEXT NOT NULL REFERENCES credential_items(id),
    epoch INTEGER NOT NULL CHECK(epoch > 0),
    ciphertext BLOB NOT NULL,
    PRIMARY KEY(item, epoch)
) STRICT;
CREATE TABLE credential_writes (
    request TEXT PRIMARY KEY,
    status TEXT NOT NULL
) STRICT;
CREATE TRIGGER immutable_credential_epoch BEFORE UPDATE ON credential_epochs
BEGIN SELECT RAISE(ABORT, 'immutable credential epoch'); END;
CREATE TRIGGER retained_credential_epoch BEFORE DELETE ON credential_epochs
BEGIN SELECT RAISE(ABORT, 'retained credential epoch'); END;
CREATE TRIGGER immutable_credential_write BEFORE UPDATE ON credential_writes
BEGIN SELECT RAISE(ABORT, 'immutable credential receipt'); END;
CREATE TRIGGER retained_credential_write BEFORE DELETE ON credential_writes
BEGIN SELECT RAISE(ABORT, 'retained credential receipt'); END;
PRAGMA user_version = 13;
