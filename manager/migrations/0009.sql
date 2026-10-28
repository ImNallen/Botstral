CREATE TABLE connections (id TEXT PRIMARY KEY, view TEXT NOT NULL);
CREATE TABLE connection_versions (
    connection TEXT NOT NULL REFERENCES connections(id), revision INTEGER NOT NULL,
    view TEXT NOT NULL, PRIMARY KEY(connection,revision)
);
CREATE TABLE connection_secrets (
    connection TEXT NOT NULL REFERENCES connections(id), epoch INTEGER NOT NULL,
    ciphertext BLOB NOT NULL, PRIMARY KEY(connection,epoch)
);
CREATE TABLE connector_catalogs (
    connection TEXT NOT NULL REFERENCES connections(id), revision INTEGER NOT NULL,
    tools TEXT NOT NULL, PRIMARY KEY(connection,revision)
);
CREATE TABLE connector_grants (
    bot TEXT NOT NULL REFERENCES bots(id), connection TEXT NOT NULL REFERENCES connections(id),
    revision INTEGER NOT NULL, view TEXT NOT NULL, PRIMARY KEY(bot,connection,revision)
);
CREATE TABLE connector_leases (
    task TEXT PRIMARY KEY REFERENCES tasks(id), execution TEXT NOT NULL,
    generation INTEGER NOT NULL, fence TEXT NOT NULL
);
CREATE TABLE connector_calls (
    id TEXT PRIMARY KEY, task TEXT NOT NULL REFERENCES tasks(id), execution TEXT NOT NULL,
    generation INTEGER NOT NULL, operation TEXT NOT NULL, view TEXT NOT NULL,
    deadline INTEGER NOT NULL, max_result_bytes INTEGER NOT NULL
);
CREATE INDEX connector_calls_task ON connector_calls(task);
CREATE TABLE connector_audit (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT, connection TEXT NOT NULL,
    call TEXT, code TEXT NOT NULL, digest TEXT, at INTEGER NOT NULL
);
CREATE TRIGGER immutable_connection_version BEFORE UPDATE ON connection_versions
BEGIN SELECT RAISE(ABORT, 'immutable connection version'); END;
CREATE TRIGGER retain_connection_version BEFORE DELETE ON connection_versions
BEGIN SELECT RAISE(ABORT, 'immutable connection version'); END;
CREATE TRIGGER immutable_connection_secret BEFORE UPDATE ON connection_secrets
BEGIN SELECT RAISE(ABORT, 'immutable credential epoch'); END;
CREATE TRIGGER retain_connection_secret BEFORE DELETE ON connection_secrets
BEGIN SELECT RAISE(ABORT, 'immutable credential epoch'); END;
CREATE TRIGGER immutable_connector_catalog BEFORE UPDATE ON connector_catalogs
BEGIN SELECT RAISE(ABORT, 'immutable connector catalog'); END;
CREATE TRIGGER retain_connector_catalog BEFORE DELETE ON connector_catalogs
BEGIN SELECT RAISE(ABORT, 'immutable connector catalog'); END;
CREATE TRIGGER immutable_connector_grant BEFORE UPDATE ON connector_grants
BEGIN SELECT RAISE(ABORT, 'immutable connector grant'); END;
CREATE TRIGGER retain_connector_grant BEFORE DELETE ON connector_grants
BEGIN SELECT RAISE(ABORT, 'immutable connector grant'); END;
CREATE TRIGGER immutable_connector_call BEFORE UPDATE OF id,task,execution,generation,operation,deadline,max_result_bytes ON connector_calls
BEGIN SELECT RAISE(ABORT, 'immutable connector invocation'); END;
PRAGMA user_version = 9;
