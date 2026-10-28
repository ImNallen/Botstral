import { z } from 'zod';
import { botIdSchema } from './domain';
const integer = z.number().int().nonnegative().safe();
const timestamp = z.number().int().safe();
export const routineIdSchema = z.uuid().brand<'RoutineId'>();
const occurrenceId = z.uuid().brand<'OccurrenceId'>();
const publisherId = z.uuid().brand<'PublisherId'>();
const runPhase = z.discriminatedUnion('type', [
  z.strictObject({ type: z.literal('ready') }),
  z.strictObject({ type: z.literal('calling'), turn: z.uuid() }),
  z.strictObject({ type: z.literal('waiting_for_tool'), turn: z.uuid(), task: z.uuid() }),
  z.strictObject({ type: z.literal('completed') }),
  z.strictObject({ type: z.literal('cancelling') }),
  z.strictObject({ type: z.literal('cancelled') }),
  z.strictObject({
    type: z.literal('stopped'),
    reason: z.enum([
      'turn_limit',
      'output_limit',
      'token_limit',
      'cost_limit',
      'operation_limit',
      'deadline',
      'configuration_changed',
      'vault_locked',
      'unsupported_proposal',
      'transcript_limit',
      'child_unavailable',
      'shutdown',
      'restart',
    ]),
  }),
  z.strictObject({
    type: z.literal('awaiting_attention'),
    reason: z.enum(['provider', 'worker', 'result_invalid', 'restart', 'shutdown']),
  }),
]);
export const occurrenceStatusSchema = z.discriminatedUnion('type', [
  z.strictObject({ type: z.literal('pending') }),
  z.strictObject({ type: z.literal('running') }),
  z.strictObject({ type: z.literal('decision'), reason: z.string() }),
  z.strictObject({ type: z.literal('finished'), phase: runPhase }),
  z.strictObject({ type: z.literal('blocked'), reason: z.string() }),
  z.strictObject({ type: z.literal('skipped'), reason: z.string() }),
]);
export const routineTriggerSchema = z.discriminatedUnion('type', [
  z.strictObject({ type: z.literal('once'), at: timestamp }),
  z.strictObject({ type: z.literal('interval'), anchor: timestamp, seconds: integer }),
  z.strictObject({
    type: z.literal('daily'),
    hour: integer,
    minute: integer,
    timezone: z.string(),
  }),
  z.strictObject({
    type: z.literal('event'),
    publisher: publisherId,
    topic: z.string(),
    equals: z.record(z.string(), z.union([z.string(), z.number().int().safe(), z.boolean()])),
  }),
]);
export const routineViewSchema = z.strictObject({
  id: routineIdSchema,
  bot: botIdSchema,
  revision: integer,
  state: z.enum(['enabled', 'paused', 'deleted']),
  name: z.string(),
  next_at: timestamp.nullable(),
  trigger: routineTriggerSchema,
  definition_json: z.string(),
});
export const occurrenceSchema = z.strictObject({
  id: occurrenceId,
  routine: routineIdSchema,
  revision: integer,
  trigger_key: z.string(),
  scheduled_at: timestamp,
  run: z.uuid().nullable(),
  status: occurrenceStatusSchema,
  skipped_count: integer,
  updated_at: timestamp,
});
export const inboxSchema = z.strictObject({
  id: z.uuid().brand<'InboxId'>(),
  occurrence: occurrenceId,
  bot: botIdSchema,
  routine: routineIdSchema,
  run: z.uuid().nullable(),
  message: z.string(),
  status: occurrenceStatusSchema,
  created_at: timestamp,
  acknowledged: z.boolean(),
});
export const publisherSchema = z.strictObject({
  id: publisherId,
  bot: botIdSchema,
  name: z.string(),
  topics: z.array(z.string()),
  revision: integer,
  revoked: z.boolean(),
  last_received_at: timestamp.nullable(),
  last_error: z.string().nullable(),
});
export const budgetSchema = z.strictObject({
  max_runs: integer,
  max_tokens: integer,
  max_cost_microusd: integer,
});
export const routinePanelSchema = z.strictObject({
  message: z.string().nullable(),
  history: z.array(occurrenceSchema),
  preview: z.array(timestamp),
});
export type RoutineView = z.infer<typeof routineViewSchema>;
export type Occurrence = z.infer<typeof occurrenceSchema>;
export type RoutineTrigger = z.infer<typeof routineTriggerSchema>;
export function occurrenceLabel(status: z.infer<typeof occurrenceStatusSchema>): string {
  switch (status.type) {
    case 'pending':
      return 'Pending wakeup';
    case 'running':
      return 'Running';
    case 'finished':
      return status.phase.type === 'stopped'
        ? `Stopped: ${status.phase.reason.replaceAll('_', ' ')}`
        : `Finished: ${status.phase.type.replaceAll('_', ' ')}`;
    case 'decision':
      return `Decision required: ${status.reason}`;
    case 'blocked':
      return `Blocked: ${status.reason}`;
    case 'skipped':
      return `Skipped: ${status.reason}`;
    default: {
      const exhaustive: never = status;
      return exhaustive;
    }
  }
}
