import { vi } from 'vitest';
import { snapshotSchema, type Application, type SignedIn } from './domain';
import type { UpdateFacts } from './update-domain';

export const idleFacts: UpdateFacts = {
  restart_version: null,
  report: null,
  available: null,
  protocol_match: true,
};

export const botId = '00000000-0000-4000-8000-000000000001';
export const secondBotId = '00000000-0000-4000-8000-000000000002';
export function signedIn(overrides: Partial<SignedIn> = {}): SignedIn {
  const value = snapshotSchema.parse({
    revision: 2,
    generation: 1,
    endpoint: '127.0.0.1:43120',
    connection: 'online',
    kind: 'signed_in',
    owner: { name: 'Synthetic owner' },
    bots: [
      { id: botId, name: 'Research', created_at: 1792960000 },
      { id: secondBotId, name: 'Writing', created_at: 1792960001 },
    ],
    expires_at: 1793046400,
    details: { vault: 'locked', provider: null, workers: [] },
    ...overrides,
  });
  if (value.kind !== 'signed_in') throw new Error('Invalid test fixture.');
  return value;
}
export function signedOut() {
  return snapshotSchema.parse({
    revision: 5,
    generation: 3,
    endpoint: '127.0.0.1:43120',
    connection: 'online',
    kind: 'signed_out',
    reason: 'user',
  });
}
export function application(initial = signedIn()) {
  return {
    inspect: vi.fn<Application['inspect']>().mockResolvedValue(initial),
    signIn: vi.fn<Application['signIn']>().mockResolvedValue(initial),
    refresh: vi.fn<Application['refresh']>().mockResolvedValue(initial),
    createBot: vi.fn<Application['createBot']>().mockResolvedValue(initial),
    signOut: vi.fn<Application['signOut']>().mockResolvedValue(signedOut()),
    testNotification: vi.fn<Application['testNotification']>().mockResolvedValue('submitted'),
    updateFacts: vi.fn<Application['updateFacts']>().mockResolvedValue(idleFacts),
    applyServerUpdate: vi.fn<Application['applyServerUpdate']>().mockResolvedValue(idleFacts),
    restartToUpdate: vi.fn<Application['restartToUpdate']>().mockResolvedValue(),
  } satisfies Application;
}
export function deferred<T>() {
  let resolve: (value: T) => void = () => {
    throw new Error('Promise is not initialized.');
  };
  const promise = new Promise<T>((fulfill) => {
    resolve = fulfill;
  });
  return { promise, resolve };
}
