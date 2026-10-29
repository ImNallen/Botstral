import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { PairedController } from './paired-application';
import { PairedWorkspace } from './PairedWorkspace';
import { remoteSnapshotSchema, type PairedApplication, type RemoteSnapshot } from './paired-domain';
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
    createMemory: result,
    writeMemory: result,
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
    expect(screen.getByRole('button', { name: 'Send' }).hasAttribute('disabled')).toBe(true);
    await user.click(screen.getByText('Execution settings and prerequisites'));
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
