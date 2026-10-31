import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { z } from 'zod';
import {
  remoteSnapshotSchema,
  deviceSchema,
  savedResultsSchema,
  sourceTaskSchema,
  artifactQuerySchema,
  type PairedApplication,
} from './paired-domain';
async function call(command: string, args?: Record<string, unknown>): Promise<unknown> {
  try {
    return await invoke<unknown>(command, args);
  } catch (error: unknown) {
    throw new Error(typeof error === 'string' ? error : 'native_unavailable');
  }
}
async function snapshot(command: string, args?: Record<string, unknown>) {
  return remoteSnapshotSchema.parse(await call(command, args));
}
export const pairedApplication: PairedApplication = {
  savedResults: async (query) =>
    savedResultsSchema.parse(
      await call('saved_results', { query: artifactQuerySchema.parse(query) }),
    ),
  sourceTask: async (id) => sourceTaskSchema.parse(await call('source_task', { id })),
  runDetails: (id) => snapshot('run_details', { id }),
  routineText: (bot, text) => snapshot('routine_text', { bot, text }),
  routine: (input) => snapshot('routine_command', { input }),
  saveRoutine: (id, expectedRevision, definitionJson) =>
    snapshot('save_routine', { id, expectedRevision, definitionJson }),
  routineHistory: (id, before) => snapshot('routine_history', { id, before }),
  routinePreview: (trigger, after) => snapshot('routine_preview', { trigger, after }),
  inspect: () => snapshot('remote_inspect'),
  observe: (receive) =>
    listen<unknown>('workspace-changed', (event) => {
      const result = remoteSnapshotSchema.safeParse(event.payload);
      if (result.success) receive(result.data);
    }),
  beginPairing: (label) => snapshot('begin_pairing', { label }),
  importPairing: (code) => snapshot('import_pairing', { code }),
  importTrust: () => snapshot('import_trust'),
  pairLocal: () => snapshot('pair_local'),
  confirmPairing: (candidate) => snapshot('confirm_pairing', { candidate }),
  refresh: () => snapshot('remote_refresh'),
  send: (input) => snapshot('send_run', { input }),
  chooseFile: async () =>
    z
      .strictObject({
        name: z.string(),
        byte_length: z.number().int().nonnegative(),
      })
      .nullable()
      .parse(await call('choose_run_file')),
  clearFile: async () => {
    await call('clear_run_file');
  },
  openSession: (bot, worker) => snapshot('open_session', { bot, worker }),
  permission: (bot, expected, allow) => snapshot('set_permission', { bot, expected, allow }),
  review: (task) => snapshot('review_approval', { task }),
  approve: (review) => snapshot('approve_action', { review }),
  cancelTask: (task) => snapshot('cancel_task', { task }),
  cancelRun: (run) => snapshot('cancel_run', { run }),
  revokeApproval: (task) => snapshot('revoke_approval', { task }),
  acknowledgeUncertainty: (intent) => snapshot('acknowledge_uncertainty', { intent }),
  signOut: () => snapshot('remote_sign_out'),
  lifecycle: (suspended) => snapshot('remote_lifecycle', { suspended }),
  exportArtifact: async (task, artifact) =>
    z.boolean().parse(await call('export_artifact', { task, artifact })),
  devices: async () => z.array(deviceSchema).parse(await call('list_devices')),
  revokeDevice: async (device) => {
    await call('revoke_device', { device });
  },
  issueInvitation: async (label) =>
    z.boolean().parse(await call('issue_invitation', { label })),
};
