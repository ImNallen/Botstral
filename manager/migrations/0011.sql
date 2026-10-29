ALTER TABLE runs ADD COLUMN memory_snapshot TEXT;
DROP TRIGGER immutable_run;
CREATE TRIGGER immutable_run BEFORE UPDATE OF id,owner,request,configuration,memory_snapshot ON runs
BEGIN SELECT RAISE(ABORT, 'immutable run'); END;
PRAGMA user_version = 11;
