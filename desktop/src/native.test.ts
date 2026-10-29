import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { nativeApplication } from './native';
import { pairedApplication } from './paired-native';
import { signedIn, signedOut } from './fixtures.test-support';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
const native = vi.mocked(invoke);
beforeEach(() => {
  native.mockReset();
});
describe('native boundary', () => {
  it('invokes only the narrow Memory write command and validates its snapshot', async () => {
    const snapshot = {
      generation: 1,
      revision: 1,
      status: 'online',
      issue: null,
      pairing: null,
      candidate: null,
      workspace: null,
      review: null,
      intents: [],
    };
    native.mockResolvedValueOnce(snapshot);
    const bot = signedIn().bots[0];
    if (!bot) throw new Error('Missing fixture bot');
    const input = {
      bot: bot.id,
      expectedRevision: 2,
      markdown: 'Corrected memory',
    };
    await pairedApplication.writeMemory(input);
    expect(native).toHaveBeenCalledWith('write_memory', input);
    native.mockResolvedValueOnce({ ...snapshot, secret: 'unexpected' });
    await expect(pairedApplication.writeMemory(input)).rejects.toThrow();
  });

  it('parses snapshots and rejects unexpected credential-bearing fields', async () => {
    native.mockResolvedValueOnce(signedIn());
    expect(await nativeApplication.inspect()).toEqual(signedIn());
    native.mockResolvedValueOnce({ ...signedIn(), token: 'SYNTHETIC credential' });
    await expect(nativeApplication.refresh()).rejects.toMatchObject({ code: 'invalid_response' });
  });
  it('parses operation errors without exposing unknown raw native failures', async () => {
    native.mockRejectedValueOnce({ code: 'unauthorized', snapshot: signedOut() });
    await expect(nativeApplication.refresh()).rejects.toMatchObject({
      code: 'unauthorized',
      snapshot: signedOut(),
    });
    native.mockRejectedValueOnce('SYNTHETIC password or server response');
    await expect(nativeApplication.refresh()).rejects.toMatchObject({
      message: 'native_unavailable',
      snapshot: null,
    });
  });
  it('accepts sanitized authorization denial and rejects malformed worker states', async () => {
    native.mockRejectedValueOnce({ code: 'unauthorized' });
    await expect(nativeApplication.inspect()).rejects.toMatchObject({
      code: 'unauthorized',
      snapshot: null,
    });
    const snapshot = signedIn();
    native.mockResolvedValueOnce({
      ...snapshot,
      details: { ...snapshot.details, workers: [{ id: 'bad', revoked: false, phase: 'working' }] },
    });
    await expect(nativeApplication.refresh()).rejects.toMatchObject({ code: 'invalid_response' });
  });
});
