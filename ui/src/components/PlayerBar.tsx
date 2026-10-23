import { useEffect, useRef, useState } from 'react';
import {
  Heart,
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
import { setFavourite } from '../actions';
import type { TrackRow } from '../types';
import { Artwork } from './Artwork';
import { Slider } from './Slider';
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
  const queueOpen = useStore((s) => s.dock === 'queue');
  const layout = useLook((s) => s.look.layout);
  const shows = (c: PlayerControl) => layout.playerControls.includes(c);
  const [track, setTrack] = useState<TrackRow | null>(null);
  const libraryVersion = useStore((s) => s.libraryVersion);

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
  }, [trackId, libraryVersion]);

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
            <button
              className={`icon-button small favourite ${shown.favourite ? 'on' : ''}`}
              aria-label="Favourite"
              aria-pressed={shown.favourite}
              title={shown.favourite ? 'Remove from Favourites' : 'Add to Favourites'}
              onClick={() => void setFavourite([shown.id], !shown.favourite)}
            >
              <Heart size={15} fill={shown.favourite ? 'currentColor' : 'none'} />
            </button>
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
  // Where the finger is while dragging; then where it was let go, held briefly so the bar
  // doesn't jump back before playback reports the new position.
  const [drag, setDrag] = useState<number | null>(null);
  const [landed, setLanded] = useState<number | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const live = usePosition();
  const position = drag ?? landed ?? live;
  const ms = (f: number) => Math.round(f * durationMs);

  return (
    <div className="scrubber">
      <span className="time">{duration(position)}</span>
      <Slider
        label="Position"
        value={durationMs ? position / durationMs : 0}
        valueText={`${duration(position)} of ${duration(durationMs)}`}
        step={durationMs ? 5000 / durationMs : 0.05}
        disabled={!durationMs}
        onInput={(f) => setDrag(ms(f))}
        onCommit={(f) => {
          const to = ms(f);
          setDrag(null);
          setLanded(to);
          clearTimeout(timer.current);
          timer.current = setTimeout(() => setLanded(null), 1200);
          void attempt(api.seek(to));
        }}
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
  const set = (nv: number) => void attempt(api.setVolume(nv));
  return (
    <div className="volume">
      <Icon size={16} aria-hidden />
      <Slider
        label="Volume"
        value={v}
        valueText={`${Math.round(v * 100)}%`}
        step={0.05}
        onInput={(nv) => {
          setLocal(nv);
          set(nv);
        }}
        onCommit={(nv) => {
          setLocal(null);
          set(nv);
        }}
      />
    </div>
  );
}
