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
