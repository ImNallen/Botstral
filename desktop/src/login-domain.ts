import { z } from 'zod';
import type { CredentialWriteStatus, LoginItemView, CredentialUseReview } from '@botstral/link';

const revision = z.number().int().positive().safe();
const metadataSchema = z.strictObject({
  label: z.string(),
  bots: z.discriminatedUnion('type', [
    z.strictObject({ type: z.literal('all_owner_bots') }),
    z.strictObject({ type: z.literal('selected'), bots: z.array(z.uuid()) }),
  ]),
  allowed_origins: z.array(z.string()),
  password_policy: z.enum(['every_fill', 'remembered_origins']),
});
export const itemSchema = z.strictObject({
  id: z.uuid(),
  revision,
  epoch: revision,
  state: z.enum(['active', 'removed']),
  metadata: metadataSchema,
  has_totp: z.boolean(),
}) satisfies z.ZodType<LoginItemView>;
export const statusSchema = z.discriminatedUnion('type', [
  z.strictObject({ type: z.literal('committed'), request: z.uuid(), item: itemSchema }),
  z.strictObject({ type: z.literal('closed_without_commit'), request: z.uuid() }),
  z.strictObject({ type: z.literal('unresolved'), request: z.uuid() }),
]) satisfies z.ZodType<CredentialWriteStatus>;

export const reviewSchema = z.strictObject({
  id: z.uuid(),
  release: z.uuid(),
  item: itemSchema,
  purpose: z.enum(['password', 'totp']),
  binding: z.strictObject({
    owner: z.uuid(),
    bot: z.uuid(),
    task: z.uuid(),
    operation: z.uuid(),
    arguments: z.string(),
    permission_revision: revision,
    approval: z.uuid(),
    execution: z.uuid(),
    expires_at: z.number().int().safe(),
  }),
  fence: z.strictObject({
    generation: revision,
    session_id: z.uuid(),
    revision,
    owner: z.enum(['owner', 'bot']),
  }),
  incarnation: z.uuid(),
  destination: z.strictObject({
    profile: z.uuid(),
    browser: z.uuid(),
    page: z.uuid(),
    top_document: z.uuid(),
    frame: z.uuid(),
    frame_document: z.uuid(),
    top_origin: z.string(),
    frame_origin: z.string(),
    plan: z.strictObject({
      version: revision,
      origin: z.string(),
      route: z.string(),
      frame: z.string().nullable(),
      form: z.string(),
      username: z.string(),
      password: z.string(),
      totp: z.string(),
      submitter: z.string(),
      action: z.string(),
    }),
    form: z.uuid(),
    fields: z.array(
      z.strictObject({
        object: z.uuid(),
        role: z.enum(['username', 'password', 'totp']),
        name: z.string(),
        input_type: z.string(),
      }),
    ),
    submitter: z.uuid(),
    certain: z.boolean(),
  }),
  expires_at: z.number().int().safe(),
  state: z.enum([
    'awaiting_approval',
    'denied',
    'authorized',
    'released',
    'applied',
    'refused',
    'uncertain',
    'revoked',
    'expired',
  ]),
}) satisfies z.ZodType<CredentialUseReview>;
