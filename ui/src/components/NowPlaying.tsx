import { useCallback, useEffect, useRef, useState } from 'react';
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
  // Where a swipe down from the top began.
  const grab = useRef<number | null>(null);
  const close = useCallback(() => setOpen(false), [setOpen]);
  useModal(open, dialog, close);

  if (!open) return null;
  const found = palette && palette.hash === hash ? palette.colors : [];
  const colors = FALLBACK.map((f, i) => found[i] ?? found[0] ?? f);
  const playing = player?.status === 'playing';

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
          <span key={i} className={`np-blob b${i}`} style={{ background: `rgb(${c.join(' ')})` }} />
        ))}
      </div>
      {phone && (
        <div
          className="np-grab"
          aria-hidden
          onPointerDown={(e) => {
            grab.current = e.clientY;
            e.currentTarget.setPointerCapture(e.pointerId);
          }}
          onPointerMove={(e) => {
            if (grab.current !== null && e.clientY - grab.current > 60) {
              grab.current = null;
              close();
            }
          }}
          onPointerUp={() => {
            grab.current = null;
          }}
        >
          <span />
        </div>
      )}
      <header className="np-top">
        <button
          className="icon-button"
          aria-label="Close Now Playing"
          title="Close (Esc)"
          onClick={() => setOpen(false)}
        >
          <ChevronDown size={22} />
        </button>
        {style !== 'minimal' && (
          <div className="np-panel-toggles">
            <button
              className={`icon-button ${panel === 'lyrics' ? 'active' : ''}`}
              aria-pressed={panel === 'lyrics'}
              aria-label="Lyrics"
              title="Lyrics"
              onClick={() => setPanel(panel === 'lyrics' ? null : 'lyrics')}
            >
              <MessageSquareQuote size={18} />
            </button>
            <button
              className={`icon-button ${panel === 'queue' ? 'active' : ''}`}
              aria-pressed={panel === 'queue'}
              aria-label="Queue"
              title="Queue"
              onClick={() => setPanel(panel === 'queue' ? null : 'queue')}
            >
              <ListMusic size={18} />
            </button>
          </div>
        )}
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
              className="icon-button np-play"
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
