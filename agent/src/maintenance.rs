use botstral_link::{JournalId, ProcessAccount, ProcessLimits, ProcessProfile};
use std::{
    fs, io,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::Path,
};

pub async fn check_supervisor(scratch: &Path) -> io::Result<()> {
    if !cfg!(target_os = "linux") || unsafe { libc::geteuid() } == 0 {
        return Err(io::Error::other(
            "nonroot Linux systemd 255 account required",
        ));
    }
    let parent = scratch
        .parent()
        .ok_or_else(|| io::Error::other("scratch parent required"))?;
    if !scratch.is_absolute() || parent.canonicalize()? != parent || scratch.file_name().is_none() {
        return Err(io::Error::other("canonical fresh scratch path required"));
    }
    fs::DirBuilder::new().mode(0o700).create(scratch)?;
    let result = async {
        let metadata = fs::symlink_metadata(scratch)?;
        if !metadata.is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
            return Err(io::Error::other("unsafe scratch directory"));
        }
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(scratch.join("agent.lock"))?;
        lock.try_lock()?;
        let configuration = crate::process::Configuration {
            profile: ProcessProfile::LinuxSystemdV1,
            account: ProcessAccount {
                uid: unsafe { libc::geteuid() },
                gid: unsafe { libc::getegid() },
            },
            ceilings: ProcessLimits {
                runtime_seconds: 10,
                output_bytes: 1024,
                processes: 8,
                memory_bytes: 64 * 1024 * 1024,
            },
        };
        crate::process::Backend::probe(scratch, JournalId::new(), configuration).await?;
        Ok(())
    }
    .await;
    if result.is_ok() {
        fs::remove_dir_all(scratch)?;
    }
    result
}
