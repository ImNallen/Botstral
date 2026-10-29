import { createRoot } from 'react-dom/client';
import { z } from 'zod';
import { PairedWorkspace } from '../src/PairedWorkspace';
import { PairedController } from '../src/paired-application';
import { remoteSnapshotSchema, type PairedApplication } from '../src/paired-domain';
import native from '../../docs/evidence/p01-desktop-memory/native-controls.json';
import initialNative from '../../docs/evidence/p01-initial-memory/native-controls.json';
import '../src/styles.css';

const initialProof = new URLSearchParams(location.search).has('initial');
const data = z
  .record(z.string(), remoteSnapshotSchema)
  .parse(initialProof ? initialNative : native);
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
  createMemory: ({ bot, sourceMessage, markdown }) => {
    const document = data.created?.workspace?.documents[0];
    if (
      !initialProof ||
      document?.bot !== bot ||
      document.markdown !== markdown ||
      document.sources.length !== 1 ||
      document.sources[0]?.type !== 'message' ||
      document.sources[0].id !== sourceMessage
    )
      return unavailable();
    return stage('created');
  },
  writeMemory: ({ expectedRevision }) => {
    switch (expectedRevision) {
      case 1:
        return stage('edited');
      case 2:
        return stage('cleared');
      case 4:
        return stage('reviewed');
      default:
        return unavailable();
    }
  },
  inspect: () => stage(initialProof ? 'absent' : 'before'),
  observe: async () => () => {},
  beginPairing: unavailable,
  importPairing: unavailable,
  importTrust: unavailable,
  pairLocal: unavailable,
  confirmPairing: unavailable,
  refresh: () => stage(initialProof ? 'settled' : 'restored'),
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
  routine: unavailable,
  saveRoutine: unavailable,
  routineHistory: unavailable,
  routinePreview: unavailable,
  routineText: unavailable,
};
const controller = new PairedController(app);
const initial = data[initialProof ? 'absent' : 'before'];
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
      Actual Memory component. Recorded native manager snapshots. This replay performs no external
      effects.
    </div>
    <PairedWorkspace controller={controller} local={false} onLocal={() => {}} />
  </>,
);
