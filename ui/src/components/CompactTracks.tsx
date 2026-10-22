import { api } from '../api';
import { menuHandler, trackMenu } from '../actions';
import { duration } from '../format';
import { attempt, useStore } from '../store';
import type { TrackRow } from '../types';
import { Artwork } from './Artwork';

/// A short list of songs from different albums (search results, an artist's top songs): artwork,
/// title, artist and album. Playing one queues the whole list from there.
export function CompactTracks({
  tracks,
  showPlays = false,
}: {
  tracks: TrackRow[];
  showPlays?: boolean;
}) {
  const playingId = useStore((s) => s.player?.trackId);
  const play = (i: number) =>
    void attempt(
      api.playTracks(
        tracks.map((t) => t.id),
        i,
      ),
    );
  return (
    <div className="result-songs" role="list">
      {tracks.map((t, i) => (
        <div
          key={t.id}
          role="listitem"
          className={`result-song ${t.id === playingId ? 'playing' : ''}`}
          onDoubleClick={() => play(i)}
          onContextMenu={menuHandler(() => trackMenu(t))}
        >
          <button className="art-button" aria-label={`Play ${t.title}`} onClick={() => play(i)}>
            <Artwork hash={t.artHash} size={40} alt="" />
          </button>
          <div className="queue-text">
            <span className="title">{t.title}</span>
            <span className="subtitle">
              {t.artist} — {t.album}
            </span>
          </div>
          <span className="time">
            {showPlays && t.playCount > 0 && (
              <span className="plays">
                {t.playCount} {t.playCount === 1 ? 'play' : 'plays'}
              </span>
            )}
            {duration(t.durationMs)}
          </span>
        </div>
      ))}
    </div>
  );
}
