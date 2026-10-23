import { useEffect, useState } from 'react';
import { api } from '../api';
import { useStore } from '../store';
import type { TrackRow } from '../types';

/// The library row of the current track; null when nothing plays. While the next track's row
/// loads, the previous one stays, so the player and artwork don't blink empty between songs.
export function useCurrentTrack(): TrackRow | null {
  const trackId = useStore((s) => s.player?.trackId ?? null);
  const [track, setTrack] = useState<TrackRow | null>(null);
  useEffect(() => {
    if (trackId === null) return;
    let live = true;
    void api.tracks([trackId]).then((rows) => live && setTrack(rows[0] ?? null));
    return () => {
      live = false;
    };
  }, [trackId]);
  return trackId === null ? null : track;
}
