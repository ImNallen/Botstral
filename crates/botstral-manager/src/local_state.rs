use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub struct LocalState {
    directory: PathBuf,
    _lock: File,
}

impl LocalState {
    pub fn open(directory: &Path) -> io::Result<Self> {
        match fs::symlink_metadata(directory) {
            Ok(metadata) => {
                check_metadata(&metadata, true)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::DirBuilder::new().mode(0o700).create(directory)?;
            }
            Err(error) => return Err(error),
        }
        let directory = directory.canonicalize()?;
        check_metadata(&fs::symlink_metadata(&directory)?, true)?;
        for entry in fs::read_dir(&directory)? {
            check_metadata(&fs::symlink_metadata(entry?.path())?, false)?;
        }
        let lock = private_file(&directory.join("manager.lock"))?;
        lock.try_lock().map_err(io::Error::other)?;
        private_file(&directory.join("manager.sqlite3"))?;
        Ok(Self {
            directory,
            _lock: lock,
        })
    }

    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }

    pub(crate) fn write(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let temporary = self.directory.join(format!(".{name}.tmp"));
        let mut file = private_file(&temporary)?;
        file.set_len(0)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(temporary, self.directory.join(name))
    }

    pub(crate) fn remove(&self, name: &str) -> io::Result<()> {
        match fs::remove_file(self.directory.join(name)) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }
}

fn private_file(path: &Path) -> io::Result<File> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        check_metadata(&metadata, false)?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    check_metadata(&file.metadata()?, false)?;
    Ok(file)
}

fn check_metadata(metadata: &fs::Metadata, directory: bool) -> io::Result<()> {
    let owned = metadata.uid() == unsafe { libc::geteuid() };
    let kind = if directory {
        metadata.is_dir()
    } else {
        metadata.is_file() && metadata.nlink() == 1
    };
    let mode = metadata.permissions().mode();
    let expected = if directory { 0o700 } else { 0o600 };
    if !owned || !kind || mode & 0o7777 != expected {
        return Err(io::Error::other("unsafe local state"));
    }
    Ok(())
}

pub fn protect_process() -> io::Result<()> {
    // These process-wide settings must precede runtime threads and state creation.
    unsafe {
        libc::umask(0o077);
        let limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if libc::setrlimit(libc::RLIMIT_CORE, &limit) != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn rejects_unsafe_state_and_second_owner() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state");
        let state = LocalState::open(&path).unwrap();
        assert!(LocalState::open(&path).is_err());
        drop(state);
        let db = path.join("manager.sqlite3");
        fs::set_permissions(&db, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(LocalState::open(&path).is_err());
        fs::remove_file(&db).unwrap();
        symlink(temp.path().join("elsewhere"), &db).unwrap();
        assert!(LocalState::open(&path).is_err());
        let link = temp.path().join("linked-state");
        symlink(&path, &link).unwrap();
        assert!(LocalState::open(&link).is_err());
    }
}
