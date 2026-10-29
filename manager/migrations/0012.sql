ALTER TABLE runs ADD COLUMN memory_entries_snapshot TEXT;
DROP TRIGGER immutable_run;
CREATE TRIGGER immutable_run BEFORE UPDATE OF id,owner,request,configuration,memory_snapshot,memory_entries_snapshot ON runs
BEGIN SELECT RAISE(ABORT, 'immutable run'); END;

CREATE TABLE memory_publications (
    turn TEXT PRIMARY KEY,
    run TEXT NOT NULL REFERENCES runs(id),
    attempt TEXT NOT NULL,
    call TEXT NOT NULL,
    entry TEXT NOT NULL,
    expected_digest TEXT,
    intended_bytes BLOB NOT NULL,
    receipt TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('pending','applied','attention')),
    UNIQUE(run,call)
) STRICT;
CREATE TRIGGER immutable_memory_publication BEFORE UPDATE OF turn,run,attempt,call,entry,expected_digest,intended_bytes,receipt ON memory_publications
BEGIN SELECT RAISE(ABORT, 'immutable memory publication'); END;

CREATE TABLE memory_imports (
    bot TEXT PRIMARY KEY,
    expected_digest TEXT NOT NULL,
    intended_bytes BLOB,
    state TEXT NOT NULL CHECK(state IN ('pending','applied'))
) STRICT;
CREATE TRIGGER immutable_memory_import BEFORE UPDATE OF bot,expected_digest,intended_bytes ON memory_imports
BEGIN SELECT RAISE(ABORT, 'immutable memory import'); END;
PRAGMA user_version = 12;
