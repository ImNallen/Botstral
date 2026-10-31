CREATE TABLE join_invitations (
    token_hash BLOB PRIMARY KEY CHECK(length(token_hash) = 32),
    role TEXT NOT NULL CHECK(role IN ('worker', 'device')),
    label TEXT,
    expires_at INTEGER NOT NULL,
    consumed_at INTEGER,
    bound_identity TEXT CHECK(bound_identity IS NULL OR length(bound_identity) = 64),
    CHECK(
        (role = 'worker' AND label IS NULL)
        OR (role = 'device' AND label IS NOT NULL AND length(label) BETWEEN 1 AND 100)
    ),
    CHECK(
        (consumed_at IS NULL AND bound_identity IS NULL)
        OR (consumed_at IS NOT NULL AND bound_identity IS NOT NULL)
    )
) STRICT;
ALTER TABLE workers ADD COLUMN second_key BLOB CHECK(second_key IS NULL);
ALTER TABLE owner_devices ADD COLUMN second_key BLOB CHECK(second_key IS NULL);
PRAGMA user_version = 18;
