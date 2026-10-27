use super::*;

impl State {
    pub(in crate::manager) fn set_input_owner(
        &mut self,
        token: SessionToken,
        id: ComputerSessionId,
        expected: Revision,
        owner: InputOwner,
    ) -> Result<ComputerSession, OperationError> {
        self.authorize(&token)?;
        let session = self.find_computer_session(id)?;
        if session.revision != expected {
            return Err(ErrorCode::StaleRevision.into());
        }
        if session.owner == owner {
            return Ok(session);
        }
        let revision = expected.advance()?;
        self.connection.execute(
            "UPDATE computer_sessions SET owner = ?1, revision = ?2 WHERE id = ?3",
            params![
                match owner {
                    InputOwner::Bot => "bot",
                    InputOwner::Owner => "owner",
                },
                revision.get() as i64,
                id.to_string()
            ],
        )?;
        let session = self.find_computer_session(id)?;
        if let Some(connection) = self.worker_connections.get_mut(&session.worker_id) {
            connection.confirmed.remove(&id);
            let generation = connection.generation;
            self.deliver(
                session.worker_id,
                Envelope::new(ManagerMessage::Control {
                    generation,
                    session: session.clone(),
                }),
            )?;
        }
        self.find_computer_session(id)
    }

    pub(in crate::manager::state) fn check_input_fence(
        &self,
        worker: WorkerId,
        fence: InputFence,
    ) -> Result<(), OperationError> {
        let connection = self
            .worker_connections
            .get(&worker)
            .ok_or(ErrorCode::Disconnected)?;
        if connection.generation != fence.generation {
            return Err(ErrorCode::StaleGeneration.into());
        }
        let session = self.find_computer_session(fence.session_id)?;
        if session.worker_id != worker {
            return Err(ErrorCode::Unauthorized.into());
        }
        if session.revision != fence.revision {
            return Err(ErrorCode::StaleRevision.into());
        }
        if session.owner != fence.owner {
            return Err(ErrorCode::WrongInputOwner.into());
        }
        if session.status != ControlStatus::Confirmed {
            return Err(ErrorCode::WrongPhase.into());
        }
        Ok(())
    }

    pub(in crate::manager) fn dispatch(
        &mut self,
        token: SessionToken,
        message: ManagerMessage,
    ) -> Result<DispatchReceipt, OperationError> {
        self.authorize(&token)?;
        let (id, fence) = dispatch_fence(&message).ok_or(OperationError::InvalidInput)?;
        let session = self.find_computer_session(fence.session_id)?;
        let worker = session.worker_id;
        self.check_input_fence(worker, fence)?;
        let connection = self.worker_connections.get(&worker).unwrap();
        match &message {
            ManagerMessage::Input { fence, .. } if fence.owner != InputOwner::Owner => {
                return Err(ErrorCode::WrongInputOwner.into());
            }
            ManagerMessage::Command(_) => return Err(ErrorCode::UnsupportedExecution.into()),
            _ => {}
        }
        if let Some(previous) = connection.commands.get(&id) {
            return if previous.message == message {
                Ok(previous.receipt.clone())
            } else {
                Err(ErrorCode::Conflict.into())
            };
        }
        if connection.commands.len() >= MAX_PENDING {
            return Err(ErrorCode::QueueFull.into());
        }
        let receipt = DispatchReceipt {
            command_id: id,
            generation: fence.generation,
            acknowledgment: Acknowledgment::Accepted,
            last_event: None,
            bot_inputs: 0,
        };
        self.worker_connections
            .get_mut(&worker)
            .unwrap()
            .commands
            .insert(
                id,
                PendingCommand {
                    message: message.clone(),
                    receipt: receipt.clone(),
                },
            );
        self.deliver(worker, Envelope::new(message))?;
        Ok(receipt)
    }

    pub(in crate::manager) fn receipt(
        &self,
        token: SessionToken,
        worker: WorkerId,
        id: CommandId,
    ) -> Result<DispatchReceipt, OperationError> {
        self.authorize(&token)?;
        self.worker_connections
            .get(&worker)
            .ok_or(ErrorCode::Disconnected)?
            .commands
            .get(&id)
            .map(|c| c.receipt.clone())
            .ok_or(ErrorCode::UnknownTask.into())
    }

    pub(in crate::manager) fn admit_delivery(
        &mut self,
        lease: ConnectionLease,
        message: &ManagerMessage,
    ) -> Result<(), OperationError> {
        self.check_lease(lease)?;
        match message {
            ManagerMessage::Delivery(delivery) => {
                self.check_input_fence(lease.worker, delivery.fence)?
            }
            ManagerMessage::Permit {
                incarnation,
                fence,
                binding,
            } => {
                self.check_input_fence(lease.worker, *fence)?;
                let task = self.load_task(binding.task)?;
                let permission = self.permission(task.command.bot)?;
                let current: String = self.connection.query_row(
                    "SELECT incarnation FROM worker_journals WHERE worker=?1",
                    [lease.worker.to_string()],
                    |r| r.get(0),
                )?;
                if task.state.terminal()
                    || !task.approval.consumed
                    || task.approval.revoked
                    || task.approval.binding.as_ref() != Some(binding)
                    || !task.command.matches_authority(binding)
                    || task.command.worker != lease.worker
                    || current != incarnation.to_string()
                    || !permission.allow
                    || permission.revision != binding.permission_revision
                    || matches!(
                        task.state,
                        botstral_link::TaskState::Cancelling
                            | botstral_link::TaskState::AwaitingAttention
                    )
                    || !self.run_child_eligible(&task)?
                    || task.request.settings.deadline <= self.now()
                    || binding.expires_at <= self.now()
                {
                    return Err(ErrorCode::WrongPhase.into());
                }
            }
            _ => {}
        }
        if let ManagerMessage::Receipt {
            command_id,
            outcome: Acknowledgment::Accepted,
        } = message
        {
            let command = self
                .worker_connections
                .get(&lease.worker)
                .unwrap()
                .commands
                .get(command_id)
                .ok_or(ErrorCode::InvalidMessage)?;
            self.check_input_fence(lease.worker, dispatch_fence(&command.message).unwrap().1)?;
        }
        if let Some((id, fence)) = dispatch_fence(message)
            && let Err(error) = self.check_input_fence(lease.worker, fence)
        {
            if let Some(command) = self
                .worker_connections
                .get_mut(&lease.worker)
                .unwrap()
                .commands
                .get_mut(&id)
            {
                command.receipt.acknowledgment = Acknowledgment::Rejected {
                    code: crate::worker_http::protocol_error(error),
                };
            }
            return Err(error);
        }
        Ok(())
    }
}
