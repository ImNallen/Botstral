export type BotId = string;
export type DocumentKind = 'instructions' | 'memory';
export type SourceReference =
  | {
      id: TaskId;
      type: 'task';
    }
  | {
      id: MessageId;
      type: 'message';
    }
  | {
      reference: string;
      type: 'external';
    };
export type TaskId = string;
export type MessageId = string;
export type InferenceId = string;
export type ModelReleaseAcknowledgment =
  | {
      type: 'stable';
    }
  | {
      terms_url: string;
      type: 'preview';
    };
export type InferenceEndpoint = 'global' | 'eu' | 'us';
export type InferenceBilling = 'text_tokens_only_at_acknowledged_ceilings';
export type ManagerMessage =
  | {
      execution: ExecutionId;
      id: ArtifactId;
      offset: number;
      slot: string;
      state: ArtifactTransferState;
      type: 'artifact_progress';
    }
  | {
      offset: number;
      type: 'inventory_request';
    }
  | {
      after: number;
      high_water: number;
      type: 'recover';
    }
  | {
      through: number;
      type: 'report_ack';
    }
  | {
      generation: Generation;
      type: 'recovery_ready';
    }
  | {
      authority?: ExecutionBinding | null;
      body: CommandBody;
      cancel: boolean;
      fence: InputFence;
      type: 'delivery';
    }
  | {
      binding: ExecutionBinding;
      fence: InputFence;
      incarnation: JournalId;
      type: 'permit';
    }
  | {
      execution: ExecutionId;
      type: 'claim_denied';
    }
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
export type ExecutionId = string;
export type ArtifactId = string;
export type ArtifactTransferState = 'receiving' | 'stored' | 'unavailable';
export type Generation = number;
export type ApprovalId = string;
export type ArgumentDigest = string;
export type OperationId = string;
export type OwnerId = string;
export type CommandId = string;
export type ComputerSessionId = string;
export type WorkSpec =
  | {
      type: 'unavailable';
    }
  | {
      steps: number;
      type: 'test_input';
    }
  | {
      observable: boolean;
      type: 'test_effect';
      value: number;
    }
  | {
      spec: ProcessSpec;
      type: 'process';
    }
  | {
      spec: ComputerSpec;
      type: 'computer';
    };
export type ProcessProfile = 'linux_systemd_v1';
export type ComputerOperation =
  | {
      type: 'navigate';
      url: string;
    }
  | {
      selector: string;
      type: 'click';
    }
  | {
      selector: string;
      text: string;
      type: 'fill';
    }
  | {
      output: string;
      selector: string;
      type: 'capture_text';
    }
  | {
      output: string;
      type: 'screenshot';
    }
  | {
      path: string;
      text: string;
      type: 'write_file';
    }
  | {
      output: string;
      path: string;
      type: 'read_file';
    }
  | {
      output: string;
      path: string;
      type: 'export_file';
    };
export type MediaType = 'png' | 'text_plain' | 'octet_stream';
export type ComputerProfile = 'linux_playwright_v1';
export type CertificateFingerprint = string;
export type WorkerId = string;
export type InputOwner = 'bot' | 'owner';
export type Revision = number;
export type JournalId = string;
export type ControlStatus = 'pending' | 'confirmed' | 'disconnected';
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
export type ConversationId = string;
export type InferenceVerification =
  | {
      type: 'unverified';
    }
  | {
      checked_at: number;
      expires_at: number;
      type: 'verified';
    };
export type InferenceAccounting =
  | {
      type: 'held';
    }
  | {
      charged_microusd: number;
      type: 'settled';
    }
  | {
      type: 'released';
    };
export type InferencePhase =
  | {
      type: 'admitted';
    }
  | {
      attempt: InferenceAttemptId;
      started_at: number;
      type: 'sending';
    }
  | {
      attempt: InferenceAttemptId;
      finish_reason: InferenceFinishReason;
      type: 'completed';
    }
  | {
      type: 'cancelled';
    }
  | {
      reason: InferenceNotSentReason;
      type: 'not_sent';
    }
  | {
      attempt: InferenceAttemptId;
      reason: InferenceAttentionReason;
      type: 'awaiting_attention';
    };
export type InferenceAttemptId = string;
export type InferenceFinishReason = 'stop' | 'length';
export type InferenceNotSentReason = 'deadline' | 'configuration_changed' | 'vault_locked' | 'shutdown' | 'restart';
export type InferenceAttentionReason =
  | 'interrupted'
  | 'provider_rejected'
  | 'invalid_response'
  | 'invalid_usage'
  | 'unexpected_model'
  | 'response_too_large'
  | 'shutdown'
  | 'restart';
export type ExecutionEvent =
  | {
      evidence: ProcessEvidence;
      type: 'process_result';
    }
  | {
      reason: ProcessCompletionReason;
      scope: ProcessScope;
      success: boolean;
      type: 'process_finished';
    }
  | {
      type: 'accepted';
    }
  | {
      type: 'approval_required';
    }
  | {
      type: 'admitted';
    }
  | {
      applied: number;
      type: 'progress';
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
      reason: string;
      type: 'uncertain';
    }
  | {
      applied: number;
      evidence: StopEvidence;
      type: 'stopped';
    };
export type ProcessEffect = 'not_released' | 'possible' | 'observed';
export type ProcessExit =
  | {
      type: 'unobserved';
    }
  | {
      code: number;
      type: 'exited';
    }
  | {
      signal: number;
      type: 'signalled';
    }
  | {
      type: 'spawn_failed';
    }
  | {
      type: 'expired';
    };
export type ProcessCompletionReason = 'result' | 'limit';
export type StopEvidence =
  | ('no_process_group' | 'not_launched')
  | {
      systemd_scope_empty: {
        scope: ProcessScope;
      };
    };
export type ArtifactState =
  | {
      type: 'pending';
    }
  | {
      content: ArtifactContent;
      id: ArtifactId;
      type: 'transferring';
    }
  | {
      content: ArtifactContent;
      id: ArtifactId;
      type: 'available';
    }
  | {
      reason: ArtifactUnavailable;
      type: 'unavailable';
    };
export type ArtifactUnavailable = 'missing' | 'rejected' | 'execution_uncertain' | 'corrupt';
export type TaskState =
  | 'queued'
  | 'running'
  | 'awaiting_approval'
  | 'cancelling'
  | 'completed'
  | 'failed'
  | 'cancelled'
  | 'awaiting_attention';
export type EventReplay =
  | {
      cursor: number;
      entries: AuthoritativeEvent[];
      type: 'events';
    }
  | {
      cursor: number;
      state: OwnerSnapshot;
      type: 'snapshot';
    };
export type WorkerMessage =
  | {
      offer: ArtifactOffer;
      type: 'artifact_failed';
    }
  | {
      offer: ArtifactOffer;
      type: 'artifact_offer';
    }
  | {
      chunk: ArtifactChunk;
      type: 'artifact_chunk';
    }
  | {
      entries: JournalEntry[];
      generation: Generation;
      incarnation: JournalId;
      offset: number;
      total: number;
      type: 'inventory';
    }
  | {
      generation: Generation;
      high_water: number;
      incarnation: JournalId;
      type: 'recovery_hello';
    }
  | {
      generation: Generation;
      high_water: number;
      incarnation: JournalId;
      reports: DurableReport[];
      type: 'reports';
    }
  | {
      binding: ExecutionBinding;
      fence: InputFence;
      incarnation: JournalId;
      type: 'claim';
    }
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
export type SealedOutput =
  | {
      content: ArtifactContent;
      type: 'content';
    }
  | {
      reason: ArtifactUnavailable;
      type: 'unavailable';
    };
export type JournalPhase = 'prepared' | 'claim_pending' | 'admitted' | 'resolved' | 'uncertain' | 'stopped';
export type Capability = 'logical_sessions' | 'input_fencing' | 'test_input' | 'process_systemd_v1' | 'computer_v1';
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
export type TaskStatus = 'running' | 'suspended' | 'completed' | 'cancelled' | 'failed';

export interface Protocol {
  document: WriteDocument;
  inference: SubmitInference;
  inference_configuration: ConfigureInference;
  inference_verification: VerifyInference;
  manager: Envelope2;
  owner: OwnerSnapshot;
  replay: EventReplay;
  worker: Envelope;
}
export interface WriteDocument {
  bot: BotId;
  expected_revision: number;
  kind: DocumentKind;
  markdown: string;
  sources: SourceReference[];
}
export interface SubmitInference {
  bot: BotId;
  configuration_revision: number;
  deadline: number;
  id: InferenceId;
  max_cost_microusd: number;
  max_output_tokens: number;
  max_total_tokens: number;
  prompt: string;
}
export interface ConfigureInference {
  bot: BotId;
  expected_revision: number;
  policy: InferencePolicy;
}
export interface InferencePolicy {
  acknowledgment: InferenceAcknowledgment;
  context_tokens: number;
  endpoint: InferenceEndpoint;
  model_id: string;
  prices: InferencePrices;
}
export interface InferenceAcknowledgment {
  acknowledged_at: number;
  developer_terms_url: string;
  model_developer: string;
  privacy_terms_url: string;
  release: ModelReleaseAcknowledgment;
}
export interface InferencePrices {
  billing: InferenceBilling;
  effective_at: number;
  expires_at: number;
  input_microusd_per_million_tokens: number;
  output_microusd_per_million_tokens: number;
}
export interface VerifyInference {
  bot: BotId;
  revision: number;
}
export interface Envelope2 {
  message: ManagerMessage;
  request_id: RequestId;
  version: number;
}
export interface ExecutionBinding {
  approval: ApprovalId;
  arguments: ArgumentDigest;
  bot: BotId;
  execution: ExecutionId;
  expires_at: number;
  operation: OperationId;
  owner: OwnerId;
  permission_revision: number;
  task: TaskId;
}
export interface CommandBody {
  bot: BotId;
  execution: ExecutionId;
  id: CommandId;
  operation: OperationId;
  owner: OwnerId;
  session: ComputerSessionId;
  task: TaskId;
  work: WorkSpec;
  worker: WorkerId;
}
export interface ProcessSpec {
  account: ProcessAccount;
  args: string[];
  cwd: string;
  deadline: number;
  environment: {
    [k: string]: string;
  };
  executable: string;
  limits: ProcessLimits;
  profile: ProcessProfile;
}
export interface ProcessAccount {
  gid: number;
  uid: number;
}
export interface ProcessLimits {
  memory_bytes: number;
  output_bytes: number;
  processes: number;
  runtime_seconds: number;
}
export interface ComputerSpec {
  account: ProcessAccount;
  deadline: number;
  limits: ProcessLimits;
  operations: ComputerOperation[];
  outputs: OutputDeclaration[];
  profile: ComputerProfile;
  runtime: ComputerRuntime;
  viewport: Viewport;
}
export interface OutputDeclaration {
  max_bytes: number;
  media_type: MediaType;
  name: string;
  slot: string;
}
export interface ComputerRuntime {
  sha256: CertificateFingerprint;
}
export interface Viewport {
  height: number;
  width: number;
}
export interface InputFence {
  generation: Generation;
  owner: InputOwner;
  revision: Revision;
  session_id: ComputerSessionId;
}
export interface ComputerSession {
  bot_id: BotId;
  id: ComputerSessionId;
  owner: InputOwner;
  revision: Revision;
  status: ControlStatus;
  worker_id: WorkerId;
}
export interface OwnerSnapshot {
  conversations: Conversation[];
  documents: Document[];
  inference_configurations: InferenceConfiguration[];
  inferences: InferenceView[];
  messages: ConversationMessage[];
  permissions: Permission[];
  tasks: TaskView[];
}
export interface Conversation {
  bot: BotId;
  id: ConversationId;
}
export interface Document {
  bot: BotId;
  kind: DocumentKind;
  markdown: string;
  revision: number;
  sources: SourceReference[];
}
export interface InferenceConfiguration {
  bot: BotId;
  policy: InferencePolicy;
  revision: number;
  verification: InferenceVerification;
}
export interface InferenceView {
  accounting: InferenceAccounting;
  cancellation_requested_at?: number | null;
  configuration: InferenceConfiguration;
  created_at: number;
  output?: string | null;
  phase: InferencePhase;
  provider_request_id?: string | null;
  request: SubmitInference;
  reservation: InferenceReservation;
  updated_at: number;
  usage?: InferenceUsage | null;
}
export interface InferenceReservation {
  cost_microusd: number;
  total_tokens: number;
}
export interface InferenceUsage {
  completion_tokens: number;
  prompt_tokens: number;
  total_tokens: number;
}
export interface ConversationMessage {
  conversation: ConversationId;
  id: MessageId;
  text: string;
}
export interface Permission {
  allow: boolean;
  bot: BotId;
  revision: number;
}
export interface TaskView {
  approval: ApprovalView;
  command: CommandBody;
  effect_evidence: ExecutionEvent[];
  id: TaskId;
  outcome?: ExecutionEvent | null;
  outputs?: OutputView[];
  process?: ProcessEvidence | null;
  progress: number;
  request: SubmitTask;
  state: TaskState;
}
export interface ApprovalView {
  binding?: ExecutionBinding | null;
  consumed: boolean;
  id: ApprovalId;
  permission_revision: number;
  revoked: boolean;
}
export interface ProcessEvidence {
  effect: ProcessEffect;
  result?: ProcessResult | null;
}
export interface ProcessResult {
  exit: ProcessExit;
  stderr: CapturedOutput;
  stdout: CapturedOutput;
}
export interface CapturedOutput {
  complete: boolean;
  hex: string;
  truncated: boolean;
}
export interface ProcessScope {
  boot: string;
  cgroup: string;
  execution: ExecutionId;
  host: CertificateFingerprint;
  invocation: string;
  journal: JournalId;
  unit: string;
}
export interface OutputView {
  declaration: OutputDeclaration;
  state: ArtifactState;
}
export interface ArtifactContent {
  length: number;
  sha256: CertificateFingerprint;
}
export interface SubmitTask {
  conversation_id?: ConversationId | null;
  id: TaskId;
  session_id: ComputerSessionId;
  settings: TaskSettings;
  work: WorkSpec;
}
export interface TaskSettings {
  deadline: number;
  max_cost_micros: number;
  max_tokens: number;
  max_turns: number;
  model: string;
}
export interface AuthoritativeEvent {
  entity: string;
  kind: string;
  sequence: number;
}
export interface Envelope {
  message: WorkerMessage;
  request_id: RequestId;
  version: number;
}
export interface ArtifactOffer {
  execution: ExecutionId;
  journal: JournalId;
  output: SealedOutput;
  slot: string;
  task: TaskId;
}
export interface ArtifactChunk {
  execution: ExecutionId;
  hex: string;
  id: ArtifactId;
  journal: JournalId;
  offset: number;
  slot: string;
}
export interface JournalEntry {
  body: CommandBody;
  phase: JournalPhase;
}
export interface DurableReport {
  command: CommandId;
  event: ExecutionEvent;
  execution: ExecutionId;
  sequence: number;
}
export interface ArtifactReference {
  id: ArtifactId;
  length: number;
  media_type: MediaType;
  sha256: CertificateFingerprint;
}
