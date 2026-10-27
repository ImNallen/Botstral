CREATE TABLE inference_authority (
    id INTEGER PRIMARY KEY CHECK(id = 1),
    provider_epoch INTEGER NOT NULL CHECK(provider_epoch >= 0)
) STRICT;
INSERT INTO inference_authority VALUES(1, 0);
CREATE TABLE inference_configurations (
    bot TEXT NOT NULL REFERENCES bots(id),
    revision INTEGER NOT NULL CHECK(revision > 0),
    policy TEXT NOT NULL,
    verified_at INTEGER,
    verified_until INTEGER,
    provider_epoch INTEGER,
    PRIMARY KEY(bot, revision)
) STRICT;
CREATE TRIGGER immutable_inference_configuration BEFORE UPDATE OF bot, revision, policy ON inference_configurations
BEGIN SELECT RAISE(ABORT, 'immutable inference configuration'); END;
CREATE TABLE inferences (
    id TEXT PRIMARY KEY,
    bot TEXT NOT NULL,
    configuration_revision INTEGER NOT NULL,
    request TEXT NOT NULL,
    view TEXT NOT NULL,
    phase TEXT NOT NULL CHECK(phase IN ('admitted','sending','completed','cancelled','not_sent','awaiting_attention')),
    attempt TEXT UNIQUE,
    FOREIGN KEY(bot, configuration_revision) REFERENCES inference_configurations(bot, revision)
) STRICT;
CREATE INDEX inferences_phase ON inferences(phase);
CREATE TRIGGER immutable_inference_request BEFORE UPDATE OF id, bot, configuration_revision, request ON inferences
BEGIN SELECT RAISE(ABORT, 'immutable inference request'); END;
PRAGMA user_version = 5;
