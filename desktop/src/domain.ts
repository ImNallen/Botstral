import { z } from 'zod';

export const botIdSchema = z.uuid().brand<'BotId'>();
const nameSchema = z.string().refine((value) => {
  const length = Array.from(value).length;
  return length >= 1 && length <= 100 && !/\p{Cc}/u.test(value);
}, 'Use 1 to 100 characters without control characters.');
export { nameSchema };
const botSchema = z.strictObject({
  id: botIdSchema,
  name: nameSchema,
  created_at: z.number().int().safe(),
});
const common = {
  revision: z.number().int().nonnegative().safe(),
  generation: z.number().int().nonnegative().safe(),
  endpoint: z.string().max(80),
  connection: z.enum(['unknown', 'online', 'offline', 'error']),
};
export const snapshotSchema = z.discriminatedUnion('kind', [
  z.strictObject({
    ...common,
    kind: z.literal('signed_out'),
    reason: z.enum(['initial', 'user', 'invalid_session', 'sign_in_failed']),
  }),
  z.strictObject({
    ...common,
    kind: z.literal('signed_in'),
    owner: z.strictObject({ name: nameSchema }),
    bots: z.array(botSchema),
    expires_at: z.number().int().safe(),
    details: z.strictObject({
      vault: z.enum(['locked', 'unlocked']),
      provider: z
        .strictObject({ provider: z.literal('mistral'), updated_at: z.number().int().safe() })
        .nullable(),
      workers: z.array(
        z.strictObject({
          id: z.uuid(),
          revoked: z.boolean(),
          phase: z.enum(['disconnected', 'synchronizing', 'ready']),
        }),
      ),
    }),
  }),
]);
export const errorCodeSchema = z.enum([
  'offline',
  'timeout',
  'unauthorized',
  'invalid_credentials',
  'invalid_input',
  'busy',
  'rate_limited',
  'conflict',
  'protocol',
  'manager_error',
  'stale',
]);
export const failureSchema = z.strictObject({ code: errorCodeSchema, snapshot: snapshotSchema });
export const notificationSchema = z.enum(['submitted', 'denied', 'unavailable']);
export type Snapshot = z.infer<typeof snapshotSchema>;
export type SignedIn = Extract<Snapshot, { kind: 'signed_in' }>;
export type Bot = SignedIn['bots'][number];
export type BotId = Bot['id'];
export type Creation = { id: BotId; name: string };
export type ErrorCode = z.infer<typeof errorCodeSchema> | 'native_unavailable' | 'invalid_response';
export type NotificationResult = z.infer<typeof notificationSchema>;
export class ApplicationError extends Error {
  constructor(
    readonly code: ErrorCode,
    readonly snapshot: Snapshot | null = null,
  ) {
    super(code);
  }
}

export interface Application {
  inspect(): Promise<Snapshot>;
  signIn(input: { endpoint: string; password: string }): Promise<Snapshot>;
  refresh(): Promise<Snapshot>;
  createBot(input: Creation): Promise<Snapshot>;
  signOut(): Promise<Snapshot>;
  testNotification(): Promise<NotificationResult>;
}

export const errorMessages: Record<ErrorCode, string> = {
  offline: 'The local manager is unavailable. Start it and try again.',
  timeout: 'The manager did not respond in time. The operation may still finish.',
  invalid_credentials: 'The owner password was not accepted. Try again.',
  unauthorized: 'Your session is no longer usable. Sign in again.',
  invalid_input: 'Check the address, password, or bot name and try again.',
  busy: 'The manager is busy. Wait a moment and try again.',
  rate_limited: 'Too many sign-in attempts. Wait one minute and try again.',
  conflict:
    'The manager could not accept this request. Retry with the saved bot identity if creation is uncertain.',
  protocol:
    'The manager returned an unsupported response. Check that your manager and app versions match.',
  manager_error: 'The manager could not complete the request. Try again.',
  stale: 'The session changed. Refresh to see its current state.',
  native_unavailable: 'The native app connection is unavailable. Restart Botstral and try again.',
  invalid_response: 'The native app returned an unsupported response. Restart Botstral.',
};
