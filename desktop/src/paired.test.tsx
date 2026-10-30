import { act, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { PairedController } from './paired-application';
import { PairedWorkspace } from './PairedWorkspace';
import {
  remoteSnapshotSchema,
  savedResultsSchema,
  type PairedApplication,
  type RemoteSnapshot,
} from './paired-domain';
import { deferred, application as localApplication, signedOut } from './fixtures.test-support';
import { ApplicationController } from './application';
import type { Snapshot } from './domain';

HTMLDialogElement.prototype.showModal = function () {
  this.open = true;
};
HTMLDialogElement.prototype.close = function () {
  this.open = false;
};

const id = '00000000-0000-4000-8000-000000000001';
const task = '00000000-0000-4000-8000-000000000002';
function snapshot(status: 'online' | 'offline' = 'online') {
  return remoteSnapshotSchema.parse({
    generation: 2,
    revision: 10,
    status,
    issue: null,
    pairing: null,
    candidate: null,
    review: null,
    intents: [],
    workspace: {
      manager: id,
      cursor: 10,
      owner: 'Owner',
      vault_unlocked: true,
      bots: [{ id, name: 'Research', created_at: 1 }],
      workers: [],
      sessions: [],
      permissions: [],
      configurations: [],
      runs: [
        {
          id,
          bot: id,
          prompt: 'Actual user request',
          reason: null,
          phase: 'waiting_for_tool',
          created_at: 1,
          turns: [{ id, ordinal: 1, text: '<script>remote content</script>', task, result: null }],
        },
      ],
      tasks: [
        {
          id: task,
          bot: id,
          state: 'awaiting_approval',
          progress: 0,
          action: 'Normalized action',
          approval_revoked: false,
          approval_consumed: false,
          outputs: [],
          evidence: '[]',
        },
      ],
    },
  });
}
function application(initial = snapshot()) {
  const result = () => Promise.resolve(initial);
  return {
    savedResults: async () => ({
      generation: initial.generation,
      manager: id,
      page: { entries: [], next: null },
    }),
    sourceTask: async () => {
      throw new Error('unavailable');
    },
    runDetails: result,
    routine: result,
    routineText: result,
    saveRoutine: result,
    routineHistory: result,
    routinePreview: result,
    inspect: vi.fn(result),
    observe: vi.fn<PairedApplication['observe']>().mockResolvedValue(() => {}),
    beginPairing: result,
    importPairing: result,
    importTrust: result,
    pairLocal: result,
    confirmPairing: result,
    refresh: result,
    send: result,
    openSession: result,
    permission: result,
    review: vi.fn(result),
    approve: result,
    cancelTask: result,
    cancelRun: result,
    revokeApproval: result,
    acknowledgeUncertainty: result,
    exportArtifact: async () => true,
    signOut: vi.fn(result),
    lifecycle: result,
    devices: async () => [],
    revokeDevice: async () => {},
    issueInvitation: async () => true,
  } satisfies PairedApplication;
}
describe('paired presentation boundary', () => {
  it('replaces a revoked session worker and enables sending after recovery', async () => {
    const replacement = '00000000-0000-4000-8000-000000000003';
    const value = snapshot();
    const revoked = remoteSnapshotSchema.parse({
      ...value,
      workspace: {
        ...value.workspace,
        workers: [
          { id, revoked: true, phase: 'disconnected' },
          { id: replacement, revoked: false, phase: 'ready' },
        ],
        sessions: [
          { id, bot_id: id, worker_id: id, owner: 'bot', revision: 1, status: 'disconnected' },
        ],
        permissions: [{ bot: id, revision: 2, allow: true }],
        configurations: [{ bot: id, model: 'mistral-fixture', ready: true, context_tokens: 4096 }],
      },
    });
    const recovered = remoteSnapshotSchema.parse({
      ...revoked,
      revision: 11,
      workspace: {
        ...revoked.workspace,
        sessions: [
          {
            id,
            bot_id: id,
            worker_id: replacement,
            owner: 'bot',
            revision: 2,
            status: 'confirmed',
          },
        ],
      },
    });
    const openSession = vi.fn<PairedApplication['openSession']>().mockResolvedValue(recovered);
    const controller = new PairedController({ ...application(revoked), openSession });
    controller.accept(revoked);
    render(<PairedWorkspace controller={controller} local={false} onLocal={() => {}} />);
    const user = userEvent.setup();
    await user.type(screen.getByLabelText('Message'), 'Continue on the replacement worker');
    await user.click(screen.getByText('Execution settings and prerequisites'));
    await user.click(screen.getByLabelText('Permit bounded Linux process proposals'));
    expect(screen.getByRole('button', { name: 'Send' }).hasAttribute('disabled')).toBe(true);
    const choices = screen.getAllByRole('button', { name: /Use worker/ });
    expect(choices).toHaveLength(1);
    await user.click(screen.getByRole('button', { name: /Use worker/ }));
    expect(openSession).toHaveBeenCalledWith(id, replacement);
    expect(screen.getByRole('button', { name: 'Send' }).hasAttribute('disabled')).toBe(false);
    expect(screen.queryByRole('button', { name: /Use worker/ })).toBeNull();
  });
  it('submits the dollar limit selected by the owner in the protocol currency unit', async () => {
    const value = snapshot();
    const ready = remoteSnapshotSchema.parse({
      ...value,
      workspace: {
        ...value.workspace,
        workers: [{ id, revoked: false, phase: 'ready' }],
        sessions: [
          { id, bot_id: id, worker_id: id, owner: 'bot', revision: 1, status: 'confirmed' },
        ],
        permissions: [{ bot: id, revision: 2, allow: true }],
        configurations: [{ bot: id, model: 'mistral-fixture', ready: true, context_tokens: 4096 }],
      },
    });
    const send = vi.fn<PairedApplication['send']>().mockResolvedValue(ready);
    const controller = new PairedController({ ...application(ready), send });
    controller.accept(ready);
    render(<PairedWorkspace controller={controller} local={false} onLocal={() => {}} />);
    const user = userEvent.setup();
    await user.click(screen.getByText('Execution settings and prerequisites'));
    const limit = screen.getByLabelText('Cost limit in US dollars');
    await user.clear(limit);
    await user.type(limit, '0.25');
    await user.type(screen.getByLabelText('Message'), 'Explain this file');
    await user.click(screen.getByRole('button', { name: 'Send' }));
    expect(send.mock.calls[0]?.[0].cost_microusd).toBe(250000);
  });
  it('renders durable turns and linked tasks as inert text', () => {
    const controller = new PairedController(application());
    controller.accept(snapshot());
    const { container } = render(
      <PairedWorkspace controller={controller} local={false} onLocal={() => {}} />,
    );
    expect(screen.getByText('Actual user request')).toBeTruthy();
    expect(screen.getByText('<script>remote content</script>')).toBeTruthy();
    expect(container.querySelector('script')).toBeNull();
    expect(screen.getByLabelText(`Task ${task}`)).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Send' }).hasAttribute('disabled')).toBe(true);
  });
  it('opens the exact-action review as a modal dialog', () => {
    const controller = new PairedController(application());
    const value = snapshot();
    controller.accept(
      remoteSnapshotSchema.parse({
        ...value,
        review: {
          id,
          task,
          action: 'Exact normalized arguments',
          expires_at: 2000,
          permission_revision: 2,
        },
      }),
    );
    render(<PairedWorkspace controller={controller} local={false} onLocal={() => {}} />);
    const dialog = screen.getByRole('dialog', { name: 'Review exact action' });
    expect(dialog.hasAttribute('open')).toBe(true);
    expect(screen.getByText('Exact normalized arguments')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Approve exact action' })).toBeTruthy();
  });
  it('blocks action review while cached state is offline', () => {
    const controller = new PairedController(application());
    controller.accept(snapshot('offline'));
    render(<PairedWorkspace controller={controller} local={false} onLocal={() => {}} />);
    expect(screen.getByRole('button', { name: 'Review action' }).hasAttribute('disabled')).toBe(
      true,
    );
  });
  it('rejects credential fields at the native result boundary', () => {
    expect(
      remoteSnapshotSchema.safeParse({ ...snapshot(), credential: 'must never cross' }).success,
    ).toBe(false);
  });
  it('ignores stale generations and revisions', () => {
    const controller = new PairedController(application());
    controller.accept(snapshot());
    controller.accept({ ...snapshot(), revision: 9 });
    controller.accept({ ...snapshot(), generation: 1, revision: 100 });
    expect(controller.getState().snapshot?.revision).toBe(10);
  });
  it('signs out local administration and the paired workspace together', async () => {
    const local = localApplication();
    const paired = application();
    const localResult = deferred<Snapshot>();
    const pairedResult = deferred<RemoteSnapshot>();
    local.signOut.mockReturnValue(localResult.promise);
    paired.signOut.mockReturnValue(pairedResult.promise);
    const controller = new ApplicationController({ ...local, paired });
    await controller.start();
    controller.paired?.accept(snapshot());
    const pending = controller.signOut();
    expect(controller.getState().view.kind).toBe('signing_out');
    expect(controller.paired?.getState().snapshot).toBeNull();
    expect(local.signOut).toHaveBeenCalledOnce();
    expect(paired.signOut).toHaveBeenCalledOnce();
    const cleared = remoteSnapshotSchema.parse({
      ...snapshot(),
      generation: 3,
      status: 'signed_out',
      workspace: null,
    });
    paired.inspect.mockResolvedValue(cleared);
    localResult.resolve(signedOut());
    pairedResult.resolve(cleared);
    await pending;
    expect(controller.paired?.getState().snapshot?.workspace).toBeNull();
    controller.paired?.dispose();
  });
  it('clears protected state before sign-out and rejects a late operation', async () => {
    const app = application();
    const controller = new PairedController(app);
    controller.accept(snapshot());
    const pending = deferred<RemoteSnapshot>();
    const signOut = deferred<RemoteSnapshot>();
    app.signOut.mockReturnValue(signOut.promise);
    const running = controller.run(() => pending.promise);
    const signingOut = controller.signOut();
    expect(controller.getState().snapshot).toBeNull();
    await act(async () => {
      pending.resolve(snapshot());
      await running;
    });
    expect(controller.getState().snapshot).toBeNull();
    const cleared = remoteSnapshotSchema.parse({
      ...snapshot(),
      generation: 3,
      revision: 11,
      status: 'signed_out',
      workspace: null,
    });
    app.inspect.mockResolvedValue(cleared);
    signOut.resolve(cleared);
    await signingOut;
    expect(controller.getState().snapshot?.workspace).toBeNull();
    controller.dispose();
  });
});

function controlled<T>() {
  let resolve: (value: T) => void = () => {};
  let reject: (reason: unknown) => void = () => {};
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}
describe('saved results authority and request ordering', () => {
  const page = () => ({ generation: 2, manager: id, page: { entries: [], next: null } });
  const query = { bot: null, name: '', after: null, limit: 32 } satisfies Parameters<
    PairedApplication['savedResults']
  >[0];
  it.each([
    { outcome: 'saved', message: 'The verified file was saved.' },
    { outcome: 'cancelled', message: 'Export cancelled.' },
    { outcome: 'failed', message: 'The native export could not be completed.' },
  ])(
    'shows saved-result export feedback inside the dialog when $outcome',
    async ({ outcome, message }) => {
      const results = savedResultsSchema.parse({
        ...page(),
        page: {
          entries: [
            {
              id: task,
              task,
              bot: id,
              output: {
                declaration: {
                  slot: 'result',
                  name: 'Saved file.txt',
                  media_type: 'text_plain',
                  max_bytes: 1024,
                },
                state: {
                  type: 'available',
                  id: task,
                  content: { length: 1, sha256: '0'.repeat(64) },
                },
              },
            },
          ],
          next: null,
        },
      });
      const app = {
        ...application(),
        savedResults: async () => results,
        exportArtifact: async () => {
          if (outcome === 'failed') throw new Error('save_failed');
          return outcome === 'saved';
        },
      };
      const controller = new PairedController(app);
      controller.accept(snapshot());
      render(<PairedWorkspace controller={controller} local={false} onLocal={() => {}} />);
      const user = userEvent.setup();
      await user.click(screen.getByRole('button', { name: 'Saved results' }));
      const dialog = screen.getByRole('dialog', { name: 'Saved results' });
      await user.click(within(dialog).getByRole('button', { name: 'Save verified file' }));
      expect(await within(dialog).findByText(message)).toBeTruthy();
      expect(screen.getAllByText(message)).toHaveLength(1);
      controller.dispose();
    },
  );
  it.each(['success', 'failure'] as const)(
    'ignores a superseded same-generation page %s',
    async (outcome) => {
      const app = application();
      const first = controlled<Awaited<ReturnType<PairedApplication['savedResults']>>>();
      const second = controlled<Awaited<ReturnType<PairedApplication['savedResults']>>>();
      const reads = vi
        .fn<PairedApplication['savedResults']>()
        .mockReturnValueOnce(first.promise)
        .mockReturnValueOnce(second.promise);
      const controller = new PairedController({ ...app, savedResults: reads });
      controller.accept(snapshot());
      const a = controller.library(query);
      const b = controller.library({ ...query, name: 'new' });
      second.resolve(page());
      await b;
      if (outcome === 'failure') first.reject(new Error('late_failure'));
      else first.resolve(page());
      await a;
      expect(controller.getState().library).toMatchObject({
        kind: 'ready',
        query: { name: 'new' },
      });
      expect(controller.getState().issue).toBeNull();
      expect(controller.getState().snapshot?.workspace?.tasks).toHaveLength(1);
    },
  );
  it.each(['offline', 'suspended', 'blocked', 'signed_out'] as const)(
    'discards pages and notices on %s without a generation change',
    async (status) => {
      const pending = controlled<Awaited<ReturnType<PairedApplication['savedResults']>>>();
      const saved = controlled<boolean>();
      const controller = new PairedController({
        ...application(),
        savedResults: () => pending.promise,
      });
      const initial = snapshot();
      controller.accept(initial);
      const read = controller.library(query);
      const exporting = controller.export(() => saved.promise);
      controller.accept({ ...initial, revision: 11, status });
      pending.resolve(page());
      saved.resolve(true);
      await Promise.all([read, exporting]);
      expect(controller.getState().library.kind).toBe('closed');
      expect(controller.getState().source.kind).toBe('idle');
      expect(controller.getState().notice).toBeNull();
      expect(controller.getState().busy).toBe(false);
    },
  );
  it('discards old source success and failure after source selection or filters change', async () => {
    const initial = snapshot();
    const source = initial.workspace?.tasks[0];
    if (!source) throw new Error('Missing test source.');
    const first = controlled<Awaited<ReturnType<PairedApplication['sourceTask']>>>();
    const second = controlled<Awaited<ReturnType<PairedApplication['sourceTask']>>>();
    const reads = vi
      .fn<PairedApplication['sourceTask']>()
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const controller = new PairedController({ ...application(), sourceTask: reads });
    controller.accept(initial);
    const a = controller.sourceTask(source.id);
    const b = controller.sourceTask(source.id);
    second.resolve({ generation: 2, manager: id, task: source });
    await b;
    first.reject(new Error('late_source_failure'));
    await a;
    expect(controller.getState().source.kind).toBe('ready');
    const third = controlled<Awaited<ReturnType<PairedApplication['sourceTask']>>>();
    reads.mockReturnValueOnce(third.promise);
    const c = controller.sourceTask(source.id);
    await controller.library({ ...query, name: 'changed' });
    third.resolve({ generation: 2, manager: id, task: source });
    await c;
    expect(controller.getState().source.kind).toBe('idle');
  });
  it.each(['manager', 'generation', 'dispose'] as const)(
    'invalidates an outstanding read on %s change',
    async (change) => {
      const pending = controlled<Awaited<ReturnType<PairedApplication['savedResults']>>>();
      const controller = new PairedController({
        ...application(),
        savedResults: () => pending.promise,
      });
      const initial = snapshot();
      controller.accept(initial);
      const read = controller.library(query);
      if (change === 'dispose') controller.dispose();
      else
        controller.accept(
          remoteSnapshotSchema.parse({
            ...initial,
            revision: 11,
            generation: change === 'generation' ? 3 : 2,
            workspace: { ...initial.workspace, manager: change === 'manager' ? task : id },
          }),
        );
      pending.reject(new Error('stale_read'));
      await read;
      expect(controller.getState().library.kind).toBe('closed');
      expect(controller.getState().issue).toBeNull();
    },
  );
  it('clears busy state and ignores a save notice after filters change', async () => {
    const saved = controlled<boolean>();
    const controller = new PairedController(application());
    controller.accept(snapshot());
    const saving = controller.export(() => saved.promise);
    await controller.library({ ...query, name: 'changed' });
    saved.resolve(true);
    await saving;
    expect(controller.getState().busy).toBe(false);
    expect(controller.getState().notice).toBeNull();
  });
  it('keeps an unrelated operation busy while the library changes', async () => {
    const pending = controlled<RemoteSnapshot>();
    const controller = new PairedController(application());
    controller.accept(snapshot());
    const operation = controller.run(() => pending.promise);
    await controller.library(query);
    controller.closeLibrary();
    expect(controller.getState().busy).toBe(true);
    pending.resolve(snapshot());
    await operation;
    expect(controller.getState().busy).toBe(false);
  });
});
