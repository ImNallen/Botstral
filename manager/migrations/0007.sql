CREATE TABLE coding_leases (
    task TEXT PRIMARY KEY REFERENCES tasks(id),
    execution TEXT NOT NULL,
    generation INTEGER NOT NULL,
    closed INTEGER NOT NULL DEFAULT 0 CHECK(closed IN (0,1))
) STRICT;
CREATE TABLE coding_calls (
    id TEXT PRIMARY KEY,
    task TEXT NOT NULL REFERENCES tasks(id),
    execution TEXT NOT NULL,
    generation INTEGER NOT NULL,
    request TEXT NOT NULL CHECK(length(request)<=262144),
    configuration TEXT NOT NULL,
    view TEXT NOT NULL,
    response TEXT
) STRICT;
CREATE TRIGGER immutable_coding_call BEFORE UPDATE OF id,task,execution,generation,request,configuration ON coding_calls
BEGIN SELECT RAISE(ABORT, 'immutable coding call'); END;
ALTER TABLE inference_configurations ADD COLUMN verified_vision INTEGER NOT NULL DEFAULT 0 CHECK(verified_vision IN (0,1));
PRAGMA user_version = 7;
