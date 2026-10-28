pub(super) mod results;
pub(super) mod tools;

use super::{
    inference::InferenceControl,
    provider::{ModelOutcome, ModelRequest, PreparedChat},
    *,
};
use botstral_link::*;
use std::{collections::HashSet, sync::atomic::Ordering, time::Duration};
use tokio::task::{JoinHandle, JoinSet};

pub(super) struct RunCallPermit {
    pub run: RunId,
    pub request: SubmitRun,
    pub configuration: InferenceConfiguration,
    pub turn: RunTurn,
    pub prepared: PreparedChat,
    pub secret: SecretString,
}
pub(super) struct CollectPermit {
    pub run: RunId,
    pub turn: RunTurnId,
    pub task: TaskView,
    pub connectors: Vec<ConnectorCallView>,
    pub directory: PathBuf,
    pub secret: SecretString,
}
pub(super) enum RunJob {
    Call(Box<RunCallPermit>),
    Collect(Box<CollectPermit>),
}
impl RunJob {
    fn run(&self) -> RunId {
        match self {
            Self::Call(permit) => permit.run,
            Self::Collect(permit) => permit.run,
        }
    }
}

pub(super) fn start(
    queue: Queue,
    provider: provider::Provider,
    control: Arc<InferenceControl>,
    stop: watch::Sender<bool>,
) -> JoinHandle<Result<(), OperationError>> {
    tokio::spawn(async move {
        let mut stopped = stop.subscribe();
        let mut jobs = JoinSet::new();
        let mut active = HashSet::new();
        let mut failure = None;
        let mut timer = tokio::time::interval(Duration::from_millis(250));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        'running: loop {
            if *stopped.borrow() {
                break;
            }
            if let Err(error) = queue
                .call_wait(|s| {
                    s.reconcile_connectors()?;
                    s.reconcile_run_stops()
                })
                .await
            {
                failure = Some(error);
                break;
            }
            while jobs.len() < MAX_INFERENCE_CALLS && !*stopped.borrow() {
                let capacity = provider.calls.clone().try_acquire_owned().ok();
                let allow_model = capacity.is_some();
                let excluded = active.clone();
                let job = match queue
                    .call_wait(move |s| s.claim_run_work(&excluded, allow_model))
                    .await
                {
                    Ok(Some(job)) => job,
                    Ok(None) => break,
                    Err(error) => {
                        failure = Some(error);
                        break 'running;
                    }
                };
                let run = job.run();
                active.insert(run);
                let queue = queue.clone();
                let provider = provider.clone();
                let mut stopped = stopped.clone();
                jobs.spawn(async move {
                    let result = match job {
                        RunJob::Call(permit) => {
                            let unknown = || ModelOutcome::Unknown { reason: InferenceAttentionReason::Shutdown, request_id: None };
                            let outcome = if *stopped.borrow() { unknown() } else {
                                tokio::select! {
                                    biased;
                                    outcome = provider.chat(ModelRequest { policy: &permit.configuration.policy, deadline: permit.request.limits.deadline, max_output_tokens: permit.request.limits.max_output_tokens, max_total_tokens: permit.turn.reservation.total_tokens, prepared: &permit.prepared }, &permit.secret, capacity) => outcome,
                                    _ = stopped.changed() => unknown(),
                                }
                            };
                            let turn = permit.turn.id;
                            let attempt = permit.turn.attempt;
                            drop(permit);
                            queue.call_wait(move |s| s.finish_run_call(run, turn, attempt, outcome)).await
                        }
                        RunJob::Collect(permit) => {
                            drop(capacity);
                            let turn = permit.turn;
                            let expected = permit.task.clone();
                            let collection = tokio::task::spawn_blocking(move || results::collect(*permit));
                            let result = if *stopped.borrow() { return (run, Ok(())); } else {
                                tokio::select! {
                                    result = tokio::time::timeout(Duration::from_secs(5), collection) => result.ok().and_then(Result::ok).unwrap_or(Err(())),
                                    _ = stopped.changed() => return (run, Ok(())),
                                }
                            };
                            queue.call_wait(move |s| s.finish_run_result(run, turn, expected, result)).await
                        }
                    };
                    (run, result)
                });
            }
            tokio::select! {
                biased;
                _ = stopped.changed() => break,
                result = jobs.join_next(), if !jobs.is_empty() => {
                    match result {
                        Some(Ok((run, Ok(())))) => { active.remove(&run); },
                        _ => { failure = Some(OperationError::Internal); break; }
                    }
                }
                _ = control.run_wake.notified() => {},
                _ = timer.tick() => {},
            }
        }
        control.stopping.store(true, Ordering::Release);
        stop.send_replace(true);
        while let Some(result) = jobs.join_next().await {
            if !matches!(result, Ok((_, Ok(())))) {
                failure = Some(OperationError::Internal);
            }
        }
        queue.call_wait(|s| s.recover_runs(true)).await?;
        failure.map_or(Ok(()), Err)
    })
}

impl Manager {
    pub async fn submit_run(
        &self,
        token: SessionToken,
        input: SubmitRun,
    ) -> Result<RunView, OperationError> {
        let control = self.inference.clone();
        if control.stopping.load(Ordering::Acquire) {
            return Err(OperationError::Busy);
        }
        self.queue
            .call(false, move |s| {
                if control.stopping.load(Ordering::Acquire) {
                    return Err(OperationError::Busy);
                }
                let result = s.submit_run(token, input);
                control.run_wake.notify_one();
                result
            })
            .await
    }
    pub async fn run(&self, token: SessionToken, id: RunId) -> Result<RunView, OperationError> {
        self.queue.call(false, move |s| s.run(token, id)).await
    }
    pub async fn cancel_run(
        &self,
        token: SessionToken,
        id: RunId,
    ) -> Result<RunView, OperationError> {
        self.queue
            .call(false, move |s| s.cancel_run(token, id))
            .await
    }
}
