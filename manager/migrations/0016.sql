CREATE TABLE credential_prepare_attempts (
    task TEXT PRIMARY KEY REFERENCES tasks(id),
    execution TEXT NOT NULL
) STRICT;
CREATE TRIGGER immutable_credential_prepare_attempt BEFORE UPDATE ON credential_prepare_attempts
BEGIN SELECT RAISE(ABORT, 'immutable credential prepare attempt'); END;
CREATE TRIGGER retained_credential_prepare_attempt BEFORE DELETE ON credential_prepare_attempts
BEGIN SELECT RAISE(ABORT, 'retained credential prepare attempt'); END;
PRAGMA user_version = 16;
