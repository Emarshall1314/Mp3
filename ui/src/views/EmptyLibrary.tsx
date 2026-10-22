import { open } from '@tauri-apps/plugin-dialog';
import { FolderPlus } from 'lucide-react';
import { api } from '../api';
import { attempt, useStore } from '../store';

/// First run (or every folder removed): say what to do, with the one button that does it.
export function EmptyLibrary() {
  const scan = useStore((s) => s.scan);
  if (scan.running) {
    return (
      <section className="view empty-state">
        <h1>Building your library…</h1>
        <p>
          {scan.progress
            ? `${scan.progress.read.toLocaleString()} of ${scan.progress.toRead.toLocaleString()} files read`
            : 'Looking for music'}
        </p>
      </section>
    );
  }
  return (
    <section className="view empty-state">
      <h1>Add your music</h1>
      <p>
        Choose the folder where your music lives. Cadenza reads it, never changes it, and keeps up
        with new files.
      </p>
      <button className="pill primary" onClick={() => void addFolder()}>
        <FolderPlus size={16} aria-hidden /> Add Music Folder…
      </button>
    </section>
  );
}

export async function addFolder() {
  const picked = await open({ directory: true, multiple: false, title: 'Add Music Folder' });
  if (typeof picked === 'string') {
    await attempt(api.addRoot(picked));
    useStore.getState().libraryChanged();
  }
}
