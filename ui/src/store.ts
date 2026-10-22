// App-wide UI state. The core is the source of truth for library and playback; this store only
// mirrors what the UI needs to render and remembers navigation.
import { create } from 'zustand';
import type { PlayerState, ScanStatus } from './types';

export type Route =
  | { name: 'recent' }
  | { name: 'artists' }
  | { name: 'artist'; artist: string }
  | { name: 'albums' }
  | { name: 'album'; id: number }
  | { name: 'songs' }
  | { name: 'search'; query: string }
  | { name: 'folders' };

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

  queueOpen: boolean;
  toggleQueue: () => void;

  menu: { x: number; y: number; items: MenuItem[] } | null;
  openMenu: (x: number, y: number, items: MenuItem[]) => void;
  closeMenu: () => void;

  toast: string | null;
  showToast: (msg: string) => void;
}

export const useStore = create<Store>((set, get) => ({
  route: { name: 'albums' },
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

  queueOpen: false,
  toggleQueue: () => set({ queueOpen: !get().queueOpen }),

  menu: null,
  openMenu: (x, y, items) => set({ menu: { x, y, items } }),
  closeMenu: () => set({ menu: null }),

  toast: null,
  showToast: (msg) => {
    set({ toast: msg });
    setTimeout(() => {
      if (get().toast === msg) set({ toast: null });
    }, 4000);
  },
}));

/// Runs a core call; failures surface as a toast instead of vanishing.
export function attempt<T>(p: Promise<T>): Promise<T | undefined> {
  return p.catch((e: unknown) => {
    useStore.getState().showToast(String(e));
    return undefined;
  });
}
