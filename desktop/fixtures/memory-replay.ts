import { z } from 'zod';
import { remoteSnapshotSchema, type PairedApplication } from '../src/paired-domain';
import captured from '../../docs/evidence/conversational-memory/native-conversations.json';

export const snapshots = z.record(z.string(), remoteSnapshotSchema).parse(captured);
let revision = 0;
export const stage = (name: string) => {
  const snapshot = snapshots[name];
  if (!snapshot) throw new Error(`Missing captured native stage ${name}`);
  return { ...snapshot, generation: 1, revision: ++revision };
};
const unavailable = async (): Promise<never> => {
  throw new Error('This replay cannot perform external work.');
};
const requests = new Map([
  ['Remember that I prefer concise answers for this bot.', 'created'],
  ['Remember morning meetings for all bots.', 'shared'],
  ['Update my individual preference to short, complete sentences.', 'updated'],
  ['List our shared memories.', 'listed'],
  ['Forget my individual response preference.', 'deleted'],
  ['Can you save a reusable login for me?', 'vault'],
]);
export const app: PairedApplication = {
  inspect: async () => stage('before'),
  observe: async () => () => {},
  send: async (input) => {
    const name = requests.get(input.text);
    if (!name || input.session !== null || input.process || input.runtime) return unavailable();
    return stage(name);
  },
  refresh: async () => stage('restored'),
  beginPairing: unavailable,
  importPairing: unavailable,
  importTrust: unavailable,
  pairLocal: unavailable,
  confirmPairing: unavailable,
  openSession: unavailable,
  permission: unavailable,
  review: unavailable,
  approve: unavailable,
  cancelTask: unavailable,
  cancelRun: unavailable,
  revokeApproval: unavailable,
  exportArtifact: unavailable,
  savedResults: unavailable,
  sourceTask: unavailable,
  acknowledgeUncertainty: unavailable,
  signOut: unavailable,
  lifecycle: unavailable,
  devices: unavailable,
  revokeDevice: unavailable,
  issueInvitation: unavailable,
  runDetails: unavailable,
  routine: unavailable,
  saveRoutine: unavailable,
  routineHistory: unavailable,
  routinePreview: unavailable,
  routineText: unavailable,
};
