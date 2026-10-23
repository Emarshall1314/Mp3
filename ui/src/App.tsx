import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { ChevronLeft, ChevronRight, LayoutTemplate } from 'lucide-react';
import { api, onApiEvent, setPlayListener } from './api';
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
import { LibraryView } from './views/LibraryView';
import { MixView } from './views/Rediscover';
import { PhonePlayer } from './components/PhonePlayer';
import { PhoneTabBar, tabOf } from './components/PhoneTabBar';
import { usePhone } from './hooks/usePhone';
import { useLongPress } from './hooks/useLongPress';
import { useGlassLight, useGlassTouch, useLiquid } from './hooks/useGlass';
import { useSwipeBack } from './hooks/useSwipeBack';
import { useAndroidBack } from './hooks/useAndroidBack';

function useCoreEvents() {
  useEffect(() => {
    const s = useStore.getState();
    void api.player().then(s.setPlayer);
    void api.scanStatus().then((scan) => s.setScan(scan));
    void api.loudnessStatus().then(s.setLoudness);
    void api.syncStatus().then(s.setSync);
    // Settings that couldn't be read are said, not silently replaced.
    void api.startupProblems().then((problems) => {
      if (problems.length) s.showToast(`Cadenza started with defaults: ${problems.join('; ')}`);
    });
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
    case 'library':
      return <LibraryView />;
    case 'mix':
      return <MixView id={route.id} />;
  }
}

function Toast() {
  const toast = useStore((s) => s.toast);
  if (!toast) return null;
  return (
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
  );
}

/// The phone: one page at a time, the mini player and the tabs. A page reached from a tab's
/// page has a back button.
function PhoneApp() {
  const route = useStore((s) => s.route);
  const back = useStore((s) => s.back.length > 0);
  const tabs = useLook((s) => s.look.layout.phoneTabs);
  const tab = tabOf(route, tabs);
  const atTabTop =
    route.name === tab ||
    (tab === 'library' && route.name === 'library') ||
    (route.name === 'search' && tab === 'search');
  const liquid = useLiquid();
  const playing = useStore((s) => s.player?.trackId != null);
  useGlassLight(liquid);
  useGlassTouch(liquid);
  const collapsed = useCollapseOnScroll(liquid, route);
  const app = useRef<HTMLDivElement>(null);
  const dock = useRef<HTMLDivElement>(null);
  const main = useRef<HTMLElement>(null);
  useSwipeBack(main, true);
  useAndroidBack();
  // Remember where music is started from, for Now Playing's swipe back to it.
  useEffect(() => setPlayListener((album) => useStore.getState().notePlaySource(album)), []);
  // Pages leave room at the bottom for the dock floating over them.
  useLayoutEffect(() => {
    const el = dock.current;
    if (!el) return;
    const fit = () =>
      app.current?.style.setProperty('--dock-height', `${Math.ceil(el.offsetHeight)}px`);
    fit();
    const observer = new ResizeObserver(fit);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  return (
    <div className="app" data-phone="" ref={app}>
      {liquid && <div className="phone-edge" aria-hidden />}
      <main className="main phone-main" ref={main}>
        {back && !atTabTop && (
          <div className="phone-top">
            <button
              className={`icon-button${liquid ? ' glass-button glass-lit' : ''}`}
              aria-label="Back"
              onClick={() => useStore.getState().goBack()}
            >
              <ChevronLeft size={22} />
            </button>
          </div>
        )}
        <Main />
      </main>
      <div
        className="phone-dock"
        ref={dock}
        data-collapsed={collapsed.value || undefined}
        data-search={(liquid && tabs.includes('search')) || undefined}
        data-player={playing || undefined}
      >
        <PhonePlayer liquid={liquid} />
        <PhoneTabBar liquid={liquid} collapsed={collapsed.value} onExpand={collapsed.expand} />
      </div>
      <NowPlaying />
      <ContextMenu />
      <SongInfo />
      <EditFailures />
      <Toast />
    </div>
  );
}

/// In liquid glass, scrolling a page down shrinks the tab bar (and brings the player alongside
/// it); scrolling up, reaching the top or changing page opens it again, as on an iPhone.
function useCollapseOnScroll(on: boolean, route: unknown) {
  // The page the bar was shrunk on: on any other page it's open.
  const [shrunkOn, setShrunkOn] = useState<unknown>(null);
  const value = on && shrunkOn === route;
  const page = useRef(route);
  const current = useRef(value);
  useLayoutEffect(() => {
    page.current = route;
    current.current = value;
  });
  const change = useCallback((next: boolean) => {
    if (current.current === next) return;
    current.current = next;
    // The dock animates between its two arrangements itself (main.css), so this only flips it.
    setShrunkOn(next ? page.current : null);
  }, []);
  useEffect(() => {
    if (!on) return;
    const last = new WeakMap<EventTarget, number>();
    const onScroll = (e: Event) => {
      const el = e.target;
      if (!(el instanceof HTMLElement) || !el.closest('.phone-main')) return;
      const top = el.scrollTop;
      const before = last.get(el);
      last.set(el, top);
      // Only up-and-down scrolling counts: a shelf scrolled sideways keeps scrollTop 0, which
      // would read as "back at the top".
      if (before === undefined || before === top) return;
      if (top < 24) change(false);
      else if (top - before > 6) change(true);
      else if (before - top > 6) change(false);
    };
    document.addEventListener('scroll', onScroll, true);
    return () => document.removeEventListener('scroll', onScroll, true);
  }, [on, change]);
  return { value, expand: useCallback(() => change(false), [change]) };
}

export function App() {
  useCoreEvents();
  useShortcuts();
  const playing = useCurrentTrack();
  useAppearance(playing?.artHash ?? null);
  useGlobalShortcuts();
  useLongPress();
  const phone = usePhone();
  const back = useStore((s) => s.back.length > 0);
  const forward = useStore((s) => s.forward.length > 0);
  const dockPosition = useLook((s) => s.look.layout.dock.position);

  if (phone) return <PhoneApp />;
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
      <Toast />
    </div>
  );
}
