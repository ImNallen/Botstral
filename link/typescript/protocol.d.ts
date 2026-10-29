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
export type InferenceAttemptId = string;
export type CodingCallPhase = 'reserved' | 'sent' | 'settled' | 'unknown' | 'not_sent';
export type TaskId = string;
export type RequestId = string;
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
export type InferenceBilling =
  'text_tokens_only_at_acknowledged_ceilings' | 'context_tokens_including_images_at_acknowledged_ceilings';
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
      spec: VibeSpec;
      type: 'vibe';
    }
  | {
      spec: ComputerSpec;
      type: 'computer';
    }
  | {
      spec: ConnectorSpec;
      type: 'connector';
    };
export type ProcessProfile = 'linux_systemd_v1';
export type VibeAccess = 'edit_files_as_guest_account';
export type CertificateFingerprint = string;
export type ConnectionId = string;
export type MediaType = 'png' | 'text_plain' | 'octet_stream';
export type CodingWorkspaceId = string;
export type VibeSession =
  | {
      type: 'new';
    }
  | {
      native_session: RequestId;
      task: TaskId;
      type: 'resume';
    };
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
export type ComputerProfile = 'linux_playwright_v1';
export type ConnectorOperation =
  | {
      connection: ConnectionVersion;
      type: 'discover';
    }
  | {
      proposal: ConnectorProposal;
      type: 'call';
    };
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
export type ManagerId = string;
export type ConnectionAvailability =
  'needs_discovery' | 'ready' | 'authentication_required' | 'unavailable' | 'revoked';
export type ConversationId = string;
export type InferenceVerification =
  | {
      type: 'unverified';
    }
  | {
      checked_at: number;
      expires_at: number;
      type: 'verified';
      vision?: boolean;
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
export type InboxId = string;
export type OccurrenceId = string;
export type RoutineId = string;
export type RunId = string;
export type OccurrenceStatus =
  | {
      type: 'pending';
    }
  | {
      type: 'running';
    }
  | {
      reason: string;
      type: 'decision';
    }
  | {
      phase: RunPhase;
      type: 'finished';
    }
  | {
      reason: string;
      type: 'blocked';
    }
  | {
      reason: string;
      type: 'skipped';
    };
export type RunPhase =
  | {
      type: 'ready';
    }
  | {
      turn: RunTurnId;
      type: 'calling';
    }
  | {
      turn: RunTurnId;
      type: 'waiting_for_memory';
    }
  | {
      task: TaskId;
      turn: RunTurnId;
      type: 'waiting_for_tool';
    }
  | {
      type: 'completed';
    }
  | {
      type: 'cancelling';
    }
  | {
      type: 'cancelled';
    }
  | {
      reason: RunStopReason;
      type: 'stopped';
    }
  | {
      reason: RunAttentionReason;
      type: 'awaiting_attention';
    };
export type RunTurnId = string;
export type RunStopReason =
  | 'turn_limit'
  | 'output_limit'
  | 'token_limit'
  | 'cost_limit'
  | 'operation_limit'
  | 'deadline'
  | 'configuration_changed'
  | 'vault_locked'
  | 'unsupported_proposal'
  | 'transcript_limit'
  | 'child_unavailable'
  | 'shutdown'
  | 'restart';
export type RunAttentionReason =
  'memory_publication' | 'provider' | 'worker' | 'result_invalid' | 'restart' | 'shutdown';
export type PublisherId = string;
export type RoutineTrigger =
  | {
      at: number;
      type: 'once';
    }
  | {
      anchor: number;
      seconds: number;
      type: 'interval';
    }
  | {
      hour: number;
      minute: number;
      timezone: string;
      type: 'daily';
    }
  | {
      equals: {
        [k: string]: EventScalar;
      };
      publisher: PublisherId;
      topic: string;
      type: 'event';
    };
export type EventScalar = string | number | boolean;
export type RoutineState = 'enabled' | 'paused' | 'deleted';
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
export type OwnerClientMessage =
  | {
      id: RequestId;
      operation: OwnerOperation;
      type: 'call';
    }
  | {
      cursor: number;
      type: 'watch';
    }
  | {
      cursor: number;
      type: 'applied';
    }
  | {
      type: 'ping';
    };
export type OwnerOperation =
  | {
      id: RequestId;
      method: 'routine_status';
    }
  | {
      input: RoutineCommand;
      method: 'routine_command';
    }
  | {
      bot: BotId;
      id: RequestId;
      method: 'routine_conversation';
      text: string;
    }
  | {
      before?: OccurrenceId | null;
      id: RoutineId;
      limit: number;
      method: 'routine_history';
    }
  | {
      after: number;
      limit: number;
      method: 'routine_preview';
      trigger: RoutineTrigger;
    }
  | {
      input: InstallConnection;
      method: 'install_connection';
    }
  | {
      method: 'connections';
    }
  | {
      id: ConnectionId;
      method: 'connection';
    }
  | {
      input: DiscoverConnection;
      method: 'discover_connection';
    }
  | {
      id: ConnectionId;
      input: ReplaceConnectionSecret;
      method: 'replace_connection_secret';
    }
  | {
      expected_revision: number;
      id: ConnectionId;
      method: 'reconnect_connection';
    }
  | {
      expected_revision: number;
      id: ConnectionId;
      method: 'revoke_connection';
    }
  | {
      bot: BotId;
      connection: ConnectionId;
      method: 'connector_grants';
    }
  | {
      bot: BotId;
      input: SetConnectorGrants;
      method: 'set_connector_grants';
    }
  | {
      bot: BotId;
      method: 'connector_tools';
    }
  | {
      method: 'connector_calls';
      task: TaskId;
    }
  | {
      after: number;
      connection: ConnectionId;
      method: 'connector_audit';
    }
  | {
      method: 'workspace';
    }
  | {
      approval: ApprovalView;
      command: CommandBody;
      expires_at: number;
      id: TaskId;
      method: 'grant_reviewed_approval';
      permission_revision: number;
    }
  | {
      cursor: number;
      limit: number;
      method: 'resume';
    }
  | {
      input: SubmitTask;
      method: 'submit_task';
    }
  | {
      id: TaskId;
      method: 'task';
    }
  | {
      id: TaskId;
      method: 'cancel_task';
    }
  | {
      expires_at: number;
      id: TaskId;
      method: 'grant_approval';
    }
  | {
      id: TaskId;
      method: 'revoke_approval';
    }
  | {
      allow: boolean;
      bot: BotId;
      expected_revision: number;
      method: 'set_permission';
    }
  | {
      input: Conversation;
      method: 'conversation';
    }
  | {
      input: ConversationMessage;
      method: 'append_message';
    }
  | {
      input: WriteDocument;
      method: 'write_document';
    }
  | {
      bot: BotId;
      method: 'search_documents';
      query: string;
    }
  | {
      method: 'rebuild_documents';
    }
  | {
      bot: BotId;
      method: 'inference_configuration';
    }
  | {
      input: ConfigureInference;
      method: 'configure_inference';
    }
  | {
      input: VerifyInference;
      method: 'verify_inference';
    }
  | {
      input: SubmitInference;
      method: 'submit_inference';
    }
  | {
      id: InferenceId;
      method: 'inference';
    }
  | {
      id: InferenceId;
      method: 'cancel_inference';
    }
  | {
      input: SubmitRun;
      method: 'submit_run';
    }
  | {
      id: RunId;
      method: 'run';
    }
  | {
      id: RunId;
      method: 'cancel_run';
    }
  | {
      id: ComputerSessionId;
      method: 'computer_session';
    }
  | {
      bot: BotId;
      method: 'open_computer_session';
      worker: WorkerId;
    }
  | {
      expected: Revision;
      id: ComputerSessionId;
      method: 'set_input_owner';
      owner: InputOwner;
    }
  | {
      fence: InputFence;
      id: CommandId;
      method: 'input';
      value: number;
    }
  | {
      id: CommandId;
      method: 'receipt';
      worker: WorkerId;
    }
  | {
      id: ArtifactId;
      method: 'artifact';
      task: TaskId;
    }
  | {
      method: 'coding_calls';
      task: TaskId;
    };
export type RoutineAction =
  | {
      definition: RoutineDefinition;
      id: RoutineId;
      type: 'create';
    }
  | {
      definition: RoutineDefinition;
      expected_revision: number;
      id: RoutineId;
      type: 'replace';
    }
  | {
      expected_revision: number;
      id: RoutineId;
      paused: boolean;
      type: 'set_paused';
    }
  | {
      expected_revision: number;
      id: RoutineId;
      type: 'test';
    }
  | {
      expected_revision: number;
      id: RoutineId;
      type: 'delete';
    }
  | {
      budget: RoutineBudget;
      type: 'set_budget';
    }
  | {
      bot: BotId;
      credential_sha256: string;
      id: PublisherId;
      name: string;
      topics: string[];
      type: 'install_publisher';
    }
  | {
      expected_revision: number;
      id: PublisherId;
      type: 'revoke_publisher';
    }
  | {
      bot: BotId;
      type: 'delete_bot';
    }
  | {
      bot: BotId;
      copy: BotId;
      name: string;
      type: 'duplicate_bot';
    }
  | {
      id: InboxId;
      type: 'acknowledge_inbox';
    };
export type ConnectionTransport =
  | {
      endpoint: string;
      type: 'remote_http';
    }
  | {
      account: ProcessAccount;
      arguments: string[];
      cwd: string;
      environment: {
        [k: string]: string;
      };
      executable: string;
      type: 'worker_stdio';
      worker: WorkerId;
    };
export type ConnectionSecretInput =
  | {
      type: 'none';
    }
  | {
      type: 'bearer';
      value: string;
    }
  | {
      type: 'local_environment';
      values: {
        [k: string]: string;
      };
    };
export type DeviceFingerprint = string;
export type DeviceId = string;
export type DeviceRole = 'owner' | 'worker';
export type OwnerHello =
  | {
      credential: string;
      manager: ManagerId;
      role: DeviceRole;
      type: 'authenticate';
      version: number;
    }
  | {
      credential: string;
      invitation: string;
      manager: ManagerId;
      role: DeviceRole;
      type: 'enroll';
      version: number;
    };
export type OwnerReply =
  | {
      type: 'routine_status';
      value: RoutineControlResult | null;
    }
  | {
      type: 'routine_control';
      value: RoutineControlResult;
    }
  | {
      type: 'routine_history';
      value: RoutineOccurrence[];
    }
  | {
      type: 'routine_preview';
      value: number[];
    }
  | {
      type: 'connection';
      value: ConnectionView;
    }
  | {
      type: 'connections';
      value: ConnectionView[];
    }
  | {
      type: 'connector_grants';
      value: BotConnectorGrants;
    }
  | {
      type: 'connector_tools';
      value: ConnectorToolDefinition[];
    }
  | {
      type: 'connector_calls';
      value: ConnectorCallView[];
    }
  | {
      type: 'connector_audit';
      value: ConnectorAudit[];
    }
  | {
      type: 'workspace';
      value: OwnerWorkspace;
    }
  | {
      type: 'replay';
      value: EventReplay;
    }
  | {
      type: 'task';
      value: TaskView;
    }
  | {
      type: 'permission';
      value: Permission;
    }
  | {
      type: 'conversation';
      value: Conversation;
    }
  | {
      type: 'message';
      value: ConversationMessage;
    }
  | {
      type: 'document';
      value: Document;
    }
  | {
      type: 'documents';
      value: Document[];
    }
  | {
      type: 'configuration';
      value: InferenceConfiguration;
    }
  | {
      type: 'inference';
      value: InferenceView;
    }
  | {
      type: 'run';
      value: RunView;
    }
  | {
      type: 'computer_session';
      value: ComputerSession;
    }
  | {
      type: 'receipt';
      value: OwnerReceipt;
    }
  | {
      type: 'coding_calls';
      value: CodingCallView[];
    }
  | {
      type: 'artifact';
      value: {
        base64: string;
        declaration: OutputDeclaration;
      };
    };
export type ConnectorCallId = string;
export type ConnectorResult =
  | {
      tools: McpTool[];
      type: 'catalog';
    }
  | {
      type: 'tool';
      value: unknown;
    };
export type ConnectorCallState =
  | {
      type: 'reserved';
    }
  | {
      type: 'dispatch_possible';
    }
  | {
      outcome: ConnectorOutcome;
      type: 'resolved';
    };
export type ConnectorOutcome =
  | {
      reason: ConnectorFailure;
      type: 'not_sent';
    }
  | {
      result: ConnectorResult;
      type: 'finished';
    }
  | {
      reason: ConnectorFailure;
      type: 'unknown';
    };
export type ConnectorFailure =
  | 'authority'
  | 'authentication'
  | 'catalog_changed'
  | 'protocol'
  | 'bound'
  | 'transport'
  | 'deadline'
  | 'restart'
  | 'interrupted';
export type ManagerOutcome =
  | {
      outcome: MemoryOutcome;
      type: 'memory';
    }
  | {
      outcome: VaultOutcome;
      type: 'vault';
    };
export type MemoryOutcome =
  | {
      entries: MemoryEntry[];
      scope: MemoryScope;
      type: 'listed';
    }
  | {
      entry: MemoryEntry;
      type: 'created';
    }
  | {
      entry: MemoryEntry;
      type: 'updated';
    }
  | {
      id: MemoryId;
      revision: number;
      scope: MemoryScope;
      type: 'deleted';
    }
  | {
      reason: MemoryRefusal;
      type: 'refused';
    };
export type MemoryId = string;
export type MemoryScope =
  | {
      bot: BotId;
      type: 'individual';
    }
  | {
      type: 'shared';
    };
export type MemorySource =
  | {
      bot: BotId;
      run: RunId;
      turn: RunTurnId;
      type: 'run';
    }
  | {
      sources: SourceReference[];
      type: 'imported';
    };
export type MemoryRefusal =
  'invalid_input' | 'not_found' | 'revision_conflict' | 'capacity' | 'protected_content' | 'needs_attention';
export type VaultOutcome =
  | {
      type: 'provider_key_setup';
    }
  | {
      type: 'reusable_login_unsupported';
    };
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
export type RunAttemptId = string;
export type RunTurnPhase =
  | {
      type: 'sending';
    }
  | {
      type: 'responded';
    }
  | {
      reason: InferenceNotSentReason;
      type: 'not_sent';
    }
  | {
      reason: InferenceAttentionReason;
      type: 'awaiting_attention';
    };
export type ToolProposal =
  | {
      arguments: ComputerProposal;
      tool: 'computer';
    }
  | {
      arguments: ProcessProposal;
      tool: 'process';
    }
  | {
      arguments: VibeProposal;
      tool: 'vibe';
    }
  | {
      arguments: ConnectorProposal;
      tool: 'connector';
    }
  | {
      arguments: RequestWakeup;
      tool: 'request_wakeup';
    }
  | {
      arguments: MemoryProposal;
      tool: 'memory';
    }
  | {
      arguments: RequestVaultEntry;
      tool: 'request_vault_entry';
    };
export type MemoryProposal =
  | {
      action: 'list';
      scope: MemoryScopeSelection;
    }
  | {
      action: 'create';
      markdown: string;
      scope: MemoryScopeSelection;
    }
  | {
      action: 'update';
      expected_revision: number;
      id: MemoryId;
      markdown: string;
      scope: MemoryScopeSelection;
    }
  | {
      action: 'delete';
      expected_revision: number;
      id: MemoryId;
      scope: MemoryScopeSelection;
    };
export type MemoryScopeSelection = 'individual' | 'shared';
export type VaultPurpose = 'provider_key' | 'reusable_login';
export type RunMessage =
  | {
      content: string;
      images: AuthorizedImage[];
      role: 'user_multimodal';
    }
  | {
      content: string;
      role: 'user';
    }
  | {
      content: string;
      role: 'assistant';
      tool_call?: RunToolCall | null;
    }
  | {
      content: string;
      role: 'tool';
      tool_call_id: string;
    };
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
export type OwnerServerMessage =
  | {
      device: DeviceId;
      manager: ManagerId;
      type: 'welcome';
      version: number;
    }
  | {
      code: OwnerError;
      id?: RequestId | null;
      type: 'error';
    }
  | {
      head: number;
      type: 'notification';
    }
  | {
      type: 'pong';
    }
  | {
      bytes: number;
      chunks: number;
      digest: string;
      id: TransferId;
      request?: RequestId | null;
      type: 'transfer_start';
    }
  | {
      data: string;
      id: TransferId;
      index: number;
      type: 'transfer_chunk';
    }
  | {
      id: TransferId;
      type: 'transfer_complete';
    };
export type OwnerError =
  | {
      type: 'invalid_input';
    }
  | {
      type: 'unauthorized';
    }
  | {
      type: 'not_found';
    }
  | {
      type: 'conflict';
    }
  | {
      type: 'locked';
    }
  | {
      type: 'unlock_failed';
    }
  | {
      type: 'rate_limited';
    }
  | {
      type: 'busy';
    }
  | {
      type: 'internal';
    }
  | {
      type: 'inference_unverified';
    }
  | {
      type: 'inference_budget';
    }
  | {
      code: ErrorCode;
      type: 'protocol';
    };
export type TransferId = string;
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
export type Capability =
  | 'logical_sessions'
  | 'input_fencing'
  | 'test_input'
  | 'process_systemd_v1'
  | 'vibe_v1'
  | 'computer_v1'
  | 'connector_v1';
export type ReleaseChannel = 'stable' | 'experimental';

export interface Protocol {
  coding_calls: CodingCallView[];
  coding_result: VibeResult;
  document: WriteDocument;
  inference: SubmitInference;
  inference_configuration: ConfigureInference;
  inference_verification: VerifyInference;
  manager: Envelope2;
  manager_trust: ManagerTrust;
  owner: OwnerSnapshot;
  owner_client: OwnerClientMessage;
  owner_device: OwnerDevice;
  owner_hello: OwnerHello;
  owner_invitation: OwnerInvitation;
  owner_reply: OwnerReply;
  owner_server: OwnerServerMessage;
  replay: EventReplay;
  run: SubmitRun;
  run_view: RunView;
  worker: Envelope;
}
export interface CodingCallView {
  accounting: InferenceAccounting;
  id: InferenceAttemptId;
  phase: CodingCallPhase;
  provider_request_id?: string | null;
  reservation: InferenceReservation;
  task: TaskId;
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
export interface VibeResult {
  approval_denied: number;
  calls: number;
  exit_code: number;
  native_session: RequestId;
  tool_errors: number;
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
export interface VibeSpec {
  limits: RunLimits;
  policy: VibePolicy;
  prompt: string;
  session: VibeSession;
}
export interface RunLimits {
  deadline: number;
  max_concurrent_operations: number;
  max_cost_microusd: number;
  max_operations: number;
  max_output_tokens: number;
  max_total_tokens: number;
  max_turns: number;
}
export interface VibePolicy {
  access: VibeAccess;
  account: ProcessAccount;
  configuration_revision: number;
  connectors?: ConnectorToolPolicy | null;
  model_id: string;
  outputs: OutputDeclaration[];
  resources: ProcessLimits;
  runtime: CertificateFingerprint;
  workspace: CodingWorkspace;
}
export interface ConnectorToolPolicy {
  max_calls: number;
  max_result_bytes: number;
  runner: ConnectorRunner;
  tools: ConnectorToolBinding[];
}
export interface ConnectorRunner {
  account: ProcessAccount;
  limits: ProcessLimits;
  runtime: CertificateFingerprint;
}
export interface ConnectorToolBinding {
  catalog_revision: number;
  connection: ConnectionVersion;
  grant_revision: number;
  name: string;
  schema_digest: CertificateFingerprint;
}
export interface ConnectionVersion {
  connection: ConnectionId;
  credential_epoch: number;
  revision: number;
}
export interface OutputDeclaration {
  max_bytes: number;
  media_type: MediaType;
  name: string;
  slot: string;
}
export interface CodingWorkspace {
  id: CodingWorkspaceId;
  path: string;
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
export interface ComputerRuntime {
  sha256: CertificateFingerprint;
}
export interface Viewport {
  height: number;
  width: number;
}
export interface ConnectorSpec {
  deadline: number;
  operation: ConnectorOperation;
  result: OutputDeclaration;
  runner: ConnectorRunner;
}
export interface ConnectorProposal {
  arguments: unknown;
  tool: ConnectorToolBinding;
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
export interface ManagerTrust {
  ca_pem: string;
  endpoint: string;
  leaf: CertificateFingerprint;
  manager: ManagerId;
  server_name: string;
}
export interface OwnerSnapshot {
  connections?: ConnectionSummary[];
  conversations: Conversation[];
  documents: Document[];
  inference_configurations: InferenceConfiguration[];
  inferences: InferenceView[];
  messages: ConversationMessage[];
  permissions: Permission[];
  routine_state?: RoutineSnapshot;
  runs: RunSummary[];
  tasks: TaskView[];
}
export interface ConnectionSummary {
  availability: ConnectionAvailability;
  catalog_revision: number;
  id: ConnectionId;
  label: string;
  tool_count: number;
  version: ConnectionVersion;
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
export interface RoutineSnapshot {
  budget?: RoutineBudget | null;
  inbox: InboxEntry[];
  occurrences: RoutineOccurrence[];
  publishers: PublisherView[];
  routines: RoutineView[];
}
export interface RoutineBudget {
  max_cost_microusd: number;
  max_runs: number;
  max_tokens: number;
}
export interface InboxEntry {
  acknowledged: boolean;
  bot: BotId;
  created_at: number;
  id: InboxId;
  message: string;
  occurrence: OccurrenceId;
  routine: RoutineId;
  run?: RunId | null;
  status: OccurrenceStatus;
}
export interface RoutineOccurrence {
  id: OccurrenceId;
  revision: number;
  routine: RoutineId;
  run?: RunId | null;
  scheduled_at: number;
  skipped_count: number;
  status: OccurrenceStatus;
  trigger_key: string;
  updated_at: number;
}
export interface PublisherView {
  bot: BotId;
  id: PublisherId;
  last_error?: string | null;
  last_received_at?: number | null;
  name: string;
  revision: number;
  revoked: boolean;
  topics: string[];
}
export interface RoutineView {
  definition: RoutineDefinition;
  id: RoutineId;
  next_at?: number | null;
  revision: number;
  state: RoutineState;
}
export interface RoutineDefinition {
  bot: BotId;
  catchup_seconds: number;
  name: string;
  template: RunTemplate;
  trigger: RoutineTrigger;
  wakeups?: WakeupGrant | null;
}
export interface RunTemplate {
  configuration_revision: number;
  duration_seconds: number;
  execution_policy: RunExecutionPolicy;
  max_cost_microusd: number;
  max_operations: number;
  max_output_tokens: number;
  max_total_tokens: number;
  max_turns: number;
  permission_revision: number;
  prompt: string;
  session: ComputerSessionId;
  session_revision: Revision;
}
export interface RunExecutionPolicy {
  computer?: ComputerToolPolicy | null;
  connectors?: ConnectorToolPolicy | null;
  process?: ProcessToolPolicy | null;
  vibe?: VibeToolPolicy | null;
  vision?: VisionPolicy | null;
}
export interface ComputerToolPolicy {
  account: ProcessAccount;
  limits: ProcessLimits;
  outputs: OutputDeclaration[];
  profile: ComputerProfile;
  runtime: ComputerRuntime;
  viewport: Viewport;
}
export interface ProcessToolPolicy {
  account: ProcessAccount;
  environment: {
    [k: string]: string;
  };
  limits: ProcessLimits;
  profile: ProcessProfile;
}
export interface VibeToolPolicy {
  limits: RunLimits;
  policy: VibePolicy;
}
export interface VisionPolicy {
  allowed_slots: string[];
  max_image_bytes: number;
  max_images: number;
  max_total_pixels: number;
}
export interface WakeupGrant {
  horizon_seconds: number;
  max_pending: number;
  max_total: number;
  minimum_spacing_seconds: number;
}
export interface RunSummary {
  accounting: RunAccounting;
  bot: BotId;
  cancellation_requested_at?: number | null;
  created_at: number;
  id: RunId;
  phase: RunPhase;
  updated_at: number;
}
export interface RunAccounting {
  admitted_coding_calls?: number;
  admitted_operations: number;
  admitted_turns: number;
  held_cost_microusd: number;
  held_tokens: number;
  settled_cost_microusd: number;
  settled_tokens: number;
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
export interface RoutineCommand {
  action: RoutineAction;
  id: RequestId;
}
export interface InstallConnection {
  id: ConnectionId;
  label: string;
  transport: ConnectionTransport;
}
export interface DiscoverConnection {
  deadline: number;
  expected: ConnectionVersion;
  runner: ConnectorRunner;
  session: ComputerSessionId;
  task: TaskId;
}
export interface ReplaceConnectionSecret {
  expected_revision: number;
  secret: ConnectionSecretInput;
}
export interface SetConnectorGrants {
  catalog_revision: number;
  connection: ConnectionVersion;
  expected_revision: number;
  tools: string[];
}
export interface SubmitRun {
  bot: BotId;
  configuration_revision: number;
  execution_policy: RunExecutionPolicy;
  id: RunId;
  images?: ImageReference[];
  limits: RunLimits;
  prompt: string;
  session?: ComputerSessionId | null;
}
export interface ImageReference {
  artifact: ArtifactId;
  sha256: CertificateFingerprint;
  slot: string;
  task: TaskId;
}
export interface OwnerDevice {
  created_at: number;
  fingerprint: DeviceFingerprint;
  id: DeviceId;
  label: string;
  revoked: boolean;
  role: DeviceRole;
}
export interface OwnerInvitation {
  expires_at: number;
  fingerprint: DeviceFingerprint;
  invitation: string;
  role: DeviceRole;
  trust: ManagerTrust;
}
export interface RoutineControlResult {
  message: string;
  request: RequestId;
}
export interface ConnectionView {
  availability: ConnectionAvailability;
  catalog_revision: number;
  id: ConnectionId;
  label: string;
  tools: McpTool[];
  transport: ConnectionTransport;
  version: ConnectionVersion;
}
export interface McpTool {
  description: string;
  input_schema: unknown;
  name: string;
}
export interface BotConnectorGrants {
  bot: BotId;
  connection: ConnectionId;
  revision: number;
  tools: ConnectorToolBinding[];
}
export interface ConnectorToolDefinition {
  binding: ConnectorToolBinding;
  description: string;
  input_schema: unknown;
}
export interface ConnectorCallView {
  argument_digest: CertificateFingerprint;
  connection: ConnectionVersion;
  created_at: number;
  execution: ExecutionId;
  id: ConnectorCallId;
  late_result?: ConnectorResult | null;
  revoked_during_call_at?: number | null;
  state: ConnectorCallState;
  task: TaskId;
  tool?: ConnectorToolBinding | null;
}
export interface ConnectorAudit {
  at: number;
  call?: ConnectorCallId | null;
  code: string;
  connection: ConnectionId;
  digest?: CertificateFingerprint | null;
  sequence: number;
}
export interface OwnerWorkspace {
  bots: WorkspaceBot[];
  cursor: number;
  manager: ManagerId;
  owner: string;
  run_ready_bots: BotId[];
  runs: WorkspaceRun[];
  sessions: ComputerSession[];
  state: OwnerSnapshot;
  vault_unlocked: boolean;
  workers: WorkspaceWorker[];
}
export interface WorkspaceBot {
  created_at: number;
  id: BotId;
  name: string;
}
export interface WorkspaceRun {
  request?: SubmitRun | null;
  summary: RunSummary;
  turns: WorkspaceRunTurn[];
}
export interface WorkspaceRunTurn {
  id: RunTurnId;
  manager_outcome?: ManagerOutcome | null;
  ordinal: number;
  result?: string | null;
  task?: TaskId | null;
  text?: string | null;
  truncated: boolean;
}
export interface MemoryEntry {
  id: MemoryId;
  markdown: string;
  revision: number;
  scope: MemoryScope;
  source: MemorySource;
}
export interface WorkspaceWorker {
  id: WorkerId;
  phase: string;
  revoked: boolean;
}
export interface AuthoritativeEvent {
  entity: string;
  kind: string;
  sequence: number;
}
export interface RunView {
  archived_turns?: ArchivedRunTurn[];
  configuration: InferenceConfiguration;
  request: SubmitRun;
  summary: RunSummary;
  turns: RunTurn[];
}
export interface ArchivedRunTurn {
  accounting: InferenceAccounting;
  attempt: RunAttemptId;
  finish_reason?: InferenceFinishReason | null;
  id: RunTurnId;
  manager_outcome?: ManagerOutcome | null;
  messages_sha256: string;
  ordinal: number;
  phase: RunTurnPhase;
  provider_request_id?: string | null;
  request_sha256: string;
  reservation: InferenceReservation;
  response_sha256: string;
  text?: string | null;
  tool_call_id?: string | null;
  usage?: InferenceUsage | null;
}
export interface RunTurn {
  accounting: InferenceAccounting;
  attempt: RunAttemptId;
  finish_reason?: InferenceFinishReason | null;
  id: RunTurnId;
  intent?: RunToolIntent | null;
  manager_result?: ManagerToolResult | null;
  messages: RunMessage[];
  ordinal: number;
  phase: RunTurnPhase;
  provider_request_id?: string | null;
  reservation: InferenceReservation;
  response?: RunMessage | null;
  usage?: InferenceUsage | null;
}
export interface RunToolIntent {
  call: RunToolCall;
  command: CommandId;
  execution: ExecutionId;
  operation: OperationId;
  result?: string | null;
  task: TaskId;
  work: WorkSpec;
}
export interface RunToolCall {
  id: string;
  proposal: ToolProposal;
}
export interface ComputerProposal {
  operations: ComputerOperation[];
  outputs: string[];
}
export interface ProcessProposal {
  args: string[];
  cwd: string;
  executable: string;
}
export interface VibeProposal {
  prompt: string;
  session: VibeSession;
}
export interface RequestWakeup {
  at: number;
  prompt: string;
}
export interface RequestVaultEntry {
  purpose: VaultPurpose;
}
export interface ManagerToolResult {
  call: RunToolCall;
  outcome?: ManagerOutcome | null;
  result: string;
}
export interface AuthorizedImage {
  height: number;
  length: number;
  reference: ImageReference;
  width: number;
}
export interface OwnerReceipt {
  acknowledgment: Acknowledgment;
  bot_inputs: number;
  command_id: CommandId;
  generation: Generation;
  last_event?: TaskEvent | null;
}
export interface ArtifactReference {
  id: ArtifactId;
  length: number;
  media_type: MediaType;
  sha256: CertificateFingerprint;
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
