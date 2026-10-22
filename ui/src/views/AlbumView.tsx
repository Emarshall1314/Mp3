import { useEffect, useMemo, useState } from 'react';
import { Play, Shuffle } from 'lucide-react';
import { api } from '../api';
import { menuHandler, trackMenu } from '../actions';
import { duration, longDuration, trackSpec } from '../format';
import { attempt, useStore } from '../store';
import type { AlbumDetail } from '../types';
import { Artwork } from '../components/Artwork';
import { QualityBadge } from '../components/QualityBadge';
import { AlbumCard } from './AlbumGrid';

/// Album page: big artwork, title, artist link, meta + badge, Play/Shuffle, tracklist, footer.
export function AlbumView({ id }: { id: number }) {
  const navigate = useStore((s) => s.navigate);
  const libraryVersion = useStore((s) => s.libraryVersion);
  const playingId = useStore((s) => s.player?.trackId);
  const [detail, setDetail] = useState<AlbumDetail | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    api.album(id).then(
      (d) => live && setDetail(d),
      (e: unknown) => live && setError(String(e)),
    );
    return () => {
      live = false;
    };
  }, [id, libraryVersion]);

  const discs = useMemo(() => {
    const groups = new Map<number, AlbumDetail['tracks']>();
    for (const t of detail?.tracks ?? []) {
      const d = t.discNo ?? 1;
      groups.set(d, [...(groups.get(d) ?? []), t]);
    }
    return [...groups.entries()];
  }, [detail]);

  if (error)
    return (
      <section className="view empty-state">
        <p role="alert">{error}</p>
      </section>
    );
  if (!detail) return <section className="view" aria-busy="true" />;
  const { album } = detail;
  const best = detail.tracks.find((t) => t.quality === album.quality) ?? detail.tracks[0];
  let index = 0;

  return (
    <section className="view scroll album-page">
      <header className="album-hero">
        <Artwork
          hash={album.artHash}
          size={260}
          alt={`${album.title} artwork`}
          className="hero-art"
        />
        <div className="album-hero-text">
          <h1>{album.title || 'Unknown Album'}</h1>
          <button
            className="link artist-link"
            onClick={() => navigate({ name: 'artist', artist: album.albumartist })}
          >
            {album.albumartist}
          </button>
          <p className="album-meta">
            {[album.genre, album.year].filter(Boolean).join(' · ')}
            {album.quality && best && (
              <>
                {album.genre || album.year ? ' · ' : ''}
                <QualityBadge quality={album.quality} detail={trackSpec(best)} />
              </>
            )}
          </p>
          <div className="hero-buttons">
            <button
              className="pill primary"
              onClick={() => void attempt(api.playAlbum(album.id, 0))}
            >
              <Play size={16} fill="currentColor" aria-hidden /> Play
            </button>
            <button className="pill" onClick={() => void attempt(api.playAlbum(album.id, 0, true))}>
              <Shuffle size={16} aria-hidden /> Shuffle
            </button>
          </div>
        </div>
      </header>

      <div className="tracklist" role="list">
        {discs.map(([disc, tracks]) => (
          <div
            key={disc}
            role="group"
            aria-label={detail.discCount > 1 ? `Disc ${disc}` : undefined}
          >
            {detail.discCount > 1 && <h2 className="disc-header">Disc {disc}</h2>}
            {tracks.map((t) => {
              const at = index++;
              const featured = t.artist && t.artist !== album.albumartist ? t.artist : null;
              return (
                <div
                  key={t.id}
                  role="listitem"
                  className={`track-row ${t.id === playingId ? 'playing' : ''}`}
                  onDoubleClick={() => void attempt(api.playAlbum(album.id, at))}
                  onContextMenu={menuHandler(() => trackMenu(t))}
                >
                  <button
                    className="track-no"
                    aria-label={`Play ${t.title}`}
                    onClick={() => void attempt(api.playAlbum(album.id, at))}
                  >
                    <span className="num">{t.trackNo ?? ''}</span>
                    <Play size={12} fill="currentColor" className="play-icon" aria-hidden />
                  </button>
                  <div className="track-title">
                    <span>{t.title}</span>
                    {featured && <span className="featured">{featured}</span>}
                  </div>
                  <span className="track-rating">
                    {t.rating ? '★'.repeat(Math.round(t.rating / 20)) : ''}
                  </span>
                  <span className="time">{duration(t.durationMs)}</span>
                </div>
              );
            })}
          </div>
        ))}
      </div>

      <footer className="album-footer">
        {detail.date && <p>{detail.date}</p>}
        <p>
          {album.trackCount} {album.trackCount === 1 ? 'song' : 'songs'},{' '}
          {longDuration(album.durationMs)}
        </p>
        {detail.label && <p>{detail.label}</p>}
      </footer>

      {detail.otherVersions.length > 0 && (
        <section className="other-versions">
          <h2>Other Versions</h2>
          <div className="card-row">
            {detail.otherVersions.map((a) => (
              <AlbumCard key={a.id} album={a} size={160} />
            ))}
          </div>
        </section>
      )}
    </section>
  );
}
