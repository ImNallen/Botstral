import { createRoot } from 'react-dom/client';
import { z } from 'zod';
import { PairedWorkspace } from '../src/PairedWorkspace';
import { PairedController } from '../src/paired-application';
import { remoteSnapshotSchema, type PairedApplication } from '../src/paired-domain';
import proof from '../../docs/evidence/m06-parent/approval-projections.json';
import '../src/styles.css';

const data = z
  .strictObject({ before: remoteSnapshotSchema, after: remoteSnapshotSchema })
  .parse(proof);
const value =
  new URLSearchParams(location.search).get('stage') === 'before' ? data.before : data.after;
const review = value.review;
if (!review) throw new Error('The native action review is required.');
const id = '00000000-0000-4000-8000-000000000001';
const snapshot = remoteSnapshotSchema.parse({
  ...value,
  workspace: {
    manager: id,
    cursor: 1,
    owner: 'Acceptance fixture',
    vault_unlocked: true,
    bots: [{ id, name: 'Research', created_at: 1 }],
    workers: [],
    sessions: [],
    permissions: [],
    configurations: [],
    runs: [],
    tasks: [
      {
        id: review.task,
        bot: id,
        state: 'awaiting_approval',
        progress: 0,
        action: review.action,
        approval_revoked: false,
        approval_consumed: false,
        outputs: [],
        evidence: '[]',
      },
    ],
  },
});
const unavailable = async (): Promise<never> => {
  throw new Error('This proof cannot grant authority.');
};
const application: PairedApplication = {
  runDetails: unavailable,
  routine: unavailable,
  routineText: unavailable,
  saveRoutine: unavailable,
  routineHistory: unavailable,
  routinePreview: unavailable,
  inspect: async () => snapshot,
  observe: async () => () => {},
  beginPairing: unavailable,
  importPairing: unavailable,
  importTrust: unavailable,
  pairLocal: unavailable,
  confirmPairing: unavailable,
  refresh: unavailable,
  send: unavailable,
  openSession: unavailable,
  permission: unavailable,
  review: unavailable,
  approve: unavailable,
  cancelTask: unavailable,
  cancelRun: unavailable,
  revokeApproval: unavailable,
  acknowledgeUncertainty: unavailable,
  exportArtifact: unavailable,
  savedResults: unavailable,
  sourceTask: unavailable,
  signOut: unavailable,
  lifecycle: unavailable,
  devices: unavailable,
  revokeDevice: unavailable,
  issueInvitation: unavailable,
};
const controller = new PairedController(application);
controller.accept(snapshot);
const root = document.getElementById('root');
if (!root) throw new Error('The proof root is required.');
createRoot(root).render(
  <PairedWorkspace controller={controller} local={false} onLocal={() => {}} />,
);
