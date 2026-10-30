import { createRoot } from 'react-dom/client';
import { z } from 'zod';
import { PairedWorkspace } from '../src/PairedWorkspace';
import { PairedController } from '../src/paired-application';
import { artifactQuerySchema, remoteSnapshotSchema, savedResultsSchema, sourceTaskSchema, type PairedApplication } from '../src/paired-domain';
import native from '../../docs/evidence/p05/native-views.json';
import '../src/styles.css';

const data = z.object({ library_workspace: remoteSnapshotSchema, page: savedResultsSchema, next: savedResultsSchema, source: sourceTaskSchema }).parse(native);
const all = [...data.page.page.entries, ...data.next.page.entries];
const unavailable = async (): Promise<never> => { throw new Error('Unavailable in this component replay.'); };
let revision = data.library_workspace.revision;
const app: PairedApplication = {
  savedResults: async (input) => {
    const query = artifactQuerySchema.parse(input);
    const entries = all.filter((e) => (!query.bot || e.bot === query.bot) && e.output.declaration.name.toLowerCase().includes(query.name.toLowerCase()));
    const start = query.after ? 32 : 0;
    return { generation: data.page.generation, manager: data.page.manager, page: { entries: entries.slice(start, start + 32), next: entries.length > start + 32 ? data.page.page.next : null } };
  },
  sourceTask: async (id) => { if (id !== data.source.task.id) return unavailable(); return data.source; },
  inspect: async () => ({ ...data.library_workspace, revision: ++revision }),
  observe: async () => () => {},
  refresh: async () => ({ ...data.library_workspace, revision: ++revision }),
  exportArtifact: async () => new Promise<boolean>((resolve, reject) => {
    const dialog = document.createElement('dialog');
    dialog.setAttribute('aria-label', 'Replay save chooser');
    dialog.innerHTML = '<p>Replay chooser. Real native file saves are verified separately.</p><button data-save>Save replay file</button><button data-cancel>Cancel replay save</button><button data-fail>Fail replay save</button>';
    document.body.append(dialog);
    const finish = (saved: boolean) => { dialog.close(); dialog.remove(); resolve(saved); };
    dialog.querySelector('[data-save]')?.addEventListener('click', () => finish(true));
    dialog.querySelector('[data-cancel]')?.addEventListener('click', () => finish(false));
    dialog.querySelector('[data-fail]')?.addEventListener('click', () => { dialog.close(); dialog.remove(); reject(new Error('save_failed')); });
    dialog.addEventListener('cancel', (event) => { event.preventDefault(); finish(false); });
    dialog.showModal();
  }),
  beginPairing: unavailable, importPairing: unavailable, importTrust: unavailable, pairLocal: unavailable, confirmPairing: unavailable, send: unavailable, openSession: unavailable, permission: unavailable, review: unavailable, approve: unavailable, cancelTask: unavailable, cancelRun: unavailable, revokeApproval: unavailable, acknowledgeUncertainty: unavailable, signOut: unavailable, lifecycle: unavailable, devices: unavailable, revokeDevice: unavailable, issueInvitation: unavailable, runDetails: unavailable, routine: unavailable, routineText: unavailable, saveRoutine: unavailable, routineHistory: unavailable, routinePreview: unavailable, chooseFile: unavailable, clearFile: unavailable,
};
const controller = new PairedController(app);
controller.accept(data.library_workspace);
const root = document.getElementById('root');
if (!root) throw new Error('Missing root.');
createRoot(root).render(<><div style={{ position: 'fixed', top: 0, right: 0, zIndex: 10000, padding: '6px 12px', background: '#fff1b8', color: '#302500', fontSize: 12 }}>Actual app components. Replay of native manager reads. Synthetic worker. Chooser is simulated. No file is saved by this video.</div><PairedWorkspace controller={controller} local={false} onLocal={() => {}} /></>);
