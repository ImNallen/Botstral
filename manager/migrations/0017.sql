CREATE TABLE staged_run_files (
    run TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    length INTEGER NOT NULL
) STRICT;
CREATE TABLE run_files (
    run TEXT PRIMARY KEY REFERENCES runs(id),
    name TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    length INTEGER NOT NULL
) STRICT;
PRAGMA user_version = 17;
