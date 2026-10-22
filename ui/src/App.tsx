import { useEffect } from 'react';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import { api, onApiEvent } from './api';
import { useStore } from './store';
import { Sidebar } from './components/Sidebar';
import { PlayerBar } from './components/PlayerBar';
import { QueuePanel } from './components/QueuePanel';
import { ContextMenu } from './components/ContextMenu';
import { AlbumsView } from './views/AlbumsView';
import { AlbumView } from './views/AlbumView';
import { SongsView } from './views/SongsView';
import { ArtistsView } from './views/ArtistsView';
import { SearchView } from './views/SearchView';
import { FoldersView } from './views/FoldersView';
import { SoundView } from './views/SoundView';

function useCoreEvents() {
  useEffect(() => {
    const s = useStore.getState();
    void api.player().then(s.setPlayer);
    void api.scanStatus().then((scan) => s.setScan(scan));
    void api.loudnessStatus().then(s.setLoudness);
    const unlisten = onApiEvent((e) => {
      const st = useStore.getState();
      switch (e.kind) {
        case 'player':
          if (e.event.type === 'state') {
            const { type: _type, ...state } = e.event;
            st.setPlayer(state);
          } else if (e.event.type === 'position') {
            st.setPosition(e.event.trackId, e.event.positionMs);
          } else if (e.event.type === 'error') {
            st.showToast(`Could not play a track: ${e.event.message}`);
          }
          break;
        case 'scanProgress':
          st.setScan({ running: e.progress.phase !== 'done', progress: e.progress });
          break;
        case 'scanFinished':
          st.setScan({ running: false, lastReport: e.report });
          break;
        case 'libraryChanged':
          st.libraryChanged();
          break;
        case 'loudness':
          st.setLoudness(e.status);
          break;
      }
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, []);
}

/// Space plays/pauses, ⌘/Ctrl+F focuses search, Alt+←/→ navigate (outside text fields).
function useShortcuts() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing =
        e.target instanceof HTMLInputElement ||
        e.target instanceof HTMLSelectElement ||
        e.target instanceof HTMLTextAreaElement;
      if ((e.ctrlKey || e.metaKey) && e.key === 'f') {
        e.preventDefault();
        document.querySelector<HTMLInputElement>('.search-field input')?.focus();
      } else if (!typing && e.key === ' ') {
        e.preventDefault();
        void api.toggle();
      } else if (e.altKey && e.key === 'ArrowLeft') {
        useStore.getState().goBack();
      } else if (e.altKey && e.key === 'ArrowRight') {
        useStore.getState().goForward();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);
}

function Main() {
  const route = useStore((s) => s.route);
  switch (route.name) {
    case 'recent':
      return <AlbumsView recent />;
    case 'albums':
      return <AlbumsView />;
    case 'album':
      return <AlbumView id={route.id} />;
    case 'songs':
      return <SongsView />;
    case 'artists':
      return <ArtistsView />;
    case 'artist':
      return <ArtistsView selected={route.artist} />;
    case 'search':
      return <SearchView query={route.query} />;
    case 'folders':
      return <FoldersView />;
    case 'sound':
      return <SoundView />;
  }
}

export function App() {
  useCoreEvents();
  useShortcuts();
  const queueOpen = useStore((s) => s.queueOpen);
  const back = useStore((s) => s.back.length > 0);
  const forward = useStore((s) => s.forward.length > 0);
  const toast = useStore((s) => s.toast);

  return (
    <div className={`app ${queueOpen ? 'with-queue' : ''}`}>
      <Sidebar />
      <main className="main">
        <div className="toolbar">
          <button
            className="icon-button small"
            aria-label="Back"
            disabled={!back}
            onClick={() => useStore.getState().goBack()}
          >
            <ChevronLeft size={18} />
          </button>
          <button
            className="icon-button small"
            aria-label="Forward"
            disabled={!forward}
            onClick={() => useStore.getState().goForward()}
          >
            <ChevronRight size={18} />
          </button>
        </div>
        <Main />
      </main>
      {queueOpen && <QueuePanel />}
      <PlayerBar />
      <ContextMenu />
      {toast && (
        <div className="toast" role="status">
          {toast}
        </div>
      )}
    </div>
  );
}
