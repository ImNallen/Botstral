CREATE TABLE bot_tombstones (bot TEXT PRIMARY KEY REFERENCES bots(id), deleted_at INTEGER NOT NULL) STRICT;
CREATE TABLE routines (
    id TEXT PRIMARY KEY,
    bot TEXT NOT NULL REFERENCES bots(id),
    revision INTEGER NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('enabled','paused','deleted')),
    next_at INTEGER,
    view TEXT NOT NULL CHECK(length(view)<=32768)
) STRICT;
CREATE INDEX routines_due ON routines(state,next_at);
CREATE TABLE routine_occurrences (
    id TEXT PRIMARY KEY,
    routine TEXT NOT NULL REFERENCES routines(id),
    revision INTEGER NOT NULL,
    trigger_key TEXT NOT NULL,
    scheduled_at INTEGER NOT NULL,
    run TEXT UNIQUE REFERENCES runs(id),
    status TEXT NOT NULL,
    view TEXT NOT NULL,
    prompt TEXT,
    binding TEXT NOT NULL CHECK(length(binding)<=1024),
    UNIQUE(routine,trigger_key)
) STRICT;
CREATE INDEX routine_occurrences_history ON routine_occurrences(routine,scheduled_at);
CREATE INDEX routine_occurrences_status ON routine_occurrences(status);
CREATE TABLE routine_commands (id TEXT PRIMARY KEY, request TEXT NOT NULL, result TEXT NOT NULL) STRICT;
CREATE TABLE routine_budget (
    id INTEGER PRIMARY KEY CHECK(id=1), view TEXT NOT NULL,
    admitted_runs INTEGER NOT NULL DEFAULT 0 CHECK(admitted_runs>=0),
    tokens INTEGER NOT NULL DEFAULT 0 CHECK(tokens>=0),
    cost INTEGER NOT NULL DEFAULT 0 CHECK(cost>=0)
) STRICT;
CREATE TABLE routine_reservations (
    run TEXT PRIMARY KEY REFERENCES runs(id), routine TEXT NOT NULL REFERENCES routines(id),
    tokens INTEGER NOT NULL, cost INTEGER NOT NULL
) STRICT;
CREATE TABLE routine_publishers (
    id TEXT PRIMARY KEY, bot TEXT NOT NULL REFERENCES bots(id), credential_hash BLOB NOT NULL,
    view TEXT NOT NULL, window_start INTEGER NOT NULL DEFAULT 0, received INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE routine_event_receipts (
    publisher TEXT NOT NULL REFERENCES routine_publishers(id), event_id TEXT NOT NULL,
    digest BLOB NOT NULL, timestamp INTEGER NOT NULL, receipt TEXT NOT NULL,
    PRIMARY KEY(publisher,event_id)
) STRICT;
CREATE TABLE routine_inbox (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT NOT NULL UNIQUE,
    occurrence TEXT NOT NULL REFERENCES routine_occurrences(id), transition TEXT NOT NULL,
    acknowledged INTEGER NOT NULL DEFAULT 0, view TEXT NOT NULL,
    UNIQUE(occurrence,transition)
) STRICT;
CREATE TABLE routine_wakeups (
    parent_run TEXT NOT NULL REFERENCES runs(id), turn TEXT NOT NULL REFERENCES run_turns(id),
    call_id TEXT NOT NULL, occurrence TEXT NOT NULL REFERENCES routine_occurrences(id),
    PRIMARY KEY(parent_run,call_id)
) STRICT;
CREATE INDEX routine_publishers_active ON routine_publishers(json_extract(view,'$.revoked'));
CREATE INDEX runs_active_phase ON runs(json_extract(summary,'$.phase.type'));
CREATE INDEX tasks_active_phase ON tasks(json_extract(view,'$.state'));
CREATE INDEX routine_occurrences_active ON routine_occurrences(routine,status);
CREATE INDEX routine_occurrences_due ON routine_occurrences(status,scheduled_at);
CREATE TABLE run_turn_receipts (
    turn TEXT PRIMARY KEY,
    run TEXT NOT NULL REFERENCES runs(id),
    attempt TEXT NOT NULL UNIQUE,
    receipt TEXT NOT NULL CHECK(length(receipt)<=32768)
) STRICT;
CREATE TRIGGER immutable_run_turn_receipt BEFORE UPDATE ON run_turn_receipts
BEGIN SELECT RAISE(ABORT,'immutable run turn receipt'); END;
PRAGMA user_version = 10;
