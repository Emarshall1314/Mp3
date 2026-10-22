// Keyboard shortcuts: the handler and the list shown in Settings come from this one table.
import { api, isAndroid } from './api';
import { attempt, useStore } from './store';

interface Shortcut {
  keys: string;
  action: string;
  /// Matches the key event; `typing` is true inside text fields, where plain keys type.
  match?: (e: KeyboardEvent, typing: boolean) => boolean;
  run?: () => void;
}

const ctrl = (e: KeyboardEvent) => e.ctrlKey || e.metaKey;
const plain = (e: KeyboardEvent) => !e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey;
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

export const SHORTCUTS: Shortcut[] = [
  {
    keys: 'Space',
    action: 'Play or pause',
    // A focused button or checkbox keeps Space for itself.
    match: (e, typing) =>
      !typing &&
      plain(e) &&
      e.key === ' ' &&
      !(e.target instanceof HTMLButtonElement || e.target instanceof HTMLInputElement),
    run: () => void attempt(api.toggle()),
  },
  {
    keys: 'Ctrl+→ / Ctrl+←',
    action: 'Next / previous track',
    match: (e, typing) => !typing && ctrl(e) && (e.key === 'ArrowRight' || e.key === 'ArrowLeft'),
  },
  {
    keys: 'Shift+→ / Shift+←',
    action: 'Skip forward / back 10 seconds',
    match: (e, typing) =>
      !typing && e.shiftKey && !ctrl(e) && (e.key === 'ArrowRight' || e.key === 'ArrowLeft'),
  },
  {
    keys: 'Ctrl+↑ / Ctrl+↓',
    action: 'Volume up / down',
    match: (e, typing) => !typing && ctrl(e) && (e.key === 'ArrowUp' || e.key === 'ArrowDown'),
  },
  {
    keys: 'Ctrl+F',
    action: 'Search',
    match: (e) => ctrl(e) && e.key === 'f',
    run: () => document.querySelector<HTMLInputElement>('.search-field input')?.focus(),
  },
  {
    keys: 'Alt+← / Alt+→',
    action: 'Back / forward',
    match: (e) => e.altKey && (e.key === 'ArrowLeft' || e.key === 'ArrowRight'),
  },
  {
    keys: 'Ctrl+L',
    action: 'Now Playing with lyrics',
    match: (e) => ctrl(e) && e.key === 'l',
    run: () => nowPlayingPanel('lyrics'),
  },
  {
    keys: 'Ctrl+U',
    action: 'Show or hide the queue',
    match: (e) => ctrl(e) && e.key === 'u',
    run: () => useStore.getState().toggleQueue(),
  },
  ...(isAndroid
    ? []
    : [
        {
          keys: 'Ctrl+M',
          action: 'Mini player',
          match: (e: KeyboardEvent) => ctrl(e) && e.key === 'm',
          run: () => void attempt(api.toggleMiniPlayer()),
        },
      ]),
  {
    keys: 'Ctrl+I',
    action: 'Song Info for the playing song',
    match: (e) => ctrl(e) && e.key === 'i',
    run: () => {
      const id = useStore.getState().player?.trackId;
      if (id != null) useStore.getState().openSongInfo([id]);
    },
  },
  {
    keys: 'Ctrl+,',
    action: 'Settings',
    match: (e) => ctrl(e) && e.key === ',',
    run: () => useStore.getState().navigate({ name: 'settings' }),
  },
  // Handled where they apply; listed so they can be found.
  { keys: '↑ / ↓, Enter', action: 'Move through songs; play the selected one' },
  { keys: 'Arrow keys', action: 'Move between albums in a grid' },
  { keys: 'Shift+F10 or Menu', action: 'Actions for the focused song or album (Get Info…)' },
  { keys: 'Esc', action: 'Close a menu or Now Playing' },
];

/// The arrow-key pairs, resolved by direction.
function runArrows(e: KeyboardEvent) {
  const forward = e.key === 'ArrowRight' || e.key === 'ArrowUp';
  if (e.altKey) {
    if (forward) useStore.getState().goForward();
    else useStore.getState().goBack();
  } else if (ctrl(e) && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
    volumeBy(forward ? VOLUME_STEP : -VOLUME_STEP);
  } else if (ctrl(e)) {
    void attempt(forward ? api.next() : api.previous());
  } else if (e.shiftKey) {
    seekBy(forward ? SEEK_MS : -SEEK_MS);
  }
}

export function handleShortcut(e: KeyboardEvent) {
  const t = e.target;
  const typing =
    t instanceof HTMLInputElement
      ? !['checkbox', 'radio', 'range', 'button'].includes(t.type)
      : t instanceof HTMLSelectElement || t instanceof HTMLTextAreaElement;
  const hit = SHORTCUTS.find((s) => s.match?.(e, typing));
  if (!hit) return;
  e.preventDefault();
  if (hit.run) hit.run();
  else runArrows(e);
}
