import { createRoot } from 'react-dom/client';
import { z } from 'zod';
import { PairedWorkspace } from '../src/PairedWorkspace';
import { PairedController } from '../src/paired-application';
import { remoteSnapshotSchema, type PairedApplication } from '../src/paired-domain';
import native from '../../docs/evidence/p03/native-controls.json';
import '../src/styles.css';

const data = z.record(z.string(), remoteSnapshotSchema).parse(native);
let revision = 0;
const stage = (name: string) => {
  const snapshot = data[name];
  if (!snapshot) throw new Error(`Missing native fixture stage ${name}`);
  return Promise.resolve({ ...snapshot, generation: 1, revision: ++revision });
};
const unavailable = async (): Promise<never> => {
  throw new Error('This component replay cannot perform external work.');
};
const app: PairedApplication = {
  writeMemory: unavailable,
  inspect: () => stage('before'),
  observe: async () => () => {},
  beginPairing: unavailable,
  importPairing: unavailable,
  importTrust: unavailable,
  pairLocal: unavailable,
  confirmPairing: unavailable,
  refresh: () => stage('completed'),
  send: unavailable,
  openSession: unavailable,
  permission: unavailable,
  review: unavailable,
  approve: unavailable,
  cancelTask: unavailable,
  cancelRun: unavailable,
  revokeApproval: unavailable,
  exportArtifact: unavailable,
  acknowledgeUncertainty: unavailable,
  signOut: unavailable,
  lifecycle: unavailable,
  devices: unavailable,
  revokeDevice: unavailable,
  issueInvitation: unavailable,
  runDetails: unavailable,
  routine: (input) => {
    switch (input.action.type) {
      case 'set_budget':
        return stage('budget');
      case 'create':
        return stage('created');
      case 'set_paused':
        return stage(input.action.paused ? 'paused' : 'resumed');
      case 'test':
        return stage('completed');
      case 'duplicate_bot':
        return stage('duplicated');
      case 'delete':
        return stage('deleted');
      case 'acknowledge_inbox':
        return stage('acknowledged');
      case 'delete_bot':
        return stage('deleted_bot');
      default:
        return unavailable();
    }
  },
  saveRoutine: () => stage('edited'),
  routineHistory: () => stage('history'),
  routinePreview: () => stage('preview'),
  routineText: () => stage('help'),
};
const controller = new PairedController(app);
const initial = data.before;
if (!initial) throw new Error('Missing before stage.');
controller.accept({ ...initial, generation: 1, revision: 0 });
const root = document.getElementById('root');
if (!root) throw new Error('Missing root.');
createRoot(root).render(
  <>
    <div
      style={{
        position: 'fixed',
        top: 0,
        right: 0,
        zIndex: 10000,
        padding: '6px 12px',
        background: '#fff1b8',
        color: '#302500',
        fontSize: 12,
      }}
    >
      Actual app components. Recorded native manager responses. Synthetic model and worker. This
      video performs no external effects.
    </div>
    <PairedWorkspace controller={controller} local={false} onLocal={() => {}} />
  </>,
);
