import { useEffect } from 'react';
import { ChevronLeft, ChevronRight, LayoutTemplate } from 'lucide-react';
import { api, onApiEvent } from './api';
import { attempt, useStore } from './store';
import { useAppearance, useLook } from './hooks/useAppearance';
import { useCurrentTrack } from './hooks/useCurrentTrack';
import { useGlobalShortcuts } from './hooks/useGlobalShortcuts';
import { presetMenu } from './components/LayoutSettings';
import { handleShortcut } from './shortcuts';
import { Sidebar } from './components/Sidebar';
import { PlayerBar } from './components/PlayerBar';
import { Dock } from './components/Dock';
import { ContextMenu } from './components/ContextMenu';
import { NowPlaying } from './components/NowPlaying';
import { EditFailures, SongInfo } from './components/SongInfo';
import { AlbumsView } from './views/AlbumsView';
import { AlbumView } from './views/AlbumView';
import { SongsView } from './views/SongsView';
import { ArtistsView } from './views/ArtistsView';
import { SearchView } from './views/SearchView';
import { FoldersView } from './views/FoldersView';
import { HomeView } from './views/HomeView';
import { SoundView } from './views/SoundView';
import { SettingsView } from './views/SettingsView';
import { SyncView } from './views/SyncView';
import { TrashView } from './views/TrashView';

function useCoreEvents() {
  useEffect(() => {
    const s = useStore.getState();
    void api.player().then(s.setPlayer);
    void api.scanStatus().then((scan) => s.setScan(scan));
    void api.loudnessStatus().then(s.setLoudness);
    void api.syncStatus().then(s.setSync);
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
        case 'editProgress':
          st.setEdit(e.status);
          break;
        case 'editFinished': {
          const { report } = e;
          st.setEdit({ running: false, batch: report.batch, done: 0, total: 0 });
          if (report.failed.length) st.setEditFailures(report);
          if (report.written > 0) {
            const songs = report.written === 1 ? '1 song' : `${report.written} songs`;
            st.showToast(report.cancelled ? `Stopped after ${songs}` : `Saved ${songs}`, {
              label: 'Undo',
              run: () => void attempt(api.undoEdit(report.batch)),
            });
          }
          break;
        }
        case 'loudness':
          st.setLoudness(e.status);
          break;
        case 'sync':
          st.setSync(e.status);
          break;
      }
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, []);
}

function useShortcuts() {
  useEffect(() => {
    window.addEventListener('keydown', handleShortcut);
    return () => window.removeEventListener('keydown', handleShortcut);
  }, []);
}

function Main() {
  const route = useStore((s) => s.route);
  switch (route.name) {
    case 'home':
      return <HomeView />;
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
    case 'settings':
      return <SettingsView />;
    case 'sync':
      return <SyncView />;
    case 'trash':
      return <TrashView />;
  }
}

export function App() {
  useCoreEvents();
  useShortcuts();
  const playing = useCurrentTrack();
  useAppearance(playing?.artHash ?? null);
  useGlobalShortcuts();
  const back = useStore((s) => s.back.length > 0);
  const forward = useStore((s) => s.forward.length > 0);
  const toast = useStore((s) => s.toast);
  const dockPosition = useLook((s) => s.look.layout.dock.position);

  return (
    <div className="app">
      <Sidebar />
      <div className="workspace" data-dock={dockPosition}>
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
            <span className="toolbar-spacer" />
            <button
              className="icon-button small"
              aria-label="Layouts"
              title="Layouts (Ctrl+Shift+L)"
              onClick={(e) => {
                const r = e.currentTarget.getBoundingClientRect();
                useStore.getState().openMenu(r.left, r.bottom + 4, presetMenu());
              }}
            >
              <LayoutTemplate size={16} />
            </button>
          </div>
          <Main />
        </main>
        <Dock />
      </div>
      <PlayerBar />
      <NowPlaying />
      <ContextMenu />
      <SongInfo />
      <EditFailures />
      {toast && (
        <div className="toast" role="status">
          <span>{toast.message}</span>
          {toast.action && (
            <button
              className="link toast-action"
              onClick={() => {
                useStore.setState({ toast: null });
                toast.action?.run();
              }}
            >
              {toast.action.label}
            </button>
          )}
        </div>
      )}
    </div>
  );
}
