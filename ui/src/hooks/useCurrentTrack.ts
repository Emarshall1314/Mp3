import { useEffect, useState } from 'react';
import { api } from '../api';
import { useStore } from '../store';
import type { TrackRow } from '../types';

/// The library row of the current track (null while loading or when nothing plays).
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
  return track && track.id === trackId ? track : null;
}
