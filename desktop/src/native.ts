import { invoke } from '@tauri-apps/api/core';
import { z } from 'zod';
import {
  ApplicationError,
  failureSchema,
  snapshotSchema,
  notificationSchema,
  type Application,
} from './domain';

const deniedSchema = z.strictObject({ code: z.literal('unauthorized') });
async function call(command: string, args?: Record<string, unknown>): Promise<unknown> {
  try {
    return await invoke<unknown>(command, args);
  } catch (error: unknown) {
    const failure = failureSchema.safeParse(error);
    if (failure.success) throw new ApplicationError(failure.data.code, failure.data.snapshot);
    if (deniedSchema.safeParse(error).success) throw new ApplicationError('unauthorized');
    throw new ApplicationError('native_unavailable');
  }
}
async function snapshot(command: string, args?: Record<string, unknown>) {
  const result = snapshotSchema.safeParse(await call(command, args));
  if (!result.success) throw new ApplicationError('invalid_response');
  return result.data;
}
export const nativeApplication: Application = {
  inspect: () => snapshot('inspect'),
  signIn: (input) => snapshot('sign_in', input),
  refresh: () => snapshot('refresh'),
  createBot: (input) => snapshot('create_bot', input),
  signOut: () => snapshot('sign_out'),
  testNotification: async () => {
    const result = notificationSchema.safeParse(await call('test_notification'));
    if (!result.success) throw new ApplicationError('invalid_response');
    return result.data;
  },
};
