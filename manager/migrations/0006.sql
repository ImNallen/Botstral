ALTER TABLE inference_configurations ADD COLUMN verified_tools INTEGER NOT NULL DEFAULT 0 CHECK(verified_tools IN (0,1));
CREATE TABLE runs (
    id TEXT PRIMARY KEY,
    owner TEXT NOT NULL,
    request TEXT NOT NULL,
    configuration TEXT NOT NULL,
    summary TEXT NOT NULL
) STRICT;
CREATE TABLE run_turns (
    id TEXT PRIMARY KEY,
    run TEXT NOT NULL REFERENCES runs(id),
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 1 AND 8),
    attempt TEXT NOT NULL UNIQUE,
    input TEXT NOT NULL,
    reservation TEXT NOT NULL,
    view TEXT NOT NULL,
    UNIQUE(run, ordinal),
    UNIQUE(run, id)
) STRICT;
CREATE TABLE run_children (
    turn TEXT PRIMARY KEY REFERENCES run_turns(id),
    run TEXT NOT NULL REFERENCES runs(id),
    call_id TEXT NOT NULL,
    task TEXT NOT NULL UNIQUE REFERENCES tasks(id),
    UNIQUE(run, call_id),
    FOREIGN KEY(run, turn) REFERENCES run_turns(run, id)
) STRICT;
CREATE TRIGGER immutable_run BEFORE UPDATE OF id,owner,request,configuration ON runs
BEGIN SELECT RAISE(ABORT, 'immutable run'); END;
CREATE TRIGGER immutable_run_turn BEFORE UPDATE OF id,run,ordinal,attempt,input,reservation ON run_turns
BEGIN SELECT RAISE(ABORT, 'immutable turn'); END;
CREATE TRIGGER immutable_run_child BEFORE UPDATE ON run_children
BEGIN SELECT RAISE(ABORT, 'immutable child'); END;
PRAGMA user_version = 6;
