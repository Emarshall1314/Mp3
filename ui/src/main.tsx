import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '@fontsource-variable/inter';
import './main.css';
import { App } from './App';

const root = document.getElementById('root');
if (!root) throw new Error('missing #root');

// Native-feeling app: no browser context menu except where we provide one.
window.addEventListener('contextmenu', (e) => {
  if (!(e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement))
    e.preventDefault();
});

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
