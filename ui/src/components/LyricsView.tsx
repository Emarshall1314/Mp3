import { useEffect, useRef, useState } from 'react';
import { api } from '../api';
import { usePosition } from '../hooks/usePosition';
import type { Lyrics } from '../types';

/// Lyrics for the playing track. Synced lyrics follow playback, the current line centred; click a
/// line to jump there.
export function LyricsView({ trackId }: { trackId: number | null }) {
  const [state, setState] = useState<{ id: number; lyrics: Lyrics | null } | null>(null);
  const position = usePosition();
  const list = useRef<HTMLOListElement>(null);

  useEffect(() => {
    if (trackId === null) return;
    let live = true;
    api.lyrics(trackId).then(
      (lyrics) => live && setState({ id: trackId, lyrics }),
      () => live && setState({ id: trackId, lyrics: null }),
    );
    return () => {
      live = false;
    };
  }, [trackId]);

  const lyrics = state && state.id === trackId ? state.lyrics : undefined;
  const lines = lyrics?.lines ?? [];
  let current = -1;
  if (lyrics?.synced) {
    for (let i = 0; i < lines.length; i++) {
      if ((lines[i]?.timeMs ?? 0) <= position + 150) current = i;
      else break;
    }
  }

  useEffect(() => {
    if (current < 0) return;
    const el = list.current?.children[current] as HTMLElement | undefined;
    el?.scrollIntoView({ block: 'center', behavior: 'smooth' });
  }, [current]);

  if (lyrics === undefined) return <div className="lyrics" aria-busy="true" />;
  if (lyrics === null)
    return (
      <div className="lyrics empty">
        <p>No lyrics for this song.</p>
        <p className="small">
          Cadenza reads lyrics embedded in the file, or a .lrc file with the same name next to it.
        </p>
      </div>
    );
  return (
    <div className={`lyrics ${lyrics.synced ? 'synced' : ''}`}>
      <ol ref={list}>
        {lines.map((l, i) => (
          <li
            key={i}
            className={i === current ? 'current' : i < current ? 'past' : ''}
            aria-current={i === current ? 'true' : undefined}
          >
            {lyrics.synced && l.timeMs !== null ? (
              <button onClick={() => void api.seek(l.timeMs ?? 0)}>{l.text || '♪'}</button>
            ) : (
              l.text || ' '
            )}
          </li>
        ))}
      </ol>
    </div>
  );
}
