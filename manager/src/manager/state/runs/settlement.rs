use super::*;

impl State {
    pub(in crate::manager) fn finish_run_call(
        &mut self,
        id: RunId,
        turn_id: RunTurnId,
        attempt: RunAttemptId,
        outcome: ModelOutcome,
    ) -> Result<(), OperationError> {
        let mut view = self.load_run(id)?;
        let index = view
            .turns
            .iter()
            .position(|turn| turn.id == turn_id && turn.attempt == attempt)
            .ok_or(OperationError::Conflict)?;
        if view.turns[index].phase != RunTurnPhase::Sending {
            return Ok(());
        }
        let mut turn = view.turns[index].clone();
        let mut child = None;
        match outcome {
            ModelOutcome::Unknown { reason, request_id } => {
                turn.phase = RunTurnPhase::AwaitingAttention { reason };
                turn.provider_request_id = request_id;
                view.summary.phase = RunPhase::AwaitingAttention {
                    reason: RunAttentionReason::Provider,
                };
            }
            ModelOutcome::NotSent { reason } => {
                turn.phase = RunTurnPhase::NotSent { reason };
                turn.accounting = InferenceAccounting::Released;
                view.summary.phase = if view.summary.cancellation_requested_at.is_some() {
                    RunPhase::Cancelled
                } else {
                    RunPhase::Stopped {
                        reason: match reason {
                            InferenceNotSentReason::Deadline => RunStopReason::Deadline,
                            InferenceNotSentReason::Shutdown => RunStopReason::Shutdown,
                            _ => RunStopReason::ConfigurationChanged,
                        },
                    }
                };
            }
            ModelOutcome::Billed {
                usage,
                request_id,
                response,
                finish_reason,
            } => {
                let charged = inference::cost(
                    &view.configuration.policy.prices,
                    usage.prompt_tokens,
                    usage.completion_tokens,
                )?;
                view.summary.accounting.settled_tokens += usage.total_tokens;
                view.summary.accounting.settled_cost_microusd += charged;
                turn.accounting = InferenceAccounting::Settled {
                    charged_microusd: charged,
                };
                turn.usage = Some(usage);
                turn.finish_reason = finish_reason;
                turn.provider_request_id = Some(request_id);
                turn.phase = RunTurnPhase::Responded;
                view.summary.accounting.held_tokens -= turn.reservation.total_tokens;
                view.summary.accounting.held_cost_microusd -= turn.reservation.cost_microusd;
                if view.summary.cancellation_requested_at.is_some() {
                    view.summary.phase = RunPhase::Cancelled;
                } else if view.request.limits.deadline <= self.now() {
                    view.summary.phase = RunPhase::Stopped {
                        reason: RunStopReason::Deadline,
                    };
                } else if let Ok(response) = response {
                    turn.response = Some(response.clone());
                    match response {
                        RunMessage::Assistant { .. }
                            if finish_reason == Some(InferenceFinishReason::Length) =>
                        {
                            view.summary.phase = RunPhase::Stopped {
                                reason: RunStopReason::OutputLimit,
                            }
                        }
                        RunMessage::Assistant {
                            tool_call: None, ..
                        } => view.summary.phase = RunPhase::Completed,
                        RunMessage::Assistant {
                            tool_call: Some(call),
                            ..
                        } => {
                            let duplicate = view.turns.iter().any(|turn| matches!(&turn.response,Some(RunMessage::Assistant { tool_call: Some(prior), .. }) if prior.id == call.id));
                            let operations = match &call.proposal {
                                ToolProposal::Computer(args) => args.operations.len() as u32,
                                ToolProposal::Process(_) => 1,
                            };
                            let reason = if duplicate {
                                Some(RunStopReason::UnsupportedProposal)
                            } else if view.summary.accounting.admitted_operations + operations
                                > view.request.limits.max_operations
                            {
                                Some(RunStopReason::OperationLimit)
                            } else {
                                next_call_stop(&view)?
                            };
                            if let Some(reason) = reason {
                                view.summary.phase = RunPhase::Stopped { reason };
                            } else if let Ok(work) = tools::translate(
                                &view.request.execution_policy,
                                &call.proposal,
                                view.request.limits.deadline,
                            ) {
                                let request = SubmitTask {
                                    id: TaskId::new(),
                                    session_id: view.request.session,
                                    conversation_id: None,
                                    settings: TaskSettings {
                                        model: view.configuration.policy.model_id.clone(),
                                        max_turns: 1,
                                        max_tokens: view.request.limits.max_output_tokens,
                                        deadline: view.request.limits.deadline,
                                        max_cost_micros: view.request.limits.max_cost_microusd,
                                    },
                                    work,
                                };
                                match self.prepare_task(request) {
                                    Ok(task) => {
                                        view.summary.accounting.admitted_operations += operations;
                                        view.summary.phase = RunPhase::WaitingForTool {
                                            turn: turn.id,
                                            task: task.id,
                                        };
                                        turn.intent = Some(RunToolIntent {
                                            call,
                                            task: task.id,
                                            command: task.command.id,
                                            operation: task.command.operation,
                                            execution: task.command.execution,
                                            work: task.command.work.clone(),
                                            result: None,
                                        });
                                        child = Some(task);
                                    }
                                    Err(OperationError::Internal) => {
                                        return Err(OperationError::Internal);
                                    }
                                    Err(_) => {
                                        view.summary.phase = RunPhase::Stopped {
                                            reason: RunStopReason::ChildUnavailable,
                                        }
                                    }
                                }
                            } else {
                                view.summary.phase = RunPhase::Stopped {
                                    reason: RunStopReason::UnsupportedProposal,
                                };
                            }
                        }
                        _ => return Err(OperationError::Internal),
                    }
                } else {
                    view.summary.phase = RunPhase::Stopped {
                        reason: RunStopReason::UnsupportedProposal,
                    };
                }
            }
        }
        if matches!(turn.accounting, InferenceAccounting::Released) {
            view.summary.accounting.held_tokens -= turn.reservation.total_tokens;
            view.summary.accounting.held_cost_microusd -= turn.reservation.cost_microusd;
        }
        view.summary.updated_at = self.now();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(task) = &child {
            insert_task(&tx, task)?;
            tx.execute(
                "INSERT INTO run_children VALUES(?1,?2,?3,?4)",
                params![
                    turn.id.to_string(),
                    id.to_string(),
                    turn.intent.as_ref().unwrap().call.id,
                    task.id.to_string()
                ],
            )?;
        }
        save_turn(&tx, &turn)?;
        save_summary(&tx, &view.summary)?;
        tx.commit()?;
        self.fault("run_response_committed");
        if let Some(task) = child {
            self.pump_tasks(task.command.worker)?;
        }
        Ok(())
    }
    pub(in crate::manager) fn finish_run_result(
        &mut self,
        id: RunId,
        turn_id: RunTurnId,
        expected: TaskView,
        result: Result<String, ()>,
    ) -> Result<(), OperationError> {
        let mut view = self.load_run(id)?;
        if !matches!(view.summary.phase, RunPhase::WaitingForTool { turn, task } if turn == turn_id && task == expected.id)
        {
            return Ok(());
        }
        if self.load_task(expected.id)? != expected {
            return Ok(());
        }
        let turn = view
            .turns
            .iter_mut()
            .find(|turn| turn.id == turn_id)
            .ok_or(OperationError::Internal)?;
        let intent = turn.intent.as_mut().ok_or(OperationError::Internal)?;
        if intent.result.is_some() {
            return Ok(());
        }
        match result {
            Ok(result) if result.len() <= MAX_RUN_RESULT_BYTES => {
                intent.result = Some(result);
                view.summary.phase = RunPhase::Ready;
            }
            _ => {
                view.summary.phase = RunPhase::AwaitingAttention {
                    reason: RunAttentionReason::ResultInvalid,
                }
            }
        }
        view.summary.updated_at = self.now();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        save_turn(&tx, turn)?;
        save_summary(&tx, &view.summary)?;
        tx.commit()?;
        self.fault("run_result_committed");
        Ok(())
    }
}
