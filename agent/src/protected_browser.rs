use crate::{process, transport};
use botstral_link::{protected::*, *};
use http_body_util::{BodyExt, Full, Limited};
use hyper::{Request, body::Bytes};
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};
use zeroize::Zeroizing;

pub(crate) struct Scope {
    pub authority: BrowserAuthority,
    pub scope: ProcessScope,
    pub profile: PathBuf,
    pub directory: PathBuf,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct History {
    journal: JournalId,
    protection: ProfileProtection,
    consumed: Vec<ReleaseId>,
}
fn path(profile: &Path) -> io::Result<PathBuf> {
    Ok(profile
        .parent()
        .ok_or_else(|| io::Error::other("profile root"))?
        .join("automation-protection.json"))
}
pub(crate) fn initialize(profile: &Path, journal: JournalId) -> io::Result<()> {
    if path(profile)?.exists() {
        let history = reconciled_history(profile, journal)?;
        return process::write(&path(profile)?, &history);
    }
    let complete = !profile.exists() || fs::read_dir(profile)?.next().is_none();
    let epoch = ProfileEpoch::new();
    fs::create_dir_all(profile)?;
    fs::set_permissions(profile, fs::Permissions::from_mode(0o700))?;
    process::write(&profile.join(".botstral-profile.json"), &epoch)?;
    process::write(
        &path(profile)?,
        &History {
            journal,
            protection: ProfileProtection::Tracked {
                profile: epoch,
                releases: vec![],
                complete,
            },
            consumed: vec![],
        },
    )
}
fn reconciled_history(profile: &Path, journal: JournalId) -> io::Result<History> {
    let mut history: History = process::read(&path(profile)?)?;
    let marker: ProfileEpoch = process::read(&profile.join(".botstral-profile.json"))?;
    let ProfileProtection::Tracked {
        profile, complete, ..
    } = &mut history.protection
    else {
        return Err(io::Error::other("unknown profile"));
    };
    if *profile != marker || history.journal != journal {
        *complete = false;
    }
    Ok(history)
}
pub(crate) async fn exchange(
    config: &crate::Config,
    request: ProtectedBrowserRequest,
) -> io::Result<ProtectedBrowserReply> {
    let bytes = Zeroizing::new(serde_json::to_vec(&request)?);
    if bytes.len() > MAX_MESSAGE {
        return Err(io::Error::other("protected frame bound"));
    }
    let stream = transport::connect_tls(config).await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .map_err(io::Error::other)?;
        let request = Request::builder()
            .method("POST")
            .uri("/worker/v1/protected-browser")
            .header("host", config.manager.trim_start_matches("wss://"))
            .header("content-type", "application/octet-stream")
            .header("connection", "close")
            .body(Full::new(Bytes::copy_from_slice(&bytes)))
            .map_err(io::Error::other)?;
        let response = async {
            let response = sender
                .send_request(request)
                .await
                .map_err(io::Error::other)?;
            if response.status() != 200 {
                return Err(io::Error::other("protected operation refused"));
            }
            let bytes = Limited::new(response.into_body(), MAX_MESSAGE)
                .collect()
                .await
                .map_err(io::Error::other)?
                .to_bytes();
            let bytes = Zeroizing::new(bytes.to_vec());
            serde_json::from_slice(&bytes)
                .map_err(|_| io::Error::other("protected response invalid"))
        };
        let (result, _) = tokio::join!(response, connection);
        result
    })
    .await
    .map_err(|_| io::Error::other("protected response unresolved"))?
}

pub(crate) async fn handle(
    config: &crate::Config,
    scope: Scope,
    mut stream: UnixStream,
) -> io::Result<()> {
    let peer = stream.peer_cred()?;
    if peer.uid() != unsafe { libc::geteuid() } {
        return Err(io::Error::other("protected peer"));
    }
    let pid = peer
        .pid()
        .ok_or_else(|| io::Error::other("protected peer process unavailable"))?;
    let group = fs::read_to_string(format!("/proc/{pid}/cgroup"))?;
    if !group
        .lines()
        .any(|line| line.strip_prefix("0::") == Some(scope.scope.cgroup.as_str()))
    {
        return Err(io::Error::other("protected peer scope"));
    }
    let mut bytes = Zeroizing::new(Vec::new());
    tokio::time::timeout(
        Duration::from_secs(3),
        (&mut stream)
            .take((MAX_MESSAGE + 1) as u64)
            .read_to_end(&mut bytes),
    )
    .await
    .map_err(|_| io::Error::other("protected request deadline"))??;
    if bytes.len() > MAX_MESSAGE {
        return Err(io::Error::other("protected frame bound"));
    }
    let mut operation: ProtectedBrowserOperation = serde_json::from_slice(&bytes)
        .map_err(|_| io::Error::other("protected request invalid"))?;
    let mut history = reconciled_history(&scope.profile, scope.authority.journal)?;
    match &mut operation {
        ProtectedBrowserOperation::Prepare { destination, .. } => {
            let ProfileProtection::Tracked { profile, .. } = history.protection else {
                return Err(io::Error::other("unknown profile"));
            };
            destination.profile = profile;
        }
        ProtectedBrowserOperation::Release {
            id,
            protection,
            destination,
        } => {
            let poll = ProtectedBrowserRequest {
                authority: BrowserAuthority {
                    task: scope.authority.task,
                    execution: scope.authority.execution,
                    fence: scope.authority.fence,
                    journal: scope.authority.journal,
                },
                operation: ProtectedBrowserOperation::Poll { id: *id },
            };
            let ProtectedBrowserReply::Review { review } = exchange(config, poll).await? else {
                return Err(io::Error::other("protected review missing"));
            };
            let ProfileProtection::Tracked {
                profile, releases, ..
            } = &mut history.protection
            else {
                return Err(io::Error::other("unknown profile"));
            };
            if history.consumed.contains(&review.release) || releases.len() >= 64 {
                return Err(io::Error::other("protected release consumed or full"));
            }
            destination.profile = *profile;
            if review.destination != *destination || review.state != UseState::Authorized {
                return Err(io::Error::other("protected review changed"));
            }
            if !releases.contains(&review.release) {
                releases.push(review.release);
            }
            history.consumed.push(review.release);
            process::write(&path(&scope.profile)?, &history)?;
            *protection = history.protection;
        }
        ProtectedBrowserOperation::Screen { protection, .. } => *protection = history.protection,
        _ => {}
    }
    let reply = exchange(
        config,
        ProtectedBrowserRequest {
            authority: scope.authority,
            operation,
        },
    )
    .await?;
    let bytes = Zeroizing::new(serde_json::to_vec(&reply)?);
    stream.write_all(&bytes).await?;
    stream.shutdown().await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_provenance_survives_restart_and_journal_replacement_refuses_capture() {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().join("profile");
        let journal = JournalId::new();
        initialize(&profile, journal).unwrap();
        let mut before = reconciled_history(&profile, journal).unwrap();
        let release = ReleaseId::new();
        let ProfileProtection::Tracked {
            releases, complete, ..
        } = &mut before.protection
        else {
            panic!()
        };
        assert!(*complete);
        releases.push(release);
        before.consumed.push(release);
        process::write(&path(&profile).unwrap(), &before).unwrap();
        initialize(&profile, journal).unwrap();
        assert_eq!(
            reconciled_history(&profile, journal).unwrap().protection,
            before.protection
        );
        initialize(&profile, JournalId::new()).unwrap();
        let disk: History = process::read(&path(&profile).unwrap()).unwrap();
        assert!(
            matches!(disk.protection, ProfileProtection::Tracked { complete: false, releases, .. } if releases == [release])
        );
        assert_eq!(disk.consumed, [release]);
        fs::remove_file(profile.join(".botstral-profile.json")).unwrap();
        assert!(initialize(&profile, journal).is_err());
    }
    #[test]
    fn existing_profile_without_history_never_gains_complete_coverage() {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().join("profile");
        fs::create_dir(&profile).unwrap();
        fs::write(profile.join("Cookies"), b"synthetic opaque browser state").unwrap();
        let journal = JournalId::new();
        initialize(&profile, journal).unwrap();
        assert!(matches!(
            reconciled_history(&profile, journal).unwrap().protection,
            ProfileProtection::Tracked {
                complete: false,
                ..
            }
        ));
    }
}
