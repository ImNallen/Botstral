CREATE TABLE artifacts (
    id TEXT PRIMARY KEY,
    task TEXT NOT NULL REFERENCES tasks(id),
    worker TEXT NOT NULL REFERENCES workers(id),
    journal TEXT NOT NULL,
    execution TEXT NOT NULL,
    slot TEXT NOT NULL,
    reserved INTEGER NOT NULL CHECK(reserved>=0),
    offer TEXT,
    offset INTEGER NOT NULL DEFAULT 0 CHECK(offset>=0),
    state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN ('pending','transferring','available','unavailable')),
    UNIQUE(execution,slot)
) STRICT;
PRAGMA user_version = 4;
