import { useEffect, useState } from 'react';
import {
  ListMusic,
  PictureInPicture2,
  Pause,
  Play,
  Repeat,
  Repeat1,
  Shuffle,
  SkipBack,
  SkipForward,
  Volume1,
  Volume2,
  VolumeX,
} from 'lucide-react';
import { api, isAndroid } from '../api';
import { duration, trackSpec } from '../format';
import { attempt, useStore } from '../store';
import type { TrackRow } from '../types';
import { Artwork } from './Artwork';
import { useLook } from '../hooks/useAppearance';
import type { PlayerControl } from '../theme/layout';
import { usePosition } from '../hooks/usePosition';
import { OutputPicker } from './OutputPicker';
import { SignalPath } from './SignalPath';

/// Persistent bar: artwork, title/artist, transport, scrubber, volume, queue toggle.
const PLAYER_ART = { compact: 40, standard: 48, large: 64 } as const;

export function PlayerBar() {
  const player = useStore((s) => s.player);
  const navigate = useStore((s) => s.navigate);
  const toggleQueue = useStore((s) => s.toggleQueue);
  const queueOpen = useStore((s) => s.queueOpen);
  const layout = useLook((s) => s.look.layout);
  const shows = (c: PlayerControl) => layout.playerControls.includes(c);
  const [track, setTrack] = useState<TrackRow | null>(null);

  const trackId = player?.trackId ?? null;
  useEffect(() => {
    if (trackId === null) return;
    let live = true;
    void api.tracks([trackId]).then((rows) => {
      if (live) setTrack(rows[0] ?? null);
    });
    return () => {
      live = false;
    };
  }, [trackId]);

  // Only show the fetched row while it is still the current track.
  const shown = track && track.id === trackId ? track : null;
  const playing = player?.status === 'playing';
  const hasTrack = player !== null && player.index !== null && player.queueLength > 0;

  return (
    <footer className="player-bar" aria-label="Player">
      <div className="now-playing">
        {shown ? (
          <>
            <button
              className="np-open"
              aria-label="Open Now Playing"
              title="Now Playing"
              onClick={() => useStore.getState().setNowPlaying(true)}
            >
              <Artwork hash={shown.artHash} size={PLAYER_ART[layout.playerSize]} alt="" />
            </button>
            <div className="now-playing-text">
              <button
                className="link title"
                onClick={() =>
                  shown.albumId !== null && navigate({ name: 'album', id: shown.albumId })
                }
              >
                {shown.title}
              </button>
              <button
                className="link subtitle"
                onClick={() => navigate({ name: 'artist', artist: shown.albumartist })}
              >
                {shown.artist}
              </button>
            </div>
            <SignalPath quality={shown.quality} fallback={trackSpec(shown)} />
          </>
        ) : (
          <div className="now-playing-empty">Not playing</div>
        )}
      </div>

      <div className="transport">
        <div className="transport-buttons">
          {shows('shuffle') && (
            <button
              className={`icon-button small ${player?.shuffle ? 'active' : ''}`}
              aria-pressed={player?.shuffle ?? false}
              aria-label="Shuffle"
              title="Shuffle"
              disabled={!hasTrack}
              onClick={() => void attempt(api.setShuffle(!player?.shuffle))}
            >
              <Shuffle size={16} />
            </button>
          )}
          <button
            className="icon-button"
            aria-label="Previous"
            title="Previous"
            disabled={!hasTrack}
            onClick={() => void attempt(api.previous())}
          >
            <SkipBack size={20} fill="currentColor" />
          </button>
          <button
            className="icon-button play"
            aria-label={playing ? 'Pause' : 'Play'}
            title={playing ? 'Pause' : 'Play'}
            disabled={!hasTrack}
            onClick={() => void attempt(api.toggle())}
          >
            {playing ? (
              <Pause size={24} fill="currentColor" />
            ) : (
              <Play size={24} fill="currentColor" />
            )}
          </button>
          <button
            className="icon-button"
            aria-label="Next"
            title="Next"
            disabled={!hasTrack}
            onClick={() => void attempt(api.next())}
          >
            <SkipForward size={20} fill="currentColor" />
          </button>
          {shows('repeat') && <RepeatButton />}
        </div>
        <Scrubber />
      </div>

      <div className="player-extras">
        {layout.playerControls.map((c) => {
          switch (c) {
            case 'volume':
              return <VolumeControl key={c} />;
            case 'output':
              return <OutputPicker key={c} />;
            case 'miniPlayer':
              return isAndroid ? null : (
                <button
                  key={c}
                  className="icon-button"
                  aria-label="Mini player"
                  title="Mini player (Ctrl+M)"
                  onClick={() => void attempt(api.toggleMiniPlayer())}
                >
                  <PictureInPicture2 size={18} />
                </button>
              );
            case 'queue':
              return (
                <button
                  key={c}
                  className={`icon-button ${queueOpen ? 'active' : ''}`}
                  aria-pressed={queueOpen}
                  aria-label="Queue"
                  title="Queue"
                  onClick={toggleQueue}
                >
                  <ListMusic size={18} />
                </button>
              );
            default:
              return null;
          }
        })}
      </div>
    </footer>
  );
}

function RepeatButton() {
  const repeat = useStore((s) => s.player?.repeat ?? 'off');
  const next = repeat === 'off' ? 'all' : repeat === 'all' ? 'one' : 'off';
  const label = repeat === 'one' ? 'Repeat one' : repeat === 'all' ? 'Repeat all' : 'Repeat off';
  return (
    <button
      className={`icon-button small ${repeat !== 'off' ? 'active' : ''}`}
      aria-label={label}
      title={label}
      onClick={() => void attempt(api.setRepeat(next))}
    >
      {repeat === 'one' ? <Repeat1 size={16} /> : <Repeat size={16} />}
    </button>
  );
}

export function Scrubber() {
  const durationMs = useStore((s) => s.player?.durationMs ?? 0);
  const [drag, setDrag] = useState<number | null>(null);
  const live = usePosition();
  const position = drag ?? live;

  return (
    <div className="scrubber">
      <span className="time">{duration(position)}</span>
      <input
        type="range"
        aria-label="Position"
        min={0}
        max={Math.max(1, durationMs)}
        step={100}
        value={position}
        disabled={!durationMs}
        onChange={(e) => setDrag(Number(e.target.value))}
        onPointerUp={() => {
          if (drag !== null) void attempt(api.seek(drag));
          setDrag(null);
        }}
        onKeyUp={() => {
          if (drag !== null) void attempt(api.seek(drag));
          setDrag(null);
        }}
        style={{ ['--progress' as string]: `${durationMs ? (position / durationMs) * 100 : 0}%` }}
      />
      <span className="time">-{duration(Math.max(0, durationMs - position))}</span>
    </div>
  );
}

function VolumeControl() {
  const volume = useStore((s) => s.player?.volume ?? 1);
  const [local, setLocal] = useState<number | null>(null);
  const v = local ?? volume;
  const Icon = v === 0 ? VolumeX : v < 0.5 ? Volume1 : Volume2;
  return (
    <div className="volume">
      <Icon size={16} aria-hidden />
      <input
        type="range"
        aria-label="Volume"
        min={0}
        max={1}
        step={0.01}
        value={v}
        onChange={(e) => {
          const nv = Number(e.target.value);
          setLocal(nv);
          void attempt(api.setVolume(nv));
        }}
        onPointerUp={() => setLocal(null)}
        style={{ ['--progress' as string]: `${v * 100}%` }}
      />
    </div>
  );
}
