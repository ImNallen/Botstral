export type ManagerMessage =
  | {
      generation: Generation;
      sessions: ComputerSession[];
      type: 'welcome';
      worker_id: WorkerId;
    }
  | {
      generation: Generation;
      session: ComputerSession;
      type: 'control';
    }
  | {
      action: CommandAction;
      fence: InputFence;
      id: CommandId;
      task_id: TaskId;
      type: 'command';
    }
  | {
      command_id: CommandId;
      fence: InputFence;
      type: 'input';
      value: number;
    }
  | {
      command_id: CommandId;
      outcome: Acknowledgment;
      type: 'receipt';
    }
  | {
      code: ErrorCode;
      type: 'error';
    }
  | {
      type: 'pong';
    };
export type Generation = number;
export type BotId = string;
export type ComputerSessionId = string;
export type InputOwner = 'bot' | 'owner';
export type Revision = number;
export type ControlStatus = 'pending' | 'confirmed' | 'disconnected';
export type WorkerId = string;
export type CommandAction =
  | {
      type: 'start';
      work: WorkSpec;
    }
  | {
      type: 'status';
    }
  | {
      type: 'cancel';
    }
  | {
      type: 'result';
    }
  | {
      approval_id: ApprovalId;
      argument_digest: CertificateFingerprint;
      expires_at: number;
      operation_id: OperationId;
      type: 'execute_approved';
    };
export type WorkSpec =
  | {
      type: 'unavailable';
    }
  | {
      steps: number;
      type: 'test_input';
    };
export type ApprovalId = string;
export type CertificateFingerprint = string;
export type OperationId = string;
export type CommandId = string;
export type TaskId = string;
export type Acknowledgment =
  | {
      type: 'accepted';
    }
  | {
      type: 'applied';
    }
  | {
      code: ErrorCode;
      type: 'rejected';
    };
export type ErrorCode =
  | 'unauthorized'
  | 'invalid_invitation'
  | 'identity_mismatch'
  | 'invitation_expired'
  | 'invitation_used'
  | 'incompatible_version'
  | 'missing_capability'
  | 'incompatible_channel'
  | 'wrong_phase'
  | 'invalid_message'
  | 'stale_generation'
  | 'stale_revision'
  | 'wrong_input_owner'
  | 'unsupported_execution'
  | 'unknown_task'
  | 'queue_full'
  | 'disconnected'
  | 'conflict';
export type RequestId = string;
export type WorkerMessage =
  | {
      capabilities: Capability[];
      channel: ReleaseChannel;
      required: Capability[];
      type: 'hello';
      versions: number[];
    }
  | {
      generation: Generation;
      type: 'snapshot_applied';
    }
  | {
      fence: InputFence;
      type: 'control_applied';
    }
  | {
      command_id: CommandId;
      generation: Generation;
      outcome: Acknowledgment;
      type: 'acknowledgment';
    }
  | {
      command_id: CommandId;
      event: TaskEvent;
      fence: InputFence;
      task_id: TaskId;
      type: 'event';
    }
  | {
      command_id: CommandId;
      fence: InputFence;
      task_id: TaskId;
      type: 'bot_input';
      value: number;
    }
  | {
      generation: Generation;
      type: 'heartbeat';
    };
export type Capability = 'logical_sessions' | 'input_fencing' | 'test_input';
export type ReleaseChannel = 'stable' | 'experimental';
export type TaskEvent =
  | {
      type: 'acceptance';
    }
  | {
      applied: number;
      total: number;
      type: 'progress';
    }
  | {
      code: LogCode;
      type: 'redacted_log';
    }
  | {
      argument_digest: CertificateFingerprint;
      operation_id: OperationId;
      type: 'approval_request';
    }
  | {
      artifact: ArtifactReference;
      type: 'screenshot';
    }
  | {
      artifact: ArtifactReference;
      type: 'artifact';
    }
  | {
      applied: number;
      type: 'completed';
    }
  | {
      code: ErrorCode;
      type: 'failed';
    }
  | {
      applied: number;
      type: 'cancelled';
    }
  | {
      applied: number;
      status: TaskStatus;
      type: 'status';
    };
export type LogCode = 'started' | 'input_applied' | 'stopped';
export type ArtifactId = string;
export type MediaType = 'png' | 'text_plain' | 'octet_stream';
export type TaskStatus = 'running' | 'suspended' | 'completed' | 'cancelled' | 'failed';

export interface Protocol {
  manager: Envelope2;
  worker: Envelope;
}
export interface Envelope2 {
  message: ManagerMessage;
  request_id: RequestId;
  version: number;
}
export interface ComputerSession {
  bot_id: BotId;
  id: ComputerSessionId;
  owner: InputOwner;
  revision: Revision;
  status: ControlStatus;
  worker_id: WorkerId;
}
export interface InputFence {
  generation: Generation;
  owner: InputOwner;
  revision: Revision;
  session_id: ComputerSessionId;
}
export interface Envelope {
  message: WorkerMessage;
  request_id: RequestId;
  version: number;
}
export interface ArtifactReference {
  id: ArtifactId;
  length: number;
  media_type: MediaType;
  sha256: CertificateFingerprint;
}
