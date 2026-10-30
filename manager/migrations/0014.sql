ALTER TABLE connector_leases RENAME TO execution_leases;
CREATE TABLE credential_uses (
    id TEXT PRIMARY KEY,
    release TEXT NOT NULL UNIQUE,
    view TEXT NOT NULL
) STRICT;
CREATE TABLE credential_rules (
    item TEXT NOT NULL REFERENCES credential_items(id),
    bot TEXT NOT NULL REFERENCES bots(id),
    contract TEXT NOT NULL,
    decision TEXT NOT NULL CHECK(decision IN ('allow','deny')),
    cause TEXT NOT NULL,
    PRIMARY KEY(item,bot,contract)
) STRICT;
CREATE TABLE credential_codes (
    release TEXT PRIMARY KEY REFERENCES credential_uses(release),
    ciphertext BLOB NOT NULL
) STRICT;
CREATE TRIGGER retained_credential_code BEFORE DELETE ON credential_codes
BEGIN SELECT RAISE(ABORT, 'retained issued code'); END;
CREATE TRIGGER immutable_credential_code BEFORE UPDATE ON credential_codes
BEGIN SELECT RAISE(ABORT, 'immutable issued code'); END;
PRAGMA user_version = 14;
