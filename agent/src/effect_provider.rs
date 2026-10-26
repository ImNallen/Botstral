use botstral_link::*;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::{io, path::Path};

pub(crate) struct Provider(Connection);
impl Provider {
    pub fn open(directory: &Path) -> io::Result<Self> {
        let path = directory.join("synthetic-provider.sqlite3");
        for name in [
            "synthetic-provider.sqlite3",
            "synthetic-provider.sqlite3-wal",
            "synthetic-provider.sqlite3-shm",
        ] {
            let file = directory.join(name);
            if std::fs::symlink_metadata(&file).is_ok() {
                tls::check_private_file(&file)?;
            }
        }
        let db = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(io::Error::other)?;
        db.pragma_update(None, "journal_mode", "WAL")
            .map_err(io::Error::other)?;
        db.pragma_update(None, "synchronous", "FULL")
            .map_err(io::Error::other)?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS effects(operation TEXT PRIMARY KEY, body TEXT NOT NULL, applied INTEGER NOT NULL, invocations INTEGER NOT NULL) STRICT;").map_err(io::Error::other)?;
        Ok(Self(db))
    }
    pub fn apply(&mut self, body: &CommandBody) -> io::Result<u32> {
        let value = match body.work {
            WorkSpec::TestEffect { value, .. } => value,
            WorkSpec::TestInput { steps } => steps,
            _ => return Err(io::Error::other("unavailable provider")),
        };
        self.0.execute("INSERT INTO effects VALUES(?1,?2,?3,1) ON CONFLICT(operation) DO UPDATE SET invocations=invocations+1",params![body.operation.to_string(),serde_json::to_string(body)?,value]).map_err(io::Error::other)?;
        Ok(value)
    }
    pub fn observe(&self, body: &CommandBody) -> io::Result<Option<u32>> {
        if matches!(
            body.work,
            WorkSpec::TestEffect {
                observable: false,
                ..
            }
        ) {
            return Ok(None);
        }
        self.0
            .query_row(
                "SELECT applied FROM effects WHERE operation=?1 AND body=?2",
                params![body.operation.to_string(), serde_json::to_string(body)?],
                |r| r.get(0),
            )
            .optional()
            .map_err(io::Error::other)
    }
}
