import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { PairedWorkspace } from './PairedWorkspace';
import { PairedController } from './paired-application';
import { pairedApplication } from './paired-native';
import {
  memoryDocumentSchema,
  remoteSnapshotSchema,
  type PairedApplication,
  type RemoteSnapshot,
} from './paired-domain';
import { deferred } from './fixtures.test-support';

const bot = '00000000-0000-4000-8000-000000000001';
const other = '00000000-0000-4000-8000-000000000002';
const document = memoryDocumentSchema.parse({
  bot,
  kind: 'memory',
  revision: 3,
  markdown: 'Remember <script>plain text</script>.',
  sources: [
    { type: 'message', id: bot },
    { type: 'external', reference: 'https://example.test/source' },
    { type: 'task', id: other },
  ],
});
function snapshot(): RemoteSnapshot {
  return remoteSnapshotSchema.parse({
    generation: 2,
    revision: 10,
    status: 'online',
    issue: null,
    pairing: null,
    candidate: null,
    review: null,
    intents: [],
    workspace: {
      manager: bot,
      cursor: 10,
      owner: 'Owner',
      vault_unlocked: false,
      bots: [
        { id: bot, name: 'Research', created_at: 1 },
        { id: other, name: 'Other', created_at: 1 },
      ],
      workers: [],
      sessions: [],
      permissions: [],
      configurations: [],
      runs: [],
      tasks: [],
      documents: [document],
    },
  });
}
function workspace() {
  const value = snapshot().workspace;
  if (!value) throw new Error('Fixture requires workspace');
  return value;
}
function setup(initial = snapshot(), overrides: Partial<PairedApplication> = {}) {
  const writeMemory = vi.fn<PairedApplication['writeMemory']>().mockImplementation(async (input) =>
    remoteSnapshotSchema.parse({
      ...initial,
      revision: initial.revision + 1,
      workspace: {
        ...initial.workspace,
        documents: [
          { ...document, revision: input.expectedRevision + 1, markdown: input.markdown },
        ],
      },
    }),
  );
  const app = {
    ...pairedApplication,
    writeMemory,
    refresh: vi.fn(async () => initial),
    ...overrides,
  };
  const controller = new PairedController(app);
  controller.accept(initial);
  const view = render(<PairedWorkspace controller={controller} local={false} onLocal={() => {}} />);
  return { user: userEvent.setup(), controller, app, ...view };
}
async function open(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByText('Memory', { exact: true }));
}
async function edit(user: ReturnType<typeof userEvent.setup>, text = 'Corrected\nMemory') {
  await open(user);
  await user.click(screen.getByRole('button', { name: 'Edit memory' }));
  await user.clear(screen.getByLabelText('Memory draft'));
  await user.type(screen.getByLabelText('Memory draft'), text);
}
function accept(controller: PairedController, change: Partial<RemoteSnapshot>) {
  act(() =>
    controller.accept({
      ...snapshot(),
      revision: (controller.getState().snapshot?.revision ?? 10) + 1,
      ...change,
    }),
  );
}

describe('current saved Memory', () => {
  it('renders provenance and plain text without execution prerequisites; edit and clear preserve the narrow command', async () => {
    const { user, app, container } = setup();
    await edit(user);
    expect(screen.getByText('Saved revision 3')).toBeTruthy();
    expect(screen.getByText(`Message ${bot}`)).toBeTruthy();
    expect(screen.getByText(`Task ${other}`)).toBeTruthy();
    expect(screen.getByText('External reference: https://example.test/source')).toBeTruthy();
    expect(container.querySelector('script')).toBeNull();
    await user.click(screen.getByRole('button', { name: 'Save memory' }));
    expect(app.writeMemory).toHaveBeenCalledWith({
      bot,
      expectedRevision: 3,
      markdown: 'Corrected\nMemory',
    });
    expect(screen.getByText('Current memory matches this edit')).toBeTruthy();
    await user.click(screen.getByRole('button', { name: 'Clear current memory' }));
    expect(app.writeMemory).toHaveBeenCalledTimes(1);
    expect(screen.getByLabelText<HTMLTextAreaElement>('Memory draft').value).toBe('');
    await user.click(screen.getByRole('button', { name: 'Save memory' }));
    expect(app.writeMemory).toHaveBeenLastCalledWith({ bot, expectedRevision: 4, markdown: '' });
    expect(screen.getByText('Saved Memory is blank.')).toBeTruthy();
    await user.click(screen.getByRole('button', { name: 'Edit memory' }));
    await user.type(screen.getByLabelText('Memory draft'), 'Restored');
    await user.click(screen.getByRole('button', { name: 'Discard draft' }));
    expect(screen.getByText('Saved Memory is blank.')).toBeTruthy();
    expect(app.writeMemory).toHaveBeenCalledTimes(2);
  });
  it('removes a match notice when a later refresh changes the current document', async () => {
    const { user, controller } = setup();
    await edit(user);
    await user.click(screen.getByRole('button', { name: 'Save memory' }));
    expect(screen.getByText('Current memory matches this edit')).toBeTruthy();
    accept(controller, {
      workspace: {
        ...workspace(),
        documents: [{ ...document, revision: 5, markdown: 'Later correction' }],
      },
    });
    expect(screen.queryByText('Current memory matches this edit')).toBeNull();
  });
  it('disables a pending save and retains a draft when a newer snapshot overtakes its late reply', async () => {
    const pending = deferred<RemoteSnapshot>();
    const { user, controller } = setup(snapshot(), { writeMemory: () => pending.promise });
    await edit(user);
    await user.click(screen.getByRole('button', { name: 'Save memory' }));
    expect(screen.getByLabelText('Memory draft').hasAttribute('disabled')).toBe(true);
    expect(screen.getByText(/Saving Memory. Waiting for current state/)).toBeTruthy();
    accept(controller, {
      revision: 20,
      workspace: {
        ...workspace(),
        documents: [{ ...document, revision: 5, markdown: 'Concurrent correction' }],
      },
    });
    await act(async () => {
      pending.resolve(
        remoteSnapshotSchema.parse({
          ...snapshot(),
          revision: 11,
          workspace: {
            ...workspace(),
            documents: [{ ...document, revision: 4, markdown: 'Corrected\nMemory' }],
          },
        }),
      );
      await pending.promise;
    });
    expect(screen.getByText(/Memory changed since this draft began/)).toBeTruthy();
    expect(screen.getByLabelText<HTMLTextAreaElement>('Memory draft').value).toBe(
      'Corrected\nMemory',
    );
    expect(screen.queryByText('Current memory matches this edit')).toBeNull();
  });
  it('does not deliver a pending draft to the bot selected before its callback returns', async () => {
    const pending = deferred<RemoteSnapshot>();
    const { user, controller } = setup(snapshot(), { writeMemory: () => pending.promise });
    await edit(user);
    await user.click(screen.getByRole('button', { name: 'Save memory' }));
    await user.click(screen.getByRole('button', { name: /Other/ }));
    await act(async () => {
      pending.resolve({ ...snapshot(), revision: 11 });
      await pending.promise;
    });
    expect(screen.queryByLabelText('Memory draft')).toBeNull();
    expect(screen.getByText(/No saved Memory/)).toBeTruthy();
    expect(controller.getState().snapshot?.workspace?.bots[1]?.id).toBe(other);
  });
  it('distinguishes absent from saved blank and rejects non-Memory projection or unexpected source fields', async () => {
    const initial = snapshot();
    const { user } = setup(
      remoteSnapshotSchema.parse({
        ...initial,
        workspace: { ...initial.workspace, documents: [] },
      }),
    );
    await open(user);
    expect(screen.getByText(/No saved Memory/)).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Edit memory' })).toBeNull();
    expect(memoryDocumentSchema.safeParse({ ...document, kind: 'instructions' }).success).toBe(
      false,
    );
    expect(
      memoryDocumentSchema.safeParse({
        ...document,
        sources: [{ type: 'external', reference: 'x', secret: 'x' }],
      }).success,
    ).toBe(false);
  });
  it('counts UTF-8 bytes, accepts multiline text and blocks an oversized multibyte draft', async () => {
    const { user, app } = setup();
    await edit(user, 'é');
    await user.clear(screen.getByLabelText('Memory draft'));
    await user.paste('é'.repeat(2049));
    expect(screen.getByText(/4098 \/ 4096 UTF-8 bytes/)).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Save memory' }).hasAttribute('disabled')).toBe(true);
    expect(app.writeMemory).not.toHaveBeenCalled();
  });
  it('retains a draft on refresh and offline recovery without auto-saving, then requires explicit conflict review', async () => {
    const { user, controller, app } = setup();
    await edit(user);
    accept(controller, {});
    expect(screen.getByLabelText<HTMLTextAreaElement>('Memory draft').value).toBe(
      'Corrected\nMemory',
    );
    accept(controller, { status: 'offline' });
    expect(screen.getByRole('button', { name: 'Save memory' }).hasAttribute('disabled')).toBe(true);
    accept(controller, { status: 'online' });
    expect(app.writeMemory).not.toHaveBeenCalled();
    accept(controller, {
      workspace: {
        ...workspace(),
        documents: [{ ...document, revision: 4, markdown: 'New current version' }],
      },
    });
    expect(screen.getByText('New current version')).toBeTruthy();
    expect(screen.getByLabelText<HTMLTextAreaElement>('Memory draft').value).toBe(
      'Corrected\nMemory',
    );
    expect(screen.getByRole('button', { name: 'Save memory' }).hasAttribute('disabled')).toBe(true);
    await user.click(screen.getByRole('button', { name: 'Review draft against current' }));
    await user.click(screen.getByRole('button', { name: 'Save memory' }));
    expect(app.writeMemory).toHaveBeenCalledWith({
      bot,
      expectedRevision: 4,
      markdown: 'Corrected\nMemory',
    });
  });
  it.each(['unchanged', 'later', 'sources'])(
    'does not claim an uncertain %s snapshot succeeded and requires a fresh load after acknowledgement',
    async (variant) => {
      const initial = snapshot();
      const unknown = {
        id: other,
        description: 'Current saved Memory edit',
        identity: other,
        status: 'unknown',
      };
      const result = remoteSnapshotSchema.parse({
        ...initial,
        revision: 11,
        intents: [unknown],
        workspace: {
          ...initial.workspace,
          documents: [
            variant === 'later'
              ? { ...document, revision: 6, markdown: 'Corrected\nMemory' }
              : variant === 'sources'
                ? { ...document, revision: 4, markdown: 'Corrected\nMemory', sources: [] }
                : document,
          ],
        },
      });
      const { user, controller } = setup(initial, {
        writeMemory: async () => result,
        refresh: async () =>
          remoteSnapshotSchema.parse({
            ...result,
            revision: 13,
            intents: result.intents.map((intent) => ({ ...intent, status: 'attention' })),
          }),
      });
      await edit(user);
      await user.click(screen.getByRole('button', { name: 'Save memory' }));
      expect(screen.queryByText('Current memory matches this edit')).toBeNull();
      expect(screen.getByText(/Save outcome is uncertain/)).toBeTruthy();
      expect(
        screen.getByRole('button', { name: 'Load current memory' }).hasAttribute('disabled'),
      ).toBe(true);
      accept(controller, {
        intents: [
          {
            id: remoteSnapshotSchema.shape.intents.element.shape.id.parse(other),
            description: 'Current saved Memory edit',
            identity: other,
            status: 'attention',
          },
        ],
      });
      expect(screen.queryByRole('button', { name: 'Edit memory' })).toBeNull();
      await user.click(screen.getByRole('button', { name: 'Load current memory' }));
      expect(screen.getByText(/Loaded current Memory/)).toBeTruthy();
      expect(screen.getByRole('button', { name: 'Edit memory' })).toBeTruthy();
    },
  );
  it('retains a refused edit without claiming success when controller.run catches an error', async () => {
    const { user } = setup(snapshot(), {
      writeMemory: async () => {
        throw new Error('refused');
      },
    });
    await edit(user);
    await user.click(screen.getByRole('button', { name: 'Save memory' }));
    expect(screen.getByText(/The edit was not confirmed/)).toBeTruthy();
    expect(screen.queryByText('Current memory matches this edit')).toBeNull();
  });
  it('discards drafts on bot, owner, manager and native generation changes with visible disclosure', async () => {
    const { user, controller } = setup();
    await edit(user);
    expect(
      screen.getByText(/Switching bots or owner sessions discards unsent Memory drafts/),
    ).toBeTruthy();
    await user.click(screen.getByRole('button', { name: /Other/ }));
    expect(screen.queryByLabelText('Memory draft')).toBeNull();
    await user.click(screen.getByRole('button', { name: /Research/ }));
    expect(screen.queryByLabelText('Memory draft')).toBeNull();
    for (const change of [
      { generation: 3 },
      { workspace: { ...workspace(), owner: 'New owner' } },
      { workspace: { ...workspace(), manager: other } },
    ]) {
      await user.click(screen.getByRole('button', { name: 'Edit memory' }));
      await user.type(screen.getByLabelText('Memory draft'), 'Unsent');
      accept(controller, { ...change, generation: 3 });
      expect(screen.queryByLabelText('Memory draft')).toBeNull();
    }
  });
  it.each(['signed_out', 'blocked'])(
    'unmounts protected drafts on %s and ignores late save callbacks',
    async (status) => {
      const pending = deferred<RemoteSnapshot>();
      const { user, controller } = setup(snapshot(), { writeMemory: () => pending.promise });
      await edit(user);
      await user.click(screen.getByRole('button', { name: 'Save memory' }));
      accept(controller, {
        generation: 3,
        status: status === 'signed_out' ? 'signed_out' : 'blocked',
        issue: status === 'blocked' ? 'revoked' : null,
        workspace: null,
      });
      await act(async () => {
        pending.resolve({ ...snapshot(), revision: 11 });
        await pending.promise;
      });
      expect(screen.queryByLabelText('Memory draft')).toBeNull();
      expect(controller.getState().snapshot?.workspace).toBeNull();
    },
  );
});
