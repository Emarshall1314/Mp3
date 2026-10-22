// Keyboard shortcuts: actions with default keys, which the owner can remap in Settings
// (stored in the look's `shortcuts`). The handler and the Settings list come from this table.
import { api, isAndroid } from './api';
import { attempt, useStore } from './store';
import { cyclePreset } from './components/LayoutSettings';
import { turnOffCustomCss, useLook } from './hooks/useAppearance';

export interface ShortcutAction {
  id: string;
  action: string;
  /// The default key combination ("Ctrl+Shift+L"), or '' for none.
  keys: string;
  run: () => void;
}

const SEEK_MS = 10_000;
const VOLUME_STEP = 0.05;

function seekBy(ms: number) {
  const p = useStore.getState().player;
  if (p?.trackId != null) void attempt(api.seek(Math.max(0, p.positionMs + ms)));
}

function volumeBy(step: number) {
  const p = useStore.getState().player;
  if (p) void attempt(api.setVolume(Math.min(1, Math.max(0, p.volume + step))));
}

function nowPlayingPanel(panel: 'lyrics' | 'queue') {
  const s = useStore.getState();
  if (s.nowPlaying && s.nowPlayingPanel === panel) s.setNowPlaying(false);
  else {
    s.setNowPlayingPanel(panel);
    s.setNowPlaying(true);
  }
}

export const ACTIONS: ShortcutAction[] = [
  {
    id: 'playPause',
    action: 'Play or pause',
    keys: 'Space',
    run: () => void attempt(api.toggle()),
  },
  {
    id: 'next',
    action: 'Next track',
    keys: 'Ctrl+ArrowRight',
    run: () => void attempt(api.next()),
  },
  {
    id: 'previous',
    action: 'Previous track',
    keys: 'Ctrl+ArrowLeft',
    run: () => void attempt(api.previous()),
  },
  {
    id: 'seekForward',
    action: 'Skip forward 10 seconds',
    keys: 'Shift+ArrowRight',
    run: () => seekBy(SEEK_MS),
  },
  {
    id: 'seekBack',
    action: 'Skip back 10 seconds',
    keys: 'Shift+ArrowLeft',
    run: () => seekBy(-SEEK_MS),
  },
  { id: 'volumeUp', action: 'Volume up', keys: 'Ctrl+ArrowUp', run: () => volumeBy(VOLUME_STEP) },
  {
    id: 'volumeDown',
    action: 'Volume down',
    keys: 'Ctrl+ArrowDown',
    run: () => volumeBy(-VOLUME_STEP),
  },
  {
    id: 'shuffle',
    action: 'Shuffle on or off',
    keys: '',
    run: () => void attempt(api.setShuffle(!useStore.getState().player?.shuffle)),
  },
  {
    id: 'repeat',
    action: 'Repeat: off, all, one',
    keys: '',
    run: () => {
      const r = useStore.getState().player?.repeat ?? 'off';
      void attempt(api.setRepeat(r === 'off' ? 'all' : r === 'all' ? 'one' : 'off'));
    },
  },
  {
    id: 'search',
    action: 'Search',
    keys: 'Ctrl+F',
    run: () => {
      if (useLook.getState().look.layout.sidebarCollapsed)
        document.querySelector<HTMLElement>('.search-field')?.click();
      else document.querySelector<HTMLInputElement>('.search-field input')?.focus();
    },
  },
  { id: 'back', action: 'Back', keys: 'Alt+ArrowLeft', run: () => useStore.getState().goBack() },
  {
    id: 'forward',
    action: 'Forward',
    keys: 'Alt+ArrowRight',
    run: () => useStore.getState().goForward(),
  },
  {
    id: 'nowPlaying',
    action: 'Now Playing',
    keys: '',
    run: () => useStore.getState().setNowPlaying(!useStore.getState().nowPlaying),
  },
  {
    id: 'lyrics',
    action: 'Now Playing with lyrics',
    keys: 'Ctrl+L',
    run: () => nowPlayingPanel('lyrics'),
  },
  {
    id: 'queue',
    action: 'Show or hide the queue',
    keys: 'Ctrl+U',
    run: () => useStore.getState().toggleQueue(),
  },
  {
    id: 'infoPanel',
    action: 'Song info panel',
    keys: '',
    run: () => useStore.getState().toggleDock('info'),
  },
  {
    id: 'signalPanel',
    action: 'Signal path panel',
    keys: '',
    run: () => useStore.getState().toggleDock('signal'),
  },
  ...(isAndroid
    ? []
    : [
        {
          id: 'miniPlayer',
          action: 'Mini player',
          keys: 'Ctrl+M',
          run: () => void attempt(api.toggleMiniPlayer()),
        },
      ]),
  {
    id: 'songInfo',
    action: 'Song Info for the playing song',
    keys: 'Ctrl+I',
    run: () => {
      const id = useStore.getState().player?.trackId;
      if (id != null) useStore.getState().openSongInfo([id]);
    },
  },
  {
    id: 'settings',
    action: 'Settings',
    keys: 'Ctrl+,',
    run: () => useStore.getState().navigate({ name: 'settings' }),
  },
  { id: 'nextLayout', action: 'Next layout preset', keys: 'Ctrl+Shift+L', run: cyclePreset },
];

/// Keys handled where they apply; listed so they can be found. Not remappable.
export const FIXED_KEYS = [
  { keys: '↑ / ↓, Enter', action: 'Move through songs; play the selected one' },
  { keys: 'Arrow keys', action: 'Move between albums in a grid' },
  { keys: 'Shift+F10 or Menu', action: 'Actions for the focused song or album (Get Info…)' },
  { keys: 'Esc', action: 'Close a menu or Now Playing' },
  ...(isAndroid
    ? []
    : [{ keys: 'Ctrl+Alt+Shift+S', action: 'Turn custom CSS off until Cadenza restarts' }]),
];

const MODIFIER_KEYS = ['Control', 'Shift', 'Alt', 'Meta', 'AltGraph', 'CapsLock'];

/// The combination a key event makes, as stored ("Ctrl+Shift+L"); null for a lone modifier.
export function comboOf(e: {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}): string | null {
  if (MODIFIER_KEYS.includes(e.key)) return null;
  const key = e.key === ' ' ? 'Space' : e.key.length === 1 ? e.key.toUpperCase() : e.key;
  return [
    e.ctrlKey || e.metaKey ? 'Ctrl' : '',
    e.altKey ? 'Alt' : '',
    e.shiftKey ? 'Shift' : '',
    key,
  ]
    .filter(Boolean)
    .join('+');
}

/// A combination as people read it ("Ctrl+→").
export function showCombo(combo: string): string {
  if (!combo) return '—';
  return combo
    .replace('ArrowRight', '→')
    .replace('ArrowLeft', '←')
    .replace('ArrowUp', '↑')
    .replace('ArrowDown', '↓');
}

/// Each action's keys with the owner's changes ('' means none).
export function bindings(overrides: Record<string, string>): Record<string, string> {
  return Object.fromEntries(ACTIONS.map((a) => [a.id, overrides[a.id] ?? a.keys]));
}

export function handleShortcut(e: KeyboardEvent) {
  const combo = comboOf(e);
  if (!combo) return;
  // The way out of custom CSS that hides everything; can't be remapped.
  if (combo === 'Ctrl+Alt+Shift+S' && !isAndroid) {
    e.preventDefault();
    turnOffCustomCss();
    useStore.getState().showToast('Custom CSS is off until Cadenza restarts.');
    return;
  }
  const keys = bindings(useLook.getState().look.shortcuts);
  const hit = ACTIONS.find((a) => keys[a.id] === combo);
  if (!hit) return;
  // Without Ctrl or Alt, keys belong to whatever control has focus: typing in a field, Space
  // on a button, arrows in a slider.
  if (!/^(Ctrl|Alt)\+/.test(combo)) {
    const t = e.target;
    if (
      t instanceof HTMLInputElement ||
      t instanceof HTMLTextAreaElement ||
      t instanceof HTMLSelectElement ||
      t instanceof HTMLButtonElement ||
      (t instanceof HTMLElement && t.isContentEditable)
    )
      return;
  }
  e.preventDefault();
  hit.run();
}
