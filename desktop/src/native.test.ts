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
  it('sends manager-only conversation without a session or worker tools', async () => {
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
    const bot = signedIn().bots[0];
    if (!bot) throw new Error('Missing fixture bot');
    const input = {
      bot: bot.id,
      session: null,
      text: 'Remember concise answers.',
      account: { uid: 1000, gid: 1000 },
      runtime: null,
      process: false,
      seconds: 120,
      turns: 4,
      tokens: 200000,
      cost_microusd: 200000,
      output_tokens: 1024,
    };
    native.mockResolvedValueOnce(snapshot);
    await pairedApplication.send(input);
    expect(native).toHaveBeenCalledWith('send_run', { input });
    expect('writeMemory' in pairedApplication).toBe(false);
    expect('createMemory' in pairedApplication).toBe(false);
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
