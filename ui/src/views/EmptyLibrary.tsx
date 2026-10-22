import { open } from '@tauri-apps/plugin-dialog';
import { FolderPlus } from 'lucide-react';
import { api, isAndroid } from '../api';
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
      {isAndroid && (
        <p className="muted small">
          Android first asks you to allow Cadenza access to all files: it reads music files
          directly, along with their lyrics, cue sheets and folder artwork.
        </p>
      )}
      <button className="pill primary" onClick={() => void addFolder()}>
        <FolderPlus size={16} aria-hidden /> Add Music Folder…
      </button>
    </section>
  );
}

/// A folder chosen by the owner, or null.
export async function pickFolder(title: string): Promise<string | null> {
  const picked = isAndroid
    ? await pickOnAndroid()
    : await open({ directory: true, multiple: false, title });
  return typeof picked === 'string' ? picked : null;
}

export async function addFolder() {
  const picked = await pickFolder('Add Music Folder');
  if (picked) {
    await attempt(api.addRoot(picked));
    useStore.getState().libraryChanged();
  }
}

/// Android: file access first (its settings page), then the system folder picker.
async function pickOnAndroid(): Promise<string | null> {
  const granted =
    (await attempt(api.storageAccess())) || (await attempt(api.requestStorageAccess()));
  if (!granted) {
    useStore.getState().showToast('Cadenza can’t read music folders without access to files.');
    return null;
  }
  return (await attempt(api.pickMusicFolder())) ?? null;
}
