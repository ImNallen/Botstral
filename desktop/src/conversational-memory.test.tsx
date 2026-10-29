import { act, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { PairedController } from './paired-application';
import { PairedWorkspace } from './PairedWorkspace';
import { remoteSnapshotSchema } from './paired-domain';
import { app, stage } from '../fixtures/memory-replay';
import { managerOutcomeSchema } from './memory-outcomes';
import legacyMemoryOutcome from '../fixtures/legacy-memory-outcome.json';

HTMLDialogElement.prototype.showModal = function () {
  this.open = true;
};
HTMLDialogElement.prototype.close = function () {
  this.open = false;
};
function setup(name: string) {
  const snapshot = stage(name);
  const send = vi.fn(app.send);
  const controller = new PairedController({ ...app, send });
  controller.accept(snapshot);
  render(<PairedWorkspace controller={controller} local={false} onLocal={() => {}} />);
  return { controller, send, user: userEvent.setup() };
}

describe('conversational memory from recorded manager outcomes', () => {
  it('accepts a workspace snapshot containing an imported memory result', () => {
    const snapshot = stage('listed');
    const workspace = snapshot.workspace;
    const run = workspace?.runs[0];
    const turn = run?.turns[0];
    if (!workspace || !run || !turn) throw new Error('Missing recorded conversation.');
    const parsed = remoteSnapshotSchema.safeParse({
      ...snapshot,
      workspace: {
        ...workspace,
        runs: [{ ...run, turns: [{ ...turn, manager_outcome: legacyMemoryOutcome }] }],
      },
    });
    expect(parsed.success).toBe(true);
  });
  it('sends a memory request with no session, worker, permission, or worker tool', async () => {
    const { controller, send, user } = setup('before');
    const value = stage('before');
    if (!value.workspace) throw new Error('Missing native workspace.');
    act(() =>
      controller.accept(
        remoteSnapshotSchema.parse({
          ...value,
          workspace: { ...value.workspace, workers: [], sessions: [], permissions: [] },
        }),
      ),
    );
    await user.type(
      screen.getByLabelText('Message'),
      'Remember that I prefer concise answers for this bot.',
    );
    expect(screen.getByRole('button', { name: 'Send' }).hasAttribute('disabled')).toBe(false);
    await user.click(screen.getByRole('button', { name: 'Send' }));
    expect(send).toHaveBeenCalledWith(
      expect.objectContaining({ session: null, process: false, runtime: null }),
    );
    expect(screen.getByText('Saved. Individual memory for this bot.')).toBeTruthy();
    expect(screen.getByText('Use concise answers.')).toBeTruthy();
    expect(screen.queryByLabelText('Memory draft')).toBeNull();
    expect(
      screen.queryByRole('button', { name: /Create memory|Edit memory|Save memory/ }),
    ).toBeNull();
  });
  it('keeps saved text and scope visible after failed follow-up inference and restart', () => {
    const { controller } = setup('updated');
    const notice = screen
      .getAllByRole('article', { name: 'Committed memory change' })
      .find((element) => element.textContent?.includes('Updated.'));
    if (!notice) throw new Error('Missing committed update.');
    expect(within(notice).getByText('Use short, complete sentences.')).toBeTruthy();
    expect(within(notice).getByText('Updated. Individual memory for this bot.')).toBeTruthy();
    expect(screen.getByText('awaiting attention · Provider')).toBeTruthy();
    act(() => controller.accept(stage('restored')));
    expect(screen.getByText('Use short, complete sentences.')).toBeTruthy();
    expect(screen.getByRole('article', { name: 'Memory list result' }).textContent).toContain(
      'Keep morning meetings.',
    );
    expect(screen.getByText('Deleted. Individual memory for this bot.')).toBeTruthy();
    expect(screen.getByRole('article', { name: 'Vault handoff' }).textContent).toContain(
      'not supported yet',
    );
  });
  it('renders no success notice for a proposed or unresolved write', () => {
    const { controller } = setup('before');
    const value = stage('updated');
    if (!value.workspace) throw new Error('Missing native workspace.');
    const workspace = value.workspace;
    act(() =>
      controller.accept(
        remoteSnapshotSchema.parse({
          ...value,
          workspace: {
            ...workspace,
            runs: workspace.runs.map((run) => ({
              ...run,
              phase: 'waiting_for_memory',
              turns: run.turns.map((turn) => ({ ...turn, manager_outcome: null, result: null })),
            })),
          },
        }),
      ),
    );
    expect(screen.queryByRole('article', { name: 'Committed memory change' })).toBeNull();
    expect(screen.queryByText('Use short, complete sentences.')).toBeNull();
  });
  it('strictly parses outcomes and rejects foreign proposal fields', () => {
    expect(
      managerOutcomeSchema.safeParse({ type: 'memory', arguments: { markdown: 'proposed' } })
        .success,
    ).toBe(false);
    expect(
      managerOutcomeSchema.safeParse({
        type: 'vault',
        outcome: { type: 'provider_key_setup', value: 'secret' },
      }).success,
    ).toBe(false);
  });
});
