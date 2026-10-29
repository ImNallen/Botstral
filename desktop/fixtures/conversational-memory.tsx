import { createRoot } from 'react-dom/client';
import { PairedWorkspace } from '../src/PairedWorkspace';
import { PairedController } from '../src/paired-application';
import { app, stage } from './memory-replay';
import '../src/styles.css';

const controller = new PairedController(app);
controller.accept(stage('before'));
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
      Real conversation components. Recorded manager and native snapshots. Component replay, not
      packaged Tauri execution.
    </div>
    <PairedWorkspace controller={controller} local={false} onLocal={() => {}} />
  </>,
);
