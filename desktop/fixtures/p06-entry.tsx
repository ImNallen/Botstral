import React from 'react';
import { createRoot } from 'react-dom/client';
import { MemoryNotice } from '../src/MemoryNotice';
import '../src/styles.css';

const root = document.getElementById('root');
if (!root) throw new Error('Missing fixture root.');
createRoot(root).render(
  <main style={{ padding: 48, maxWidth: 800 }}>
    <h1>Save a reusable login</h1>
    <p>Can you save a reusable login for me?</p>
    <MemoryNotice
      outcome={{ type: 'vault', outcome: { type: 'login_entry', bot: 'c80d783d-b2c5-41d1-b230-4138248994bc' } }}
      result="Open protected native login entry on your paired desktop. Do not send login secrets in chat. No secret was saved by this handoff."
    />
  </main>,
);
