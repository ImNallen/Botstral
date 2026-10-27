use super::*;
use botstral_link::*;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(test)]
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::{
    sync::Notify,
    task::{JoinHandle, JoinSet},
};

use super::provider::*;

#[cfg(test)]
mod tests;

pub(super) struct InferenceControl {
    wake: Notify,
    pub(super) stopping: AtomicBool,
    pub(super) run_wake: Notify,
    pub(super) stop: watch::Sender<bool>,
    pub(super) coding_calls: tokio::sync::Mutex<JoinSet<()>>,
}

impl Default for InferenceControl {
    fn default() -> Self {
        Self {
            wake: Notify::new(),
            stopping: AtomicBool::new(false),
            run_wake: Notify::new(),
            stop: watch::channel(false).0,
            coding_calls: tokio::sync::Mutex::new(JoinSet::new()),
        }
    }
}

pub(crate) struct InferenceRuntime {
    stop: watch::Sender<bool>,
    task: Option<JoinHandle<Result<(), OperationError>>>,
    run_task: Option<JoinHandle<Result<(), OperationError>>>,
    control: Arc<InferenceControl>,
}

impl InferenceRuntime {
    pub(super) fn start(queue: Queue, provider: Provider, control: Arc<InferenceControl>) -> Self {
        let stop = control.stop.clone();
        let mut stopped = stop.subscribe();
        let run_task = super::runs::start(
            queue.clone(),
            provider.clone(),
            control.clone(),
            stop.clone(),
        );
        let stop_calls = stop.clone();
        let wake = control.clone();
        let task = tokio::spawn(async move {
            let mut calls = JoinSet::new();
            let mut failure = None;
            'running: loop {
                if *stopped.borrow() {
                    break;
                }
                while calls.len() < MAX_INFERENCE_CALLS && !*stopped.borrow() {
                    let permit = match queue.call_wait(|s| s.claim_inference()).await {
                        Ok(Some(permit)) => permit,
                        Ok(None) => break,
                        Err(error) => {
                            failure = Some(error);
                            break 'running;
                        }
                    };
                    let provider = provider.clone();
                    let queue = queue.clone();
                    let mut stopped = stopped.clone();
                    calls.spawn(async move {
                        let outcome = if *stopped.borrow() { ProviderOutcome::Unknown { reason: InferenceAttentionReason::Shutdown, request_id: None } } else {
                            tokio::select! {
                                biased;
                                outcome = provider.execute(&permit) => outcome,
                                _ = stopped.changed() => ProviderOutcome::Unknown { reason: InferenceAttentionReason::Shutdown, request_id: None },
                            }
                        };
                        let id = permit.view.request.id;
                        let attempt = permit.attempt;
                        drop(permit);
                        queue.call_wait(move |s| s.finish_inference(id, attempt, outcome)).await
                    });
                }
                tokio::select! {
                    biased;
                    _ = stopped.changed() => break,
                    result = calls.join_next(), if !calls.is_empty() => {
                        if !matches!(result, Some(Ok(Ok(())))) {
                            failure = Some(OperationError::Internal);
                            break;
                        }
                    },
                    _ = wake.wake.notified() => {},
                }
            }
            wake.stopping.store(true, Ordering::Release);
            stop_calls.send_replace(true);
            while let Some(result) = calls.join_next().await {
                if !matches!(result, Ok(Ok(()))) {
                    failure = Some(OperationError::Internal);
                }
            }
            queue.call_wait(|s| s.recover_inferences(true)).await?;
            failure.map_or(Ok(()), Err)
        });
        Self {
            stop,
            task: Some(task),
            run_task: Some(run_task),
            control,
        }
    }

    pub(crate) async fn shutdown(mut self) -> Result<(), OperationError> {
        self.control.stopping.store(true, Ordering::Release);
        self.stop.send_replace(true);
        let result = self
            .task
            .take()
            .ok_or(OperationError::Internal)?
            .await
            .map_err(|_| OperationError::Internal)?;
        let runs = self
            .run_task
            .take()
            .ok_or(OperationError::Internal)?
            .await
            .map_err(|_| OperationError::Internal)?;
        let mut coding = self.control.coding_calls.lock().await;
        let mut coding_result = Ok(());
        while let Some(result) = coding.join_next().await {
            if result.is_err() {
                coding_result = Err(OperationError::Internal);
            }
        }
        result.and(runs).and(coding_result)
    }
}

impl Drop for InferenceRuntime {
    fn drop(&mut self) {
        self.control.stopping.store(true, Ordering::Release);
        self.stop.send_replace(true);
    }
}

impl Manager {
    pub async fn configure_inference(
        &self,
        token: SessionToken,
        input: ConfigureInference,
    ) -> Result<InferenceConfiguration, OperationError> {
        self.queue
            .call(false, move |s| s.configure_inference(token, input))
            .await
    }
    pub async fn inference_configuration(
        &self,
        token: SessionToken,
        bot: BotId,
    ) -> Result<InferenceConfiguration, OperationError> {
        self.queue
            .call(false, move |s| s.inference_configuration(token, bot))
            .await
    }
    pub async fn verify_inference(
        &self,
        token: SessionToken,
        input: VerifyInference,
    ) -> Result<InferenceConfiguration, OperationError> {
        let permit = self
            .queue
            .call(false, move |s| {
                s.prepare_inference_verification(token, input)
            })
            .await?;
        let _capacity = self
            .provider
            .calls
            .clone()
            .try_acquire_owned()
            .map_err(|_| OperationError::Busy)?;
        let verified = self.provider.verify(&permit).await;
        let config = permit.configuration;
        let epoch = permit.epoch;
        self.queue
            .call_wait(move |s| s.finish_inference_verification(config, epoch, verified))
            .await
    }
    pub async fn submit_inference(
        &self,
        token: SessionToken,
        input: SubmitInference,
    ) -> Result<InferenceView, OperationError> {
        if self.inference.stopping.load(Ordering::Acquire) {
            return Err(OperationError::Busy);
        }
        let wake = self.inference.clone();
        self.queue
            .call(false, move |s| {
                if wake.stopping.load(Ordering::Acquire) {
                    return Err(OperationError::Busy);
                }
                let result = s.submit_inference(token, input);
                if result.is_ok() {
                    wake.wake.notify_one();
                }
                result
            })
            .await
    }
    pub async fn inference(
        &self,
        token: SessionToken,
        id: InferenceId,
    ) -> Result<InferenceView, OperationError> {
        self.queue
            .call(false, move |s| s.inference(token, id))
            .await
    }
    pub async fn cancel_inference(
        &self,
        token: SessionToken,
        id: InferenceId,
    ) -> Result<InferenceView, OperationError> {
        self.queue
            .call(false, move |s| s.cancel_inference(token, id))
            .await
    }
}
