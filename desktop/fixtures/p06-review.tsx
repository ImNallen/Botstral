import React from 'react';
import { createRoot } from 'react-dom/client';
import { LoginReviews } from '../src/LoginReviews';
import '../src/styles.css';

const root = document.getElementById('root');
if (!root) throw new Error('Missing fixture root.');
createRoot(root).render(<main style={{ padding: 32, maxWidth: 1000 }}><h1>Protected login</h1><LoginReviews online /></main>);
