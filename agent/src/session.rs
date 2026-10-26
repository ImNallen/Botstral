use botstral_link::*;
use std::collections::HashMap;

pub(crate) struct Sessions {
    pub generation: Generation,
    sessions: HashMap<ComputerSessionId, ComputerSession>,
}
impl Sessions {
    pub(crate) fn new(
        generation: Generation,
        sessions: Vec<ComputerSession>,
        worker: WorkerId,
    ) -> Result<Self, ErrorCode> {
        if sessions.len() > MAX_SESSIONS || sessions.iter().any(|s| s.worker_id != worker) {
            return Err(ErrorCode::InvalidMessage);
        }
        let count = sessions.len();
        let sessions: HashMap<_, _> = sessions.into_iter().map(|s| (s.id, s)).collect();
        if sessions.len() != count {
            return Err(ErrorCode::InvalidMessage);
        }
        Ok(Self {
            generation,
            sessions,
        })
    }
    pub(crate) fn control(
        &mut self,
        generation: Generation,
        session: ComputerSession,
    ) -> Result<InputFence, ErrorCode> {
        if generation != self.generation {
            return Err(ErrorCode::StaleGeneration);
        }
        if let Some(old) = self.sessions.get(&session.id) {
            if old.bot_id != session.bot_id || old.worker_id != session.worker_id {
                return Err(ErrorCode::Conflict);
            }
            if session.revision < old.revision
                || (session.revision == old.revision && session.owner != old.owner)
            {
                return Err(ErrorCode::StaleRevision);
            }
        } else if self.sessions.len() >= MAX_SESSIONS {
            return Err(ErrorCode::QueueFull);
        }
        let fence = session.fence(generation);
        self.sessions.insert(session.id, session);
        Ok(fence)
    }
    pub(crate) fn check(&self, fence: InputFence) -> Result<(), ErrorCode> {
        if fence.generation != self.generation {
            return Err(ErrorCode::StaleGeneration);
        }
        let session = self
            .sessions
            .get(&fence.session_id)
            .ok_or(ErrorCode::StaleRevision)?;
        if session.revision != fence.revision {
            return Err(ErrorCode::StaleRevision);
        }
        if session.owner != fence.owner {
            return Err(ErrorCode::WrongInputOwner);
        }
        Ok(())
    }
}
