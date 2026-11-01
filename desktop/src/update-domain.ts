import { z } from 'zod';

const identitySchema = z.strictObject({
  version: z.string(),
  channel: z.string(),
  revision: z.string(),
  build_id: z.string(),
});
const outcomeSchema = z.discriminatedUnion('type', [
  z.strictObject({ type: z.literal('current') }),
  z.strictObject({
    type: z.literal('deferred'),
    busy: z.strictObject({
      state: z.literal('busy'),
      reason: z.enum(['active_task', 'active_process']),
    }),
  }),
  z.strictObject({
    type: z.literal('rejected'),
    reason: z.enum(['signature', 'tamper', 'channel', 'downgrade', 'identity']),
  }),
  z.strictObject({ type: z.literal('applied'), previous: identitySchema, current: identitySchema }),
  z.strictObject({
    type: z.literal('rolled_back'),
    previous: identitySchema,
    failed: identitySchema,
    method: z.enum(['reinstall_package', 'restore_backup']),
  }),
  z.strictObject({ type: z.literal('failed'), detail: z.string() }),
]);
export const updateReportSchema = z.strictObject({
  installed: z.strictObject({
    identity: identitySchema,
    protocols: z.strictObject({ worker: z.number().int(), owner: z.number().int() }),
    link_schema_sha256: z.string(),
    component: z.string().optional(),
  }),
  last: z
    .strictObject({
      started_at: z.number().int(),
      finished_at: z.number().int(),
      trigger: z.enum(['timer', 'now']),
      outcome: outcomeSchema,
    })
    .nullable(),
});
export const updateFactsSchema = z.strictObject({
  restart_version: z.string().nullable(),
  report: updateReportSchema.nullable(),
  available: identitySchema.nullable(),
  protocol_match: z.boolean(),
});
export type UpdateFacts = z.infer<typeof updateFactsSchema>;
export type UpdateReport = z.infer<typeof updateReportSchema>;
export type ReleaseIdentity = z.infer<typeof identitySchema>;
export type DesktopNotice =
  | { type: 'restart_ready'; version: string }
  | { type: 'server_available'; report: UpdateReport; available: ReleaseIdentity }
  | { type: 'vault_locked' }
  | { type: 'protocol_mismatch' }
  | { type: 'update_deferred' };

function versionKey(version: string) {
  const split = version.indexOf('-');
  const base = split < 0 ? version : version.slice(0, split);
  const numbers = base.split('.').map((part) => Number(part));
  if (split < 0) return [...numbers, 1, 0, 0];
  const nightly = version
    .slice(split + 1)
    .split('.')
    .slice(1)
    .map((part) => Number(part));
  return [...numbers, 0, ...nightly];
}

function newer(available: ReleaseIdentity, installed: ReleaseIdentity) {
  if (available.channel !== installed.channel || available.version === installed.version)
    return false;
  const left = versionKey(available.version);
  const right = versionKey(installed.version);
  const length = Math.max(left.length, right.length);
  for (let index = 0; index < length; index += 1) {
    const difference = (left[index] ?? 0) - (right[index] ?? 0);
    if (difference !== 0) return difference > 0;
  }
  return false;
}

export function selectNotices(input: {
  restartVersion: string | null;
  report: UpdateReport | null;
  available: ReleaseIdentity | null;
  protocolMatch: boolean;
  vaultLocked: boolean;
}): DesktopNotice[] {
  const notices: DesktopNotice[] = [];
  if (input.restartVersion) notices.push({ type: 'restart_ready', version: input.restartVersion });
  if (input.report && !input.protocolMatch) notices.push({ type: 'protocol_mismatch' });
  if (input.report && input.available && newer(input.available, input.report.installed.identity)) {
    notices.push({
      type: 'server_available',
      report: input.report,
      available: input.available,
    });
  }
  if (input.report?.last?.outcome.type === 'deferred') notices.push({ type: 'update_deferred' });
  if (
    input.vaultLocked &&
    (input.report?.last?.outcome.type === 'applied' ||
      input.report?.last?.outcome.type === 'rolled_back')
  )
    notices.push({ type: 'vault_locked' });
  return notices;
}

export function managerUpdate(report: UpdateReport | null) {
  if (!report) return null;
  const outcome = report.last?.outcome.type;
  const result =
    outcome === 'applied'
      ? 'Updated'
      : outcome === 'rolled_back'
        ? 'Rolled back'
        : outcome === 'deferred'
          ? 'Deferred'
          : outcome === 'rejected'
            ? 'Rejected'
            : outcome === 'failed'
              ? 'Failed'
              : outcome === 'current'
                ? 'Already current'
                : 'No update yet';
  return { version: report.installed.identity.version, result };
}

export const updateCopy = {
  restart: 'Restart to update',
  server: 'Server update available',
  apply: 'Update now',
  deferred: 'The update was deferred because the host is busy. It will retry in the next window.',
  vault: 'The vault is locked. The owner must unlock it to resume.',
  protocol: 'This manager does not support the desktop workspace protocol. Upgrade the manager.',
};
