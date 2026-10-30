CREATE TABLE credential_authorities (
    use_id TEXT PRIMARY KEY REFERENCES credential_uses(id),
    source TEXT NOT NULL CHECK(json_valid(source))
) STRICT;
PRAGMA user_version = 15;
