import { z } from 'zod';
import { botIdSchema } from './domain';

const id = z.uuid();
const taskId = id.brand<'TaskId'>();
const runId = id.brand<'RunId'>();
const sessionId = id.brand<'ComputerSessionId'>();
const workerId = id.brand<'WorkerId'>();
const artifactId = id.brand<'ArtifactId'>();
const reviewId = id.brand<'ReviewId'>();
const candidateId = id.brand<'PairingCandidateId'>();
const intentId = id.brand<'IntentId'>();
const deviceId = id.brand<'DeviceId'>();
const digest = z.string().regex(/^[a-f0-9]{64}$/);
const integer = z.number().int().nonnegative().safe();
const content = z.strictObject({ sha256: digest, length: integer });
const output = z.strictObject({
  declaration: z.strictObject({
    slot: z.string(),
    name: z.string(),
    media_type: z.enum(['png', 'text_plain', 'octet_stream']),
    max_bytes: integer,
  }),
  state: z.discriminatedUnion('type', [
    z.strictObject({ type: z.literal('pending') }),
    z.strictObject({ type: z.literal('transferring'), id: artifactId, content }),
    z.strictObject({ type: z.literal('available'), id: artifactId, content }),
    z.strictObject({
      type: z.literal('unavailable'),
      reason: z.enum(['missing', 'rejected', 'execution_uncertain', 'corrupt']),
    }),
  ]),
});
const workspace = z.strictObject({
  manager: id,
  cursor: integer,
  owner: z.string(),
  vault_unlocked: z.boolean(),
  bots: z.array(
    z.strictObject({ id: botIdSchema, name: z.string(), created_at: z.number().int().safe() }),
  ),
  workers: z.array(
    z.strictObject({
      id: workerId,
      revoked: z.boolean(),
      phase: z.enum(['disconnected', 'synchronizing', 'ready']),
    }),
  ),
  sessions: z.array(
    z.strictObject({
      id: sessionId,
      bot_id: botIdSchema,
      worker_id: workerId,
      owner: z.enum(['bot', 'owner']),
      revision: integer,
      status: z.enum(['disconnected', 'pending', 'confirmed']),
    }),
  ),
  permissions: z.array(z.strictObject({ bot: botIdSchema, revision: integer, allow: z.boolean() })),
  configurations: z.array(
    z.strictObject({
      bot: botIdSchema,
      model: z.string(),
      ready: z.boolean(),
      context_tokens: integer,
    }),
  ),
  runs: z.array(
    z.strictObject({
      id: runId,
      bot: botIdSchema,
      prompt: z.string(),
      reason: z.string().nullable(),
      phase: z.enum([
        'ready',
        'calling',
        'waiting_for_tool',
        'completed',
        'cancelling',
        'cancelled',
        'stopped',
        'awaiting_attention',
      ]),
      created_at: z.number().int().safe(),
      turns: z.array(
        z.strictObject({
          id,
          ordinal: integer,
          text: z.string().nullable(),
          task: taskId.nullable(),
          result: z.string().nullable(),
        }),
      ),
    }),
  ),
  tasks: z.array(
    z.strictObject({
      id: taskId,
      bot: botIdSchema,
      state: z.enum([
        'queued',
        'running',
        'awaiting_approval',
        'cancelling',
        'completed',
        'failed',
        'cancelled',
        'awaiting_attention',
      ]),
      progress: integer,
      action: z.string(),
      approval_revoked: z.boolean(),
      approval_consumed: z.boolean(),
      outputs: z.array(output),
      evidence: z.string(),
    }),
  ),
});
export const remoteSnapshotSchema = z.strictObject({
  generation: integer,
  revision: integer,
  status: z.enum([
    'unpaired',
    'pairing',
    'online',
    'offline',
    'blocked',
    'suspended',
    'signed_out',
  ]),
  issue: z.string().nullable(),
  pairing: z.strictObject({ fingerprint: digest, label: z.string() }).nullable(),
  candidate: z
    .strictObject({
      id: candidateId,
      manager: id,
      endpoint: z.string(),
      server_name: z.string(),
      leaf: digest,
      fingerprint: digest,
      role: z.literal('owner'),
      expires_at: z.number().int().safe(),
    })
    .nullable(),
  workspace: workspace.nullable(),
  review: z
    .strictObject({
      id: reviewId,
      task: taskId,
      action: z.string(),
      expires_at: z.number().int().safe(),
      permission_revision: integer,
    })
    .nullable(),
  intents: z.array(
    z.strictObject({
      id: intentId,
      description: z.string(),
      identity: z.string(),
      status: z.enum(['unknown', 'pending', 'attention', 'confirmed', 'refused']),
    }),
  ),
});
export const deviceSchema = z.strictObject({
  id: deviceId,
  label: z.string(),
  fingerprint: digest,
  role: z.enum(['owner', 'worker']),
  revoked: z.boolean(),
  created_at: z.number().int().safe(),
});
export type RemoteSnapshot = z.infer<typeof remoteSnapshotSchema>;
export type Workspace = z.infer<typeof workspace>;
export type Task = Workspace['tasks'][number];
export type Device = z.infer<typeof deviceSchema>;
export type SendInput = {
  bot: Workspace['bots'][number]['id'];
  session: Workspace['sessions'][number]['id'];
  text: string;
  account: { uid: number; gid: number };
  runtime: string | null;
  process: boolean;
  seconds: number;
  turns: number;
  tokens: number;
  cost_microusd: number;
  output_tokens: number;
};
export interface PairedApplication {
  inspect(): Promise<RemoteSnapshot>;
  observe(receive: (value: RemoteSnapshot) => void): Promise<() => void>;
  beginPairing(label: string): Promise<RemoteSnapshot>;
  importPairing(): Promise<RemoteSnapshot>;
  importTrust(): Promise<RemoteSnapshot>;
  pairLocal(): Promise<RemoteSnapshot>;
  confirmPairing(
    candidate: NonNullable<RemoteSnapshot['candidate']>['id'],
  ): Promise<RemoteSnapshot>;
  refresh(): Promise<RemoteSnapshot>;
  send(input: SendInput): Promise<RemoteSnapshot>;
  openSession(
    bot: SendInput['bot'],
    worker: Workspace['workers'][number]['id'],
  ): Promise<RemoteSnapshot>;
  permission(bot: SendInput['bot'], expected: number, allow: boolean): Promise<RemoteSnapshot>;
  review(task: Task['id']): Promise<RemoteSnapshot>;
  approve(review: NonNullable<RemoteSnapshot['review']>['id']): Promise<RemoteSnapshot>;
  cancelTask(task: Task['id']): Promise<RemoteSnapshot>;
  cancelRun(run: Workspace['runs'][number]['id']): Promise<RemoteSnapshot>;
  revokeApproval(task: Task['id']): Promise<RemoteSnapshot>;
  exportArtifact(task: Task['id'], artifact: z.infer<typeof artifactId>): Promise<boolean>;
  acknowledgeUncertainty(intent: RemoteSnapshot['intents'][number]['id']): Promise<RemoteSnapshot>;
  signOut(): Promise<RemoteSnapshot>;
  lifecycle(suspended: boolean): Promise<RemoteSnapshot>;
  devices(): Promise<Device[]>;
  revokeDevice(device: Device['id']): Promise<void>;
  issueInvitation(fingerprint: string, label: string): Promise<boolean>;
}
export const remoteIssues: Record<string, string> = {
  offline:
    'The paired manager is offline. Cached activity is read-only. Reconnect to recover current state.',
  trust:
    'Manager trust failed. Import an authenticated local trust export. The saved pin has not changed.',
  revoked:
    'This device was revoked. Protected activity has been cleared. Authorize a new pairing locally.',
  revocation_storage_failed:
    'This device was revoked. Protected activity is blocked, but local revocation cleanup failed. Retry cleanup before restarting.',
  keychain_or_storage:
    'Native credential storage is locked or unavailable. Unlock the system keychain and try again.',
  incompatible_manager:
    'This manager does not support the desktop workspace protocol. Upgrade the manager.',
  unknown_outcome:
    'The reply was lost. The saved operation identity is retained. Refresh to reconcile. Do not submit a replacement.',
  unresolved_intent:
    'A saved operation has an unknown outcome. Refresh to reconcile it before sending more work.',
  journal_unavailable:
    'The saved operation journal cannot be validated. New work is blocked. Restore its protected permissions or recover the profile before continuing.',
  journal_full:
    'The saved operation journal is full. Reconcile uncertain work before starting another action.',
  inference_unverified:
    'Verify a tool-capable inference configuration for this bot in trusted manager administration.',
  configure_inference_locally:
    'Configure and verify inference for this bot through trusted manager administration.',
  vault_locked: 'Unlock the manager vault through trusted local administration.',
  permission_denied: 'Allow execution for this bot before reviewing an action.',
  stale_review: 'This review expired or the workspace changed. Review the current action again.',
  refused: 'The manager refused the request. Refresh and review its current state.',
  budget: 'The configured model needs a larger token or cost budget. Review execution settings.',
  protected_file_required:
    'Choose a protected file owned by you with read and write permissions only for your account.',
  credential_deletion_failed:
    'Signed out. The native credential could not be deleted. It will not be used automatically.',
  sign_out_storage_failed:
    'The signed-out marker could not be saved. Resolve the native storage failure before quitting.',
  invalid_invitation: 'The invitation is invalid, expired, or bound to another device fingerprint.',
  begin_pairing_first: 'Create a native pairing request before importing an invitation.',
  local_invitation_failed:
    'Local invitation creation failed. Sign in to the trusted loopback manager and choose its trust export.',
  cancelled: 'The native dialog was cancelled.',
  invalid_input: 'Check the execution settings and try again.',
  artifact_invalid: 'The artifact failed verification. No completed export was written.',
  artifact_unavailable: 'This artifact is not available from the manager.',
  save_failed: 'The native export could not be completed.',
};
