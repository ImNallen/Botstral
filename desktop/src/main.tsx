import { createRoot } from 'react-dom/client';
import { App } from './App';
import { ApplicationController } from './application';
import { nativeApplication } from './native';
import './styles.css';

const root = document.getElementById('root');
if (!root) throw new Error('Missing application root.');
createRoot(root).render(<App controller={new ApplicationController(nativeApplication)} />);
