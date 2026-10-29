/// <reference types="node" />
import { readFileSync } from 'node:fs';
import { expect, it } from 'vitest';
import { z } from 'zod';
import { remoteSnapshotSchema } from './paired-domain';

const source = process.env.BOTSTRAL_C02_VIEW_OUTPUT;
it.skipIf(!source)('parses actual-manager native projections at the renderer boundary', () => {
  if (!source) throw new Error('Run desktop/fixtures/paired_manager.py --verify-native.');
  const input: unknown = JSON.parse(readFileSync(source, 'utf8'));
  const views = z.array(remoteSnapshotSchema).parse(input);
  expect(views.some((view) => view.candidate !== null)).toBe(true);
  expect(views.some((view) => view.review !== null)).toBe(true);
  expect(
    views.some((view) => view.workspace?.runs.some((run) => run.turns.some((turn) => turn.text))),
  ).toBe(true);
  expect(
    views.some((view) =>
      view.workspace?.tasks.some((task) =>
        task.outputs.some((output) => output.state.type === 'available'),
      ),
    ),
  ).toBe(true);
  expect(views.some((view) => view.status === 'signed_out' && view.workspace === null)).toBe(true);
});

it('defaults to no sources and strictly validates bounded message previews', () => {
  const value = {
    generation: 1,
    revision: 1,
    status: 'online',
    issue: null,
    pairing: null,
    candidate: null,
    review: null,
    intents: [],
    workspace: {
      manager: '00000000-0000-4000-8000-000000000001',
      cursor: 1,
      owner: 'Owner',
      vault_unlocked: false,
      bots: [],
      workers: [],
      sessions: [],
      permissions: [],
      configurations: [],
      runs: [],
      tasks: [],
    },
  };
  expect(remoteSnapshotSchema.parse(value).workspace?.memory_sources).toEqual([]);
  const preview = { id: value.workspace.manager, bot: value.workspace.manager, text: 'Preference' };
  expect(
    remoteSnapshotSchema.parse({
      ...value,
      workspace: {
        ...value.workspace,
        memory_sources: Array.from({ length: 256 }, () => preview),
      },
    }).workspace?.memory_sources,
  ).toHaveLength(256);
  for (const memory_sources of [
    [{ ...preview, id: 'invalid' }],
    [{ ...preview, text: 'a'.repeat(481) }],
    [{ ...preview, conversation: preview.id }],
    Array.from({ length: 257 }, () => preview),
  ]) {
    expect(
      remoteSnapshotSchema.safeParse({
        ...value,
        workspace: { ...value.workspace, memory_sources },
      }).success,
    ).toBe(false);
  }
});

const initialSource = process.env.BOTSTRAL_INITIAL_MEMORY_VIEWS;
it.skipIf(!initialSource)(
  'parses initial creation snapshots recorded by the real-manager contract',
  () => {
    if (!initialSource) throw new Error('Run paired_manager.py --verify-initial-memory.');
    const views = z
      .record(z.string(), remoteSnapshotSchema)
      .parse(JSON.parse(readFileSync(initialSource, 'utf8')));
    expect(views.absent?.workspace?.documents).toEqual([]);
    expect(views.absent?.workspace?.memory_sources.length).toBeGreaterThan(0);
    expect(views.created?.workspace?.documents[0]?.revision).toBe(1);
    expect(views.restarted?.workspace?.documents).toEqual(views.created?.workspace?.documents);
  },
);
