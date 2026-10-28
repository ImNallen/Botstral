use super::*;

#[derive(Deserialize)]
#[serde(untagged)]
enum CachedRun {
    Current(Box<WorkspaceRun>),
    Previous(Box<RunView>),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CachedWorkspace {
    manager: ManagerId,
    cursor: u64,
    owner: String,
    bots: Vec<WorkspaceBot>,
    workers: Vec<WorkspaceWorker>,
    sessions: Vec<ComputerSession>,
    runs: Vec<CachedRun>,
    run_ready_bots: Vec<BotId>,
    vault_unlocked: bool,
    state: OwnerSnapshot,
}

pub(super) fn read(reader: impl Read) -> std::result::Result<OwnerWorkspace, serde_json::Error> {
    let cached: CachedWorkspace = serde_json::from_reader(reader)?;
    Ok(OwnerWorkspace {
        manager: cached.manager,
        cursor: cached.cursor,
        owner: cached.owner,
        bots: cached.bots,
        workers: cached.workers,
        sessions: cached.sessions,
        runs: cached
            .runs
            .into_iter()
            .map(|run| match run {
                CachedRun::Current(run) => *run,
                CachedRun::Previous(run) => WorkspaceRun::project(*run, true),
            })
            .collect(),
        run_ready_bots: cached.run_ready_bots,
        vault_unlocked: cached.vault_unlocked,
        state: cached.state,
    })
}
