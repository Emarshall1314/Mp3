import { useEffect, useRef, useState } from 'react';
import { useStore } from '../store';

/// The playback position in ms, interpolated between the core's 4 Hz position events so bars and
/// lyrics move smoothly while playing.
export function usePosition(): number {
  const status = useStore((s) => s.player?.status);
  const reported = useStore((s) => s.player?.positionMs ?? 0);
  const durationMs = useStore((s) => s.player?.durationMs ?? 0);
  const anchor = useRef({ pos: 0, at: 0 });
  const [live, setLive] = useState(0);
  useEffect(() => {
    anchor.current = { pos: reported, at: performance.now() };
  }, [reported]);
  useEffect(() => {
    if (status !== 'playing') return;
    let raf = 0;
    const tick = () => {
      setLive(anchor.current.pos + Math.min(500, performance.now() - anchor.current.at));
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [status]);
  return Math.min(status === 'playing' ? Math.max(live, reported) : reported, durationMs);
}
