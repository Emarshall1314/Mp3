import { useEffect, useState } from 'react';
import { Maximize2, Pause, Play, SkipBack, SkipForward, X } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { api, onApiEvent } from './api';
import type { PlayerState, TrackRow } from './types';
import { Artwork } from './components/Artwork';

/// The always-on-top mini player window: artwork, title, transport. Drag it by its body.
export function MiniPlayer() {
  const [player, setPlayer] = useState<PlayerState | null>(null);
  const [track, setTrack] = useState<TrackRow | null>(null);

  useEffect(() => {
    void api.player().then(setPlayer);
    const unlisten = onApiEvent((e) => {
      if (e.kind === 'player' && e.event.type === 'state') {
        const { type: _type, ...state } = e.event;
        setPlayer(state);
      }
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, []);

  const trackId = player?.trackId ?? null;
  useEffect(() => {
    if (trackId === null) return;
    let live = true;
    void api.tracks([trackId]).then((rows) => live && setTrack(rows[0] ?? null));
    return () => {
      live = false;
    };
  }, [trackId]);

  const shown = track && track.id === trackId ? track : null;
  const playing = player?.status === 'playing';
  const close = () => void invoke('toggle_mini_player');

  return (
    <div className="mini-player" data-tauri-drag-region>
      <Artwork hash={shown?.artHash ?? null} size={64} alt="" />
      <div className="mini-text" data-tauri-drag-region>
        <span className="title">{shown?.title ?? 'Not Playing'}</span>
        <span className="subtitle">{shown?.artist ?? ''}</span>
      </div>
      <div className="mini-buttons">
        <button className="icon-button" aria-label="Previous" onClick={() => void api.previous()}>
          <SkipBack size={16} fill="currentColor" />
        </button>
        <button
          className="icon-button"
          aria-label={playing ? 'Pause' : 'Play'}
          onClick={() => void api.toggle()}
          disabled={!shown}
        >
          {playing ? (
            <Pause size={20} fill="currentColor" />
          ) : (
            <Play size={20} fill="currentColor" />
          )}
        </button>
        <button className="icon-button" aria-label="Next" onClick={() => void api.next()}>
          <SkipForward size={16} fill="currentColor" />
        </button>
      </div>
      <div className="mini-window-buttons">
        <button
          className="icon-button small"
          aria-label="Show Cadenza"
          title="Show Cadenza"
          onClick={() => void invoke('show_main_window')}
        >
          <Maximize2 size={12} />
        </button>
        <button className="icon-button small" aria-label="Close mini player" onClick={close}>
          <X size={12} />
        </button>
      </div>
    </div>
  );
}
