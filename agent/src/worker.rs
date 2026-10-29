use crate::{journal::Journal, session::Runtime};
use botstral_link::*;
use std::{collections::HashMap, io, path::Path, time::Duration};

pub(crate) struct Worker {
    journal: Journal,
    runtime: Option<Runtime>,
    pending: HashMap<ComputerSessionId, Envelope<WorkerMessage>>,
    test_worker: bool,
    browser_listener: Option<(ExecutionId, std::os::unix::net::UnixListener)>,
    browser_request: Option<(protected::BrowserAuthority, tokio::task::JoinHandle<()>)>,
}
impl Worker {
    pub async fn open(directory: &Path, test_worker: bool) -> io::Result<Self> {
        let mut journal = Journal::open(directory, test_worker)?;
        journal.configure_processes().await?;
        Ok(Self {
            journal,
            runtime: None,
            pending: HashMap::new(),
            test_worker,
            browser_listener: None,
            browser_request: None,
        })
    }
    pub fn protected_browser(&mut self, config: &crate::Config) -> io::Result<()> {
        let Some(runtime) = &self.runtime else {
            return Ok(());
        };
        let Some(scope) = self.journal.browser_scope(&runtime.sessions)? else {
            self.stop_browser_request();
            self.browser_listener = None;
            return Ok(());
        };
        if self
            .browser_request
            .as_ref()
            .is_some_and(|(authority, task)| *authority != scope.authority || task.is_finished())
        {
            self.stop_browser_request();
        }
        if self.browser_request.is_some() {
            return Ok(());
        }
        if self
            .browser_listener
            .as_ref()
            .is_none_or(|(id, _)| *id != scope.authority.execution)
        {
            crate::protected_browser::initialize(&scope.profile, scope.authority.journal)?;
            let listener =
                std::os::unix::net::UnixListener::bind(scope.directory.join("protected.sock"))?;
            listener.set_nonblocking(true)?;
            self.browser_listener = Some((scope.authority.execution, listener));
        }
        let listener = &self.browser_listener.as_ref().unwrap().1;
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(true)?;
                let stream = tokio::net::UnixStream::from_std(stream)?;
                let config = config.clone();
                self.browser_request = Some((
                    scope.authority,
                    tokio::spawn(async move {
                        let _ = crate::protected_browser::handle(&config, scope, stream).await;
                    }),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) => return Err(error),
        }
        Ok(())
    }
    fn stop_browser_request(&mut self) {
        if let Some((_, request)) = self.browser_request.take() {
            request.abort();
        }
    }
    pub fn connector_settlements(&self) -> io::Result<Vec<crate::connector::Settlement>> {
        self.journal.connector_settlements()
    }
    pub fn hello(&self) -> Hello {
        let mut hello = Hello::current(self.test_worker);
        if self.journal.process_capable() {
            hello.capabilities.push(Capability::ProcessSystemdV1);
        }
        if self.journal.connector_capable() {
            hello.capabilities.push(Capability::ConnectorV1);
        }
        if self.journal.vibe_capable() {
            hello.capabilities.push(Capability::VibeV1);
        }
        if self.journal.computer_capable() {
            hello.capabilities.push(Capability::ComputerV1);
        }
        hello
    }
    pub fn attach(
        &mut self,
        generation: Generation,
        sessions: Vec<ComputerSession>,
        worker: WorkerId,
    ) -> io::Result<WorkerMessage> {
        let fences: Vec<_> = sessions
            .iter()
            .map(|session| session.fence(generation))
            .collect();
        if let Some(runtime) = &mut self.runtime {
            runtime.sessions = crate::session::Sessions::new(generation, sessions, worker)
                .map_err(io::Error::other)?;
        } else {
            self.runtime = Some(
                Runtime::new(generation, sessions, worker, self.test_worker)
                    .map_err(io::Error::other)?,
            );
        }
        self.pending.clear();
        for fence in fences {
            if self.journal.needs_process_confirmation(fence.session_id)? {
                self.pending.insert(
                    fence.session_id,
                    Envelope::new(WorkerMessage::ControlApplied { fence }),
                );
            }
        }
        self.journal.attach();
        Ok(WorkerMessage::RecoveryHello {
            generation,
            incarnation: self.journal.incarnation,
            high_water: self.journal.high_water()?,
        })
    }
    pub fn disconnected(&mut self) -> io::Result<()> {
        self.stop_browser_request();
        self.browser_listener = None;
        self.pending.clear();
        self.journal.stop_processes(false)?;
        self.journal.disconnected()
    }
    pub fn handle(
        &mut self,
        envelope: Envelope<ManagerMessage>,
    ) -> io::Result<Vec<Envelope<WorkerMessage>>> {
        let runtime = self
            .runtime
            .as_mut()
            .ok_or_else(|| io::Error::other("worker is disconnected"))?;
        let messages = match self.journal.handle(&runtime.sessions, &envelope.message)? {
            Some(messages) => messages,
            None => runtime.handle(envelope).map_err(io::Error::other)?,
        };
        let mut output = Vec::new();
        for message in messages {
            if let WorkerMessage::ControlApplied { fence } = &message.message
                && !self.journal.can_confirm_session(fence.session_id)?
            {
                self.pending.insert(fence.session_id, message);
            } else {
                output.push(message);
            }
        }
        Ok(output)
    }
    pub async fn tick(&mut self, connected: bool) -> io::Result<Vec<Envelope<WorkerMessage>>> {
        self.journal
            .tick_processes(if connected {
                self.runtime.as_ref().map(|r| &r.sessions)
            } else {
                None
            })
            .await?;
        if !connected {
            return Ok(vec![]);
        }
        let runtime = self
            .runtime
            .as_mut()
            .ok_or_else(|| io::Error::other("missing sessions"))?;
        let mut messages = self.journal.tick(&runtime.sessions)?;
        messages.extend(runtime.tick());
        for id in self.pending.keys().copied().collect::<Vec<_>>() {
            if self.journal.can_confirm_session(id)? {
                messages.push(self.pending.remove(&id).unwrap());
            }
        }
        Ok(messages)
    }
    pub async fn shutdown(&mut self) -> io::Result<()> {
        self.stop_browser_request();
        self.journal.stop_processes(true)?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(18);
        while !self.journal.processes_stopped()? {
            self.tick(false).await?;
            if tokio::time::Instant::now() >= deadline {
                return Err(io::Error::other("process shutdown unproved"));
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Ok(())
    }
}
