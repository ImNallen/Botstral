use super::*;
use std::collections::BTreeMap;

pub const MAX_PROCESS_SPEC: usize = 2048;
pub const MAX_PROCESS_OUTPUT: usize = 1024;
pub const MAX_PROCESS_REPORT: usize = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProcessProfile {
    LinuxSystemdV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProcessAccount {
    pub uid: u32,
    pub gid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProcessLimits {
    pub runtime_seconds: u32,
    pub output_bytes: u32,
    pub processes: u32,
    pub memory_bytes: u64,
}
impl ProcessLimits {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        if !(1..=300).contains(&self.runtime_seconds)
            || !(1..=1024).contains(&self.output_bytes)
            || !(2..=32).contains(&self.processes)
            || !(16 * 1024 * 1024..=256 * 1024 * 1024).contains(&self.memory_bytes)
        {
            return Err(ErrorCode::InvalidMessage);
        }
        Ok(())
    }
    pub fn permits(&self, requested: &Self) -> bool {
        requested.runtime_seconds <= self.runtime_seconds
            && requested.output_bytes <= self.output_bytes
            && requested.processes <= self.processes
            && requested.memory_bytes <= self.memory_bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProcessSpec {
    pub profile: ProcessProfile,
    pub account: ProcessAccount,
    pub executable: String,
    pub args: Vec<String>,
    pub environment: BTreeMap<String, String>,
    pub cwd: String,
    pub deadline: i64,
    pub limits: ProcessLimits,
}
impl ProcessSpec {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        self.limits.validate()?;
        if self.account.uid == 0
            || self.deadline <= 0
            || !self.executable.starts_with('/')
            || !self.cwd.starts_with('/')
            || self.executable.contains('\0')
            || self.cwd.contains('\0')
            || self.args.len() > 32
            || self.args.iter().any(|v| v.contains('\0'))
            || self.environment.len() > 32
            || self
                .environment
                .iter()
                .any(|(k, v)| k.is_empty() || k.contains(['=', '\0']) || v.contains('\0'))
            || serde_json::to_vec(self)
                .map_err(|_| ErrorCode::InvalidMessage)?
                .len()
                > MAX_PROCESS_SPEC
        {
            return Err(ErrorCode::InvalidMessage);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProcessScope {
    pub journal: JournalId,
    pub execution: ExecutionId,
    pub boot: String,
    pub invocation: String,
    pub unit: String,
    pub cgroup: String,
}
impl ProcessScope {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        if self.boot.len() != 36
            || self.invocation.len() != 32
            || !self.invocation.bytes().all(|b| b.is_ascii_hexdigit())
            || self.unit != format!("botstral-{}-{}.service", self.journal, self.execution)
            || self.cgroup.len() > 512
            || !self.cgroup.starts_with('/')
            || self.cgroup.split('/').any(|s| matches!(s, "." | ".."))
            || !self.cgroup.ends_with(&self.unit)
        {
            return Err(ErrorCode::InvalidMessage);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProcessEffect {
    NotReleased,
    Possible,
    Observed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapturedOutput {
    pub hex: String,
    pub truncated: bool,
    pub complete: bool,
}
impl CapturedOutput {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        if self.hex.len() > MAX_PROCESS_OUTPUT * 2
            || !self.hex.len().is_multiple_of(2)
            || !self.hex.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(ErrorCode::InvalidMessage);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProcessExit {
    Unobserved,
    Exited { code: i32 },
    Signalled { signal: i32 },
    SpawnFailed,
    Expired,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProcessResult {
    pub exit: ProcessExit,
    pub stdout: CapturedOutput,
    pub stderr: CapturedOutput,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProcessEvidence {
    pub effect: ProcessEffect,
    pub result: Option<ProcessResult>,
}
impl ProcessEvidence {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        if let Some(result) = &self.result {
            result.stdout.validate()?;
            result.stderr.validate()?;
        }
        Ok(())
    }
}
impl DurableReport {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        let limit = match &self.event {
            ExecutionEvent::ProcessResult { evidence } => {
                evidence.validate()?;
                MAX_PROCESS_REPORT
            }
            ExecutionEvent::ProcessFinished { scope, .. } => {
                scope.validate()?;
                2048
            }
            ExecutionEvent::Stopped {
                evidence: StopEvidence::SystemdScopeEmpty { scope },
                ..
            } => {
                scope.validate()?;
                2048
            }
            ExecutionEvent::Uncertain { reason } if reason.len() > 256 => {
                return Err(ErrorCode::InvalidMessage);
            }
            _ => 1024,
        };
        if serde_json::to_vec(self)
            .map_err(|_| ErrorCode::InvalidMessage)?
            .len()
            > limit
        {
            return Err(ErrorCode::InvalidMessage);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> ProcessSpec {
        ProcessSpec {
            profile: ProcessProfile::LinuxSystemdV1,
            account: ProcessAccount {
                uid: 1000,
                gid: 1000,
            },
            executable: "/usr/bin/printf".into(),
            args: vec!["%s".into(), "$HOME %n".into()],
            environment: BTreeMap::from([("LITERAL".into(), "$HOME %n".into())]),
            cwd: "/tmp".into(),
            deadline: 4000000000,
            limits: ProcessLimits {
                runtime_seconds: 300,
                output_bytes: 1024,
                processes: 32,
                memory_bytes: 268435456,
            },
        }
    }

    #[test]
    fn process_inputs_are_bounded_and_every_field_is_bound() {
        let original = spec();
        original.validate().unwrap();
        let mut oversized = original.clone();
        oversized.args = vec!["a".repeat(2048)];
        assert!(oversized.validate().is_err());
        let mut nul = original.clone();
        nul.args.push("\0".into());
        assert!(nul.validate().is_err());
        let mut many = original.clone();
        many.environment = (0..33).map(|n| (format!("E{n}"), "v".into())).collect();
        assert!(many.validate().is_err());
        let mut limits = original.clone();
        limits.limits.runtime_seconds = 301;
        assert!(limits.validate().is_err());
        let body = CommandBody {
            id: CommandId::new(),
            owner: OwnerId::new(),
            task: TaskId::new(),
            bot: BotId::new(),
            worker: WorkerId::new(),
            session: ComputerSessionId::new(),
            operation: OperationId::new(),
            execution: ExecutionId::new(),
            work: WorkSpec::Process { spec: original },
        };
        let encoded = serde_json::to_value(&body.work).unwrap();
        for field in [
            "account",
            "executable",
            "args",
            "environment",
            "cwd",
            "deadline",
            "limits",
        ] {
            let mut changed = encoded.clone();
            match field {
                "account" => changed["spec"][field]["uid"] = 1001.into(),
                "executable" | "cwd" => changed["spec"][field] = "/changed".into(),
                "args" => changed["spec"][field] = serde_json::json!(["changed"]),
                "environment" => changed["spec"][field] = serde_json::json!({"E":"changed"}),
                "deadline" => changed["spec"][field] = 4000000001i64.into(),
                "limits" => changed["spec"][field]["runtime_seconds"] = 299.into(),
                _ => unreachable!(),
            }
            let mut other = body.clone();
            other.work = serde_json::from_value(changed).unwrap();
            assert_ne!(body.argument_digest(), other.argument_digest(), "{field}");
        }
    }

    #[test]
    fn maximum_process_encoding_fits_pages_and_snapshots() {
        let mut spec = spec();
        let size = serde_json::to_vec(&spec).unwrap().len();
        spec.args.push("a".repeat(MAX_PROCESS_SPEC - size - 3));
        assert_eq!(serde_json::to_vec(&spec).unwrap().len(), MAX_PROCESS_SPEC);
        spec.validate().unwrap();
        let body = CommandBody {
            id: CommandId::new(),
            owner: OwnerId::new(),
            task: TaskId::new(),
            bot: BotId::new(),
            worker: WorkerId::new(),
            session: ComputerSessionId::new(),
            operation: OperationId::new(),
            execution: ExecutionId::new(),
            work: WorkSpec::Process { spec: spec.clone() },
        };
        let stream = CapturedOutput {
            hex: "ff".repeat(MAX_PROCESS_OUTPUT),
            truncated: true,
            complete: false,
        };
        let evidence = ProcessEvidence {
            effect: ProcessEffect::Observed,
            result: Some(ProcessResult {
                exit: ProcessExit::Signalled { signal: i32::MAX },
                stdout: stream.clone(),
                stderr: stream,
            }),
        };
        let report = DurableReport {
            sequence: MAX_COUNTER,
            command: body.id,
            execution: body.execution,
            event: ExecutionEvent::ProcessResult {
                evidence: evidence.clone(),
            },
        };
        report.validate().unwrap();
        let mut reports = vec![];
        for _ in 0..REPORT_PAGE {
            reports.push(report.clone());
            if serde_json::to_vec(&reports).unwrap().len() > PAGE_BYTES {
                reports.pop();
                break;
            }
        }
        assert!(!reports.is_empty());
        let message = Envelope::new(WorkerMessage::Reports {
            generation: Generation::INITIAL,
            incarnation: JournalId::new(),
            high_water: MAX_COUNTER,
            reports,
        });
        assert!(serde_json::to_vec(&message).unwrap().len() <= MAX_MESSAGE);
        let entries = vec![
            JournalEntry {
                body: body.clone(),
                phase: JournalPhase::Admitted
            };
            REPORT_PAGE
        ];
        assert!(serde_json::to_vec(&entries).unwrap().len() <= PAGE_BYTES);
        let inventory = Envelope::new(WorkerMessage::Inventory {
            generation: Generation::INITIAL,
            incarnation: JournalId::new(),
            offset: 0,
            total: MAX_TASKS as u16,
            entries,
        });
        assert!(serde_json::to_vec(&inventory).unwrap().len() <= MAX_MESSAGE);
        let task = TaskView {
            id: body.task,
            request: SubmitTask {
                id: body.task,
                session_id: body.session,
                conversation_id: Some(ConversationId::new()),
                settings: TaskSettings {
                    model: "m".repeat(128),
                    max_turns: 1000,
                    max_tokens: 1000000,
                    deadline: spec.deadline,
                    max_cost_micros: MAX_COUNTER,
                },
                work: body.work.clone(),
            },
            command: body,
            state: TaskState::Cancelled,
            progress: 0,
            outcome: Some(ExecutionEvent::Uncertain {
                reason: "x".repeat(256),
            }),
            effect_evidence: vec![
                ExecutionEvent::Uncertain {
                    reason: "x".repeat(256)
                };
                4
            ],
            process: Some(evidence),
            approval: ApprovalView {
                id: ApprovalId::new(),
                binding: None,
                permission_revision: MAX_COUNTER,
                revoked: true,
                consumed: true,
            },
        };
        let bytes = serde_json::to_vec(&task).unwrap().len();
        assert!(bytes <= 24 * 1024);
        let snapshot = serde_json::to_vec(&vec![task; MAX_TASKS]).unwrap();
        assert!(snapshot.len() <= MAX_TASKS * 24 * 1024);
        println!(
            "maximum retained output report={} task={} task_snapshot={}",
            serde_json::to_vec(&report).unwrap().len(),
            bytes,
            snapshot.len()
        );
    }
}
