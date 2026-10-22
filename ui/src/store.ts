// App-wide UI state. The core is the source of truth for library and playback; this store only
// mirrors what the UI needs to render and remembers navigation.
import { create } from 'zustand';
import type { EditReport, EditStatus, LoudnessStatus, PlayerState, ScanStatus } from './types';

export type Route =
  | { name: 'home' }
  | { name: 'recent' }
  | { name: 'artists' }
  | { name: 'artist'; artist: string }
  | { name: 'albums' }
  | { name: 'album'; id: number }
  | { name: 'songs' }
  | { name: 'search'; query: string }
  | { name: 'folders' }
  | { name: 'sound' }
  | { name: 'settings' };

export type DockTab = 'queue' | 'lyrics' | 'info' | 'signal' | 'tags';

export interface MenuItem {
  label: string;
  run: () => void;
}

interface Store {
  route: Route;
  back: Route[];
  forward: Route[];
  navigate: (r: Route) => void;
  goBack: () => void;
  goForward: () => void;

  player: PlayerState | null;
  setPlayer: (p: PlayerState) => void;
  setPosition: (trackId: number, positionMs: number) => void;

  scan: ScanStatus;
  setScan: (s: Partial<ScanStatus>) => void;
  /// Bumped when the catalogue changes so views refetch.
  libraryVersion: number;
  libraryChanged: () => void;

  /// The dock's open tab (queue, lyrics, info, signal path), or closed.
  dock: DockTab | null;
  /// Opens the dock on `tab`, or closes it when it already shows that tab.
  toggleDock: (tab: DockTab) => void;
  setDock: (tab: DockTab | null) => void;
  toggleQueue: () => void;

  /// The full-screen Now Playing view, and which side panel it shows.
  nowPlaying: boolean;
  setNowPlaying: (open: boolean) => void;
  nowPlayingPanel: 'lyrics' | 'queue' | null;
  setNowPlayingPanel: (p: 'lyrics' | 'queue' | null) => void;

  loudness: LoudnessStatus | null;
  setLoudness: (s: LoudnessStatus) => void;
  /// Bumped when output or sound settings change elsewhere (the output menu).
  settingsVersion: number;
  settingsChanged: () => void;

  menu: { x: number; y: number; items: MenuItem[] } | null;
  openMenu: (x: number, y: number, items: MenuItem[]) => void;
  closeMenu: () => void;

  toast: { message: string; action?: MenuItem } | null;
  /// A short message at the bottom; `action` adds a button (Undo).
  showToast: (msg: string, action?: MenuItem) => void;

  /// The Song Info editor, for these tracks.
  songInfo: number[] | null;
  openSongInfo: (ids: number[]) => void;
  closeSongInfo: () => void;
  /// Tag writing in progress, and the last finished batch with failures (shown until dismissed).
  edit: EditStatus | null;
  setEdit: (s: EditStatus) => void;
  editFailures: EditReport | null;
  setEditFailures: (r: EditReport | null) => void;
}

export const useStore = create<Store>((set, get) => ({
  route: { name: 'home' },
  back: [],
  forward: [],
  navigate: (r) => {
    const cur = get().route;
    if (JSON.stringify(cur) === JSON.stringify(r)) return;
    set({ route: r, back: [...get().back, cur].slice(-50), forward: [] });
  },
  goBack: () => {
    const back = get().back;
    const prev = back.at(-1);
    if (!prev) return;
    set({ route: prev, back: back.slice(0, -1), forward: [get().route, ...get().forward] });
  },
  goForward: () => {
    const [next, ...rest] = get().forward;
    if (!next) return;
    set({ route: next, forward: rest, back: [...get().back, get().route] });
  },

  player: null,
  setPlayer: (p) => set({ player: p }),
  setPosition: (trackId, positionMs) => {
    const p = get().player;
    if (p && p.trackId === trackId) set({ player: { ...p, positionMs } });
  },

  scan: { running: false, progress: null, lastReport: null },
  setScan: (s) => set({ scan: { ...get().scan, ...s } }),
  libraryVersion: 0,
  libraryChanged: () => set({ libraryVersion: get().libraryVersion + 1 }),

  dock: null,
  toggleDock: (tab) => set({ dock: get().dock === tab ? null : tab }),
  setDock: (dock) => set({ dock }),
  toggleQueue: () => get().toggleDock('queue'),

  nowPlaying: false,
  setNowPlaying: (open) => set({ nowPlaying: open }),
  nowPlayingPanel: 'lyrics',
  setNowPlayingPanel: (p) => set({ nowPlayingPanel: p }),

  loudness: null,
  setLoudness: (s) => set({ loudness: s }),
  settingsVersion: 0,
  settingsChanged: () => set({ settingsVersion: get().settingsVersion + 1 }),

  menu: null,
  openMenu: (x, y, items) => set({ menu: { x, y, items } }),
  closeMenu: () => set({ menu: null }),

  toast: null,
  showToast: (message, action) => {
    const toast = { message, action };
    set({ toast });
    setTimeout(
      () => {
        if (get().toast === toast) set({ toast: null });
      },
      action ? 8000 : 4000,
    );
  },

  songInfo: null,
  openSongInfo: (ids) => set({ songInfo: ids.length ? ids : null, menu: null }),
  closeSongInfo: () => set({ songInfo: null }),
  edit: null,
  setEdit: (edit) => set({ edit }),
  editFailures: null,
  setEditFailures: (editFailures) => set({ editFailures }),
}));

/// Runs a core call; failures surface as a toast instead of vanishing.
export function attempt<T>(p: Promise<T>): Promise<T | undefined> {
  return p.catch((e: unknown) => {
    useStore.getState().showToast(String(e));
    return undefined;
  });
}
