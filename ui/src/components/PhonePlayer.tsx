import { useRef } from 'react';
import { Pause, Play, SkipForward } from 'lucide-react';
import { api } from '../api';
import { attempt, useStore } from '../store';
import { useCurrentTrack } from '../hooks/useCurrentTrack';
import { useLook } from '../hooks/useAppearance';
import { usePosition } from '../hooks/usePosition';
import { useLens } from '../hooks/useGlass';
import { Artwork } from './Artwork';

/// The phone's player above the tabs (beside them while the tab bar is collapsed). Tap it, or
/// swipe it up, for Now Playing.
export function PhonePlayer({ liquid }: { liquid: boolean }) {
  const track = useCurrentTrack();
  const player = useStore((s) => s.player);
  const style = useLook((s) => s.look.layout.miniPlayer);
  const position = usePosition();
  const from = useRef<number | null>(null);
  const self = useRef<HTMLDivElement>(null);
  useLens(self, liquid && !!track && !!player);
  if (!track || !player) return null;
  const playing = player.status === 'playing';
  const open = () => useStore.getState().setNowPlaying(true);
  const progress = player.durationMs ? Math.min(1, position / player.durationMs) : 0;
  return (
    <div
      ref={self}
      className={`phone-player${liquid ? ' glass-lit' : ''}`}
      data-style={style}
      role="button"
      tabIndex={0}
      aria-label={`Now Playing: ${track.title}. Open`}
      onClick={open}
      onKeyDown={(e) => (e.key === 'Enter' || e.key === ' ') && open()}
      onPointerDown={(e) => {
        from.current = e.clientY;
      }}
      onPointerMove={(e) => {
        // A swipe up opens Now Playing.
        if (from.current !== null && from.current - e.clientY > 40) {
          from.current = null;
          open();
        }
      }}
      onPointerUp={() => {
        from.current = null;
      }}
    >
      <Artwork hash={track.artHash} size={style === 'minimal' ? 40 : 48} alt="" />
      {style !== 'minimal' && (
        <div className="phone-player-text">
          <span className="title">{track.title}</span>
          <span className="subtitle">{track.artist}</span>
        </div>
      )}
      <button
        className="icon-button"
        aria-label={playing ? 'Pause' : 'Play'}
        onClick={(e) => {
          e.stopPropagation();
          void attempt(api.toggle());
        }}
      >
        {playing ? <Pause size={22} /> : <Play size={22} />}
      </button>
      {style !== 'minimal' && (
        <button
          className="icon-button"
          aria-label="Next"
          onClick={(e) => {
            e.stopPropagation();
            void attempt(api.next());
          }}
        >
          <SkipForward size={20} />
        </button>
      )}
      <span className="phone-player-progress" style={{ width: `${progress * 100}%` }} aria-hidden />
    </div>
  );
}
