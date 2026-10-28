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
    binding TEXT NOT NULL,
    UNIQUE(routine,trigger_key)
) STRICT;
CREATE INDEX routine_occurrences_history ON routine_occurrences(routine,scheduled_at);
CREATE INDEX routine_occurrences_status ON routine_occurrences(status);
CREATE TABLE routine_commands (id TEXT PRIMARY KEY, request TEXT NOT NULL, result TEXT NOT NULL) STRICT;
CREATE TABLE routine_budget (id INTEGER PRIMARY KEY CHECK(id=1), view TEXT NOT NULL) STRICT;
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
PRAGMA user_version = 10;
