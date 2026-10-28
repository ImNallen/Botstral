/// <reference types="node" />
import { readFileSync } from 'node:fs';
import { expect, it } from 'vitest';
import { z } from 'zod';
import { remoteSnapshotSchema } from './paired-domain';

it('validates recorded real manager routine projections and rejects added authority', () => {
  const input: unknown = JSON.parse(
    readFileSync('../docs/evidence/p03/native-controls.json', 'utf8'),
  );
  const stages = z.record(z.string(), remoteSnapshotSchema).parse(input);
  const created = stages.created;
  const paused = stages.paused;
  const completed = stages.completed;
  const copy = stages.duplicated;
  expect(created?.workspace?.routines[0]?.name).toBe('Daily briefing');
  expect(paused?.workspace?.routines[0]?.state).toBe('paused');
  expect(completed?.workspace?.inbox[0]?.status.type).toBe('finished');
  expect(copy?.workspace?.routines.some((r) => r.state === 'paused')).toBe(true);
  if (!created?.workspace) throw new Error('Missing actual native fixture.');
  expect(
    remoteSnapshotSchema.safeParse({
      ...created,
      workspace: {
        ...created.workspace,
        routines: created.workspace.routines.map((r) => ({ ...r, system_authority: true })),
      },
    }).success,
  ).toBe(false);
});
