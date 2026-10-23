import { type RefObject, useCallback, useEffect, useRef, useState } from 'react';
import {
  ChevronDown,
  ListMusic,
  MessageSquareQuote,
  Pause,
  Play,
  SkipBack,
  SkipForward,
} from 'lucide-react';
import { api } from '../api';
import { usePhone } from '../hooks/usePhone';
import { useCurrentTrack } from '../hooks/useCurrentTrack';
import { attempt, useStore } from '../store';
import { Artwork } from './Artwork';
import { LyricsView } from './LyricsView';
import { useModal } from '../hooks/useModal';
import { useLook } from '../hooks/useAppearance';
import { useLens, useLiquid } from '../hooks/useGlass';
import { NO_SWIPE, swipeAxis, swipeDone } from '../hooks/useSwipeBack';
import { Visualizer } from './Visualizer';
import { Scrubber } from './PlayerBar';
import { QueuePanel } from './QueuePanel';

type Rgb = [number, number, number];

/// Calm defaults for artwork without colours (or none at all).
const FALLBACK: Rgb[] = [
  [58, 64, 92],
  [92, 54, 84],
  [40, 70, 80],
  [70, 70, 70],
];

/// Full-screen Now Playing: big artwork over a gradient drawn from its colours, transport, and a
/// lyrics or queue panel.
export function NowPlaying() {
  const open = useStore((s) => s.nowPlaying);
  const setOpen = useStore((s) => s.setNowPlaying);
  const panel = useStore((s) => s.nowPlayingPanel);
  const setPanel = useStore((s) => s.setNowPlayingPanel);
  const player = useStore((s) => s.player);
  const track = useCurrentTrack();
  const [palette, setPalette] = useState<{ hash: string; colors: Rgb[] } | null>(null);
  const hash = track?.artHash ?? null;
  const style = useLook((s) => s.look.layout.nowPlaying);

  // Lyrics-first opens on the lyrics.
  useEffect(() => {
    if (open && style === 'lyrics') setPanel('lyrics');
  }, [open, style, setPanel]);

  useEffect(() => {
    if (!open || !hash) return;
    let live = true;
    api.artworkPalette(hash).then(
      (colors) => live && setPalette({ hash, colors }),
      () => live && setPalette({ hash, colors: [] }),
    );
    return () => {
      live = false;
    };
  }, [open, hash]);

  const dialog = useRef<HTMLDivElement>(null);
  const phone = usePhone();
  const liquid = useLiquid();
  const glass = liquid ? ' glass-button glass-lit' : '';
  const playButton = useRef<HTMLButtonElement>(null);
  useLens(playButton, liquid && open);
  const close = useCallback(() => setOpen(false), [setOpen]);
  useModal(open, dialog, close);
  useSheetGestures(dialog, open && phone, close);

  if (!open) return null;
  // The last song's colours stay until the new song's arrive, then fade across (no flash of
  // the fallback colours in between).
  const found = palette?.colors ?? [];
  const colors = FALLBACK.map((f, i) => found[i] ?? found[0] ?? f);
  const playing = player?.status === 'playing';
  const toggles = (
    <div className="np-panel-toggles">
      <button
        className={`icon-button${glass} ${panel === 'lyrics' ? 'active' : ''}`}
        aria-pressed={panel === 'lyrics'}
        aria-label="Lyrics"
        title="Lyrics"
        onClick={() => setPanel(panel === 'lyrics' ? null : 'lyrics')}
      >
        <MessageSquareQuote size={18} />
      </button>
      <button
        className={`icon-button${glass} ${panel === 'queue' ? 'active' : ''}`}
        aria-pressed={panel === 'queue'}
        aria-label="Queue"
        title="Queue"
        onClick={() => setPanel(panel === 'queue' ? null : 'queue')}
      >
        <ListMusic size={18} />
      </button>
    </div>
  );

  return (
    <div
      ref={dialog}
      className="now-playing-view"
      data-style={style}
      role="dialog"
      aria-modal="true"
      aria-label="Now Playing"
    >
      <div className="np-bg" aria-hidden>
        {colors.map((c, i) => (
          <span
            key={i}
            className={`np-blob b${i}`}
            style={{ backgroundColor: `rgb(${c.join(' ')})` }}
          />
        ))}
      </div>
      {phone && (
        <div className="np-grab" aria-hidden>
          <span />
        </div>
      )}
      <header className="np-top">
        <button
          className={`icon-button${glass}`}
          aria-label="Close Now Playing"
          title="Close (Esc)"
          onClick={() => setOpen(false)}
        >
          <ChevronDown size={22} />
        </button>
        {style !== 'minimal' && !phone && toggles}
      </header>

      <div className={`np-body ${panel && style !== 'minimal' ? 'with-panel' : ''}`}>
        <div className="np-main">
          <div className={`np-art ${playing ? 'playing' : 'paused'}`}>
            <Artwork hash={hash} size={1024} alt={track ? `${track.album} artwork` : ''} />
          </div>
          <div className="np-info">
            <h1>{track?.title ?? 'Not Playing'}</h1>
            <p>{track ? [track.artist, track.album].filter(Boolean).join(' — ') : ''}</p>
          </div>
          {style === 'visualizer' && <Visualizer color="#ffffff" />}
          <Scrubber />
          <div className="np-transport">
            <button
              className="icon-button"
              aria-label="Previous"
              disabled={!track}
              onClick={() => void attempt(api.previous())}
            >
              <SkipBack size={28} fill="currentColor" />
            </button>
            <button
              ref={playButton}
              className={`icon-button np-play${liquid ? ' glass-lit' : ''}`}
              aria-label={playing ? 'Pause' : 'Play'}
              disabled={!track}
              onClick={() => void attempt(api.toggle())}
            >
              {playing ? (
                <Pause size={40} fill="currentColor" />
              ) : (
                <Play size={40} fill="currentColor" />
              )}
            </button>
            <button
              className="icon-button"
              aria-label="Next"
              disabled={!track}
              onClick={() => void attempt(api.next())}
            >
              <SkipForward size={28} fill="currentColor" />
            </button>
          </div>
          {style !== 'minimal' && phone && toggles}
        </div>
        {panel && style !== 'minimal' && (
          <div className="np-panel">
            {panel === 'lyrics' ? <LyricsView trackId={player?.trackId ?? null} /> : <QueuePanel />}
          </div>
        )}
      </div>
    </div>
  );
}

/// Now Playing on a phone follows the finger: dragged down it slides away (closing), swiped right
/// it slides off to where the music was started from (the album or page), and either springs
/// back if let go too soon.
function useSheetGestures(ref: RefObject<HTMLElement | null>, on: boolean, close: () => void) {
  useEffect(() => {
    const sheet = ref.current;
    if (!sheet || !on) return;
    let start: { x: number; y: number; t: number } | null = null;
    let axis: 'x' | 'y' | null = null;
    let moved = 0;
    const down = (e: PointerEvent) => {
      if (e.button !== 0 || (e.target as Element).closest(NO_SWIPE)) return;
      start = { x: e.clientX, y: e.clientY, t: e.timeStamp };
      axis = null;
    };
    const move = (e: PointerEvent) => {
      if (!start) return;
      const dx = e.clientX - start.x;
      const dy = e.clientY - start.y;
      axis ??= swipeAxis(dx, dy);
      // Lyrics and the queue scroll up and down themselves.
      if (axis === 'y' && (e.target as Element).closest('.np-panel')) axis = null;
      if (!axis) return;
      const d = Math.max(0, axis === 'x' ? dx : dy);
      sheet.style.transition = 'none';
      sheet.style.transform =
        axis === 'x' ? `translateX(${d}px) scale(${1 - d / 4000})` : `translateY(${d}px)`;
      sheet.style.borderRadius = `${Math.min(36, d / 4)}px`;
    };
    const up = (e: PointerEvent) => {
      const from = start;
      start = null;
      if (!from || !axis) return;
      moved = e.timeStamp;
      const d = axis === 'x' ? e.clientX - from.x : e.clientY - from.y;
      const size = axis === 'x' ? innerWidth : innerHeight;
      if (swipeDone(d, e.timeStamp - from.t, size)) {
        sheet.style.transition = 'transform 220ms ease-in';
        sheet.style.transform =
          axis === 'x' ? `translateX(${innerWidth}px)` : `translateY(${innerHeight}px)`;
        const toSource = axis === 'x';
        setTimeout(() => {
          close();
          if (toSource) useStore.getState().goToPlaySource();
        }, 200);
      } else {
        sheet.style.transition = 'transform 420ms var(--ease-spring), border-radius 300ms';
        sheet.style.transform = '';
        sheet.style.borderRadius = '';
      }
      axis = null;
    };
    const click = (e: MouseEvent) => {
      if (e.timeStamp - moved < 400) {
        e.stopPropagation();
        e.preventDefault();
      }
    };
    sheet.addEventListener('pointerdown', down);
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
    window.addEventListener('pointercancel', up);
    sheet.addEventListener('click', click, true);
    return () => {
      sheet.removeEventListener('pointerdown', down);
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      window.removeEventListener('pointercancel', up);
      sheet.removeEventListener('click', click, true);
    };
  }, [ref, on, close]);
}
