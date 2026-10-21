import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { About } from './About';
import { buildInfo } from './core';
import '@fontsource-variable/inter';
import './main.css';

const root = document.getElementById('root');
if (!root) throw new Error('missing #root');

createRoot(root).render(
  <StrictMode>
    <About load={buildInfo} />
  </StrictMode>,
);
