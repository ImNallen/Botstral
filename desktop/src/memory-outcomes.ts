import { z } from 'zod';
import type { ManagerOutcome } from '@botstral/link';

const id = z.uuid();
const revision = z.number().int().positive().safe();
const scope = z.discriminatedUnion('type', [
  z.strictObject({ type: z.literal('individual'), bot: id }),
  z.strictObject({ type: z.literal('shared') }),
]);
const source = z.discriminatedUnion('type', [
  z.strictObject({ type: z.literal('run'), bot: id, run: id, turn: id }),
  z.strictObject({
    type: z.literal('imported'),
    sources: z
      .array(
        z.discriminatedUnion('type', [
          z.strictObject({ type: z.literal('task'), id }),
          z.strictObject({ type: z.literal('message'), id }),
          z.strictObject({ type: z.literal('external'), reference: z.string() }),
        ]),
      )
      .min(1)
      .max(8),
  }),
]);
const entry = z.strictObject({
  id,
  scope,
  revision,
  markdown: z.string().min(1).max(4096),
  source,
});
export const managerOutcomeSchema = z.discriminatedUnion('type', [
  z.strictObject({
    type: z.literal('memory'),
    outcome: z.discriminatedUnion('type', [
      z.strictObject({ type: z.literal('listed'), scope, entries: z.array(entry).max(16) }),
      z.strictObject({ type: z.literal('created'), entry }),
      z.strictObject({ type: z.literal('updated'), entry }),
      z.strictObject({ type: z.literal('deleted'), id, scope, revision }),
      z.strictObject({
        type: z.literal('refused'),
        reason: z.enum([
          'invalid_input',
          'not_found',
          'revision_conflict',
          'capacity',
          'protected_content',
          'needs_attention',
        ]),
      }),
    ]),
  }),
  z.strictObject({
    type: z.literal('vault'),
    outcome: z.discriminatedUnion('type', [
      z.strictObject({ type: z.literal('provider_key_setup') }),
      z.strictObject({ type: z.literal('reusable_login_unsupported') }),
      z.strictObject({ type: z.literal('login_entry'), bot: id }),
    ]),
  }),
]) satisfies z.ZodType<ManagerOutcome>;
