use super::*;
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

struct FileRow {
    name: String,
    sha256: String,
    length: i64,
}

pub(super) struct AttachmentOffer {
    pub(super) inline: Option<String>,
    pub(super) retained: String,
}

fn invalid() -> OperationError {
    OperationError::InvalidInput
}

fn storage(_: std::io::Error) -> OperationError {
    OperationError::Internal
}

fn validate_name(name: &str) -> Result<(), OperationError> {
    if !(1..=120).contains(&name.len())
        || name.contains(['/', '\\', '\0'])
        || name == "."
        || name == ".."
        || Path::new(name).file_name().and_then(|part| part.to_str()) != Some(name)
    {
        return Err(invalid());
    }
    Ok(())
}

fn decode_hex(hex: &str) -> Result<Vec<u8>, OperationError> {
    if hex.is_empty() || !hex.len().is_multiple_of(2) || hex.len() > MAX_RUN_FILE_BYTES * 2 {
        return Err(invalid());
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let digits = hex.as_bytes();
    for pair in digits.chunks_exact(2) {
        let high = nibble(pair[0])?;
        let low = nibble(pair[1])?;
        bytes.push((high << 4) | low);
    }
    Ok(bytes)
}

fn nibble(byte: u8) -> Result<u8, OperationError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(invalid()),
    }
}

fn component(id: RunId) -> Result<String, OperationError> {
    let name = id.to_string();
    if name.is_empty() || name.contains(['/', '\\', '\0']) {
        return Err(invalid());
    }
    Ok(name)
}

fn directory(root: &Path) -> PathBuf {
    root.join("run-files")
}

fn blob_path(root: &Path, id: RunId) -> Result<PathBuf, OperationError> {
    Ok(directory(root).join(component(id)?))
}

fn ensure_directory(root: &Path) -> Result<PathBuf, OperationError> {
    let dir = directory(root);
    match fs::symlink_metadata(&dir) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
            return Err(OperationError::Conflict);
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&dir)
                .map_err(storage)?;
        }
        Err(_) => return Err(OperationError::Internal),
    }
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).map_err(storage)?;
    Ok(dir)
}

fn read_blob(path: &Path) -> Result<Vec<u8>, OperationError> {
    let meta = fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            OperationError::NotFound
        } else {
            OperationError::Internal
        }
    })?;
    if meta.file_type().is_symlink() || !meta.is_file() || meta.nlink() != 1 {
        return Err(OperationError::Conflict);
    }
    if meta.len() == 0 || meta.len() > MAX_RUN_FILE_BYTES as u64 {
        return Err(OperationError::Conflict);
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(storage)?;
    let opened = file.metadata().map_err(storage)?;
    if !opened.is_file() || opened.nlink() != 1 || opened.len() != meta.len() {
        return Err(OperationError::Conflict);
    }
    let mut bytes = Vec::new();
    file.take(MAX_RUN_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(storage)?;
    if bytes.len() as u64 != meta.len() {
        return Err(OperationError::Conflict);
    }
    Ok(bytes)
}

fn write_blob(root: &Path, id: RunId, bytes: &[u8]) -> Result<(), OperationError> {
    let dir = ensure_directory(root)?;
    let name = component(id)?;
    let dest = dir.join(&name);
    let temp = dir.join(format!("{name}.part"));
    if fs::symlink_metadata(&temp).is_ok_and(|meta| meta.file_type().is_symlink())
        || fs::symlink_metadata(&dest).is_ok_and(|meta| meta.file_type().is_symlink())
    {
        return Err(OperationError::Conflict);
    }
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temp)
            .map_err(storage)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(storage)?;
        file.write_all(bytes).map_err(storage)?;
        file.sync_all().map_err(storage)?;
    }
    fs::rename(&temp, &dest).map_err(storage)?;
    fs::File::open(&dir)
        .and_then(|dir| dir.sync_all())
        .map_err(storage)?;
    let stored = read_blob(&dest)?;
    if stored != bytes {
        let _ = fs::remove_file(&dest);
        return Err(OperationError::Conflict);
    }
    Ok(())
}

fn row(db: &Connection, staged: bool, id: RunId) -> Result<Option<FileRow>, OperationError> {
    let sql = if staged {
        "SELECT name, sha256, length FROM staged_run_files WHERE run=?1"
    } else {
        "SELECT name, sha256, length FROM run_files WHERE run=?1"
    };
    db.query_row(sql, [id.to_string()], |record| {
        Ok(FileRow {
            name: record.get(0)?,
            sha256: record.get(1)?,
            length: record.get(2)?,
        })
    })
    .optional()
    .map_err(|_| OperationError::Internal)
}

fn same(row: &FileRow, name: &str, sha256: &str, length: i64) -> bool {
    row.name == name && row.sha256 == sha256 && row.length == length
}

fn blob_is(root: &Path, id: RunId, bytes: &[u8]) -> Result<bool, OperationError> {
    let path = blob_path(root, id)?;
    match read_blob(&path) {
        Ok(stored) => Ok(stored == bytes),
        Err(OperationError::NotFound) => Ok(false),
        Err(error) => Err(error),
    }
}

fn retained(name: &str, sha256: &str, length: i64) -> String {
    format!(
        "Untrusted attached file {name} is retained. sha256 {sha256}. length {length}. The bytes are not inlined."
    )
}

fn inline_body(
    state: &State,
    name: &str,
    sha256: &str,
    bytes: &[u8],
) -> Result<Option<String>, OperationError> {
    if bytes.len() > MAX_RUN_TEXT_BYTES || bytes.contains(&0) {
        return Ok(None);
    }
    let Ok(body) = std::str::from_utf8(bytes) else {
        return Ok(None);
    };
    let owned = body.to_owned();
    match state.reject_known_secrets(&owned) {
        Ok(()) => Ok(Some(format!(
            "Untrusted attached file {name} sha256 {sha256}:\n{body}"
        ))),
        Err(OperationError::InvalidInput) => Ok(None),
        Err(error) => Err(error),
    }
}

pub(super) fn attachment_offer(
    state: &State,
    id: RunId,
) -> Result<Option<AttachmentOffer>, OperationError> {
    let (row, staged) = if let Some(row) = row(&state.connection, false, id)? {
        (row, false)
    } else if let Some(row) = row(&state.connection, true, id)? {
        (row, true)
    } else {
        return Ok(None);
    };
    let path = blob_path(&state.directory, id)?;
    let bytes = match read_blob(&path) {
        Ok(bytes)
            if bytes.len() as i64 == row.length
                && CertificateFingerprint::from_der(&bytes).as_str() == row.sha256 =>
        {
            Some(bytes)
        }
        Ok(_) | Err(OperationError::NotFound | OperationError::Conflict) if staged => {
            return Err(invalid());
        }
        Ok(_) | Err(OperationError::NotFound | OperationError::Conflict) => None,
        Err(error) => return Err(error),
    };
    let inline = match &bytes {
        Some(bytes) => inline_body(state, &row.name, &row.sha256, bytes)?,
        None => None,
    };
    Ok(Some(AttachmentOffer {
        inline,
        retained: retained(&row.name, &row.sha256, row.length),
    }))
}

impl State {
    pub(in crate::manager) fn stage_run_file(
        &mut self,
        token: impl Into<OwnerAuthority>,
        input: StageRunFile,
    ) -> Result<(), OperationError> {
        self.authorize_owner(&token.into())?;
        validate_name(&input.name)?;
        let bytes = decode_hex(&input.hex)?;
        let digest = CertificateFingerprint::from_der(&bytes);
        if digest != input.sha256 {
            return Err(invalid());
        }
        let sha256 = digest.as_str().to_string();
        let length = i64::try_from(bytes.len()).map_err(|_| invalid())?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = row(&tx, false, input.id)? {
            if same(&existing, &input.name, &sha256, length)
                && blob_is(&self.directory, input.id, &bytes)?
            {
                tx.commit()?;
                return Ok(());
            }
            return Err(OperationError::Conflict);
        }
        let committed: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM runs WHERE id=?1)",
            [input.id.to_string()],
            |record| record.get(0),
        )?;
        if committed {
            return Err(OperationError::Conflict);
        }
        if let Some(existing) = row(&tx, true, input.id)? {
            if same(&existing, &input.name, &sha256, length)
                && blob_is(&self.directory, input.id, &bytes)?
            {
                tx.commit()?;
                return Ok(());
            }
            return Err(OperationError::Conflict);
        }
        write_blob(&self.directory, input.id, &bytes)?;
        tx.execute(
            "INSERT INTO staged_run_files(run, name, sha256, length) VALUES(?1,?2,?3,?4)",
            params![input.id.to_string(), input.name, sha256, length],
        )?;
        tx.commit()?;
        Ok(())
    }
}

pub(super) fn bind_staged_run_file(
    directory: &Path,
    db: &Connection,
    id: RunId,
) -> Result<(), OperationError> {
    let Some(row) = row(db, true, id)? else {
        return Ok(());
    };
    let bytes = match read_blob(&blob_path(directory, id)?) {
        Ok(bytes) => bytes,
        Err(OperationError::NotFound | OperationError::Conflict) => {
            return Err(OperationError::Conflict);
        }
        Err(error) => return Err(error),
    };
    if bytes.len() as i64 != row.length
        || CertificateFingerprint::from_der(&bytes).as_str() != row.sha256
    {
        return Err(OperationError::Conflict);
    }
    db.execute(
        "INSERT INTO run_files(run, name, sha256, length) VALUES(?1,?2,?3,?4)",
        params![id.to_string(), row.name, row.sha256, row.length],
    )?;
    db.execute(
        "DELETE FROM staged_run_files WHERE run=?1",
        [id.to_string()],
    )?;
    Ok(())
}
