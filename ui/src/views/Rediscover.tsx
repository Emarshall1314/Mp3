import { useEffect, useState } from 'react';
import { Play, Shuffle } from 'lucide-react';
import { api } from '../api';
import { attempt, useStore } from '../store';
import type { Mix, Rediscover, TrackRow } from '../types';
import { Artwork } from '../components/Artwork';
import { CompactTracks } from '../components/CompactTracks';
import { AlbumCard } from './AlbumGrid';

/// Today's Rediscover, fetched once per page visit (and when the library changes); the picks
/// only change at the owner's midnight anyway.
function useRediscover(): Rediscover | null {
  const libraryVersion = useStore((s) => s.libraryVersion);
  const [r, setR] = useState<Rediscover | null>(null);
  useEffect(() => {
    let live = true;
    void api.rediscover().then(
      (x) => live && setR(x),
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, [libraryVersion]);
  return r;
}

const REFRESH: Record<Mix['refresh'], string> = {
  daily: 'New every day',
  weekly: 'New every week',
};

/// A mix's cover: four of its artworks in a square (or one, when it has fewer), with its title.
function MixCover({ mix, size }: { mix: Mix; size: number }) {
  const four = mix.art.length >= 4;
  return (
    <div className="mix-cover" data-mix={mix.id} style={{ width: size, height: size }}>
      {four ? (
        mix.art.slice(0, 4).map((a) => <Artwork key={a} hash={a} size={size / 2} alt="" />)
      ) : (
        <Artwork hash={mix.art[0] ?? null} size={size} alt="" />
      )}
      <span className="mix-cover-title">{mix.title}</span>
    </div>
  );
}

function playMix(mix: Mix, shuffle = false) {
  void attempt(api.playTracks(mix.trackIds, 0, shuffle));
}

/// Home's Rediscover shelf: the album of the day, the day's mixes, and this week's still
/// unplayed albums. Parts with nothing in them are left out; with no listening yet at all it
/// says how it fills up.
export function RediscoverShelf() {
  const r = useRediscover();
  const navigate = useStore((s) => s.navigate);
  if (!r) return null;
  const day = r.albumOfTheDay;
  const empty = !day && r.mixes.length === 0 && r.unplayed.length === 0;
  return (
    <section className="shelf rediscover" aria-label="Rediscover">
      <h2 className="section-title">Rediscover</h2>
      {empty && (
        <p className="muted home-hint">
          As you listen, this fills with mixes of forgotten favourites, songs you haven’t tried and
          what you play at this time of day, plus an album to revisit each day.
        </p>
      )}
      {day && (
        <div className="album-of-the-day">
          <button
            className="aotd-art"
            onClick={() => navigate({ name: 'album', id: day.id })}
            aria-label={`${day.title} by ${day.albumartist}`}
          >
            <Artwork hash={day.artHash} size={132} alt="" />
          </button>
          <div className="aotd-text">
            <span className="aotd-label">Album of the Day</span>
            <span className="title">{day.title}</span>
            <span className="subtitle">{day.albumartist}</span>
            <button className="pill primary" onClick={() => void attempt(api.playAlbum(day.id, 0))}>
              <Play size={14} fill="currentColor" aria-hidden /> Play
            </button>
          </div>
        </div>
      )}
      {r.mixes.length > 0 && (
        <div className="card-row mix-row">
          {r.mixes.map((m) => (
            <div key={m.id} className="mix-card">
              <button
                className="mix-open"
                onClick={() => navigate({ name: 'mix', id: m.id })}
                aria-label={`${m.title}: ${m.trackIds.length} songs`}
              >
                <MixCover mix={m} size={170} />
              </button>
              <button
                className="play-overlay"
                aria-label={`Play ${m.title}`}
                onClick={() => playMix(m)}
              >
                <Play size={18} fill="currentColor" />
              </button>
              <span className="subtitle">{m.subtitle}</span>
            </div>
          ))}
        </div>
      )}
      {r.unplayed.length > 0 && (
        <>
          <h3 className="shelf-subtitle">Still Unplayed · this week</h3>
          <div className="card-row">
            {r.unplayed.map((a) => (
              <AlbumCard key={a.id} album={a} size={150} />
            ))}
          </div>
        </>
      )}
    </section>
  );
}

/// A mix's own page: its cover, what it is, Play and Shuffle, and its songs.
export function MixView({ id }: { id: string }) {
  const r = useRediscover();
  const mix = r?.mixes.find((m) => m.id === id) ?? null;
  const [tracks, setTracks] = useState<TrackRow[] | null>(null);
  const ids = mix?.trackIds.join(',');
  useEffect(() => {
    if (!mix) return;
    let live = true;
    void api.tracks(mix.trackIds).then((rows) => {
      if (!live) return;
      // In the mix's order.
      const by = new Map(rows.map((t) => [t.id, t]));
      setTracks(mix.trackIds.flatMap((i) => by.get(i) ?? []));
    });
    return () => {
      live = false;
    };
    // The ids are the mix's identity for the day.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ids]);
  if (!r) return <section className="view" aria-busy="true" />;
  if (!mix)
    return (
      <section className="view scroll">
        <header className="view-header">
          <h1>Mix</h1>
        </header>
        <p className="muted home-hint">This mix isn’t in today’s Rediscover any more.</p>
      </section>
    );
  return (
    <section className="view scroll mix-page">
      <div className="album-hero">
        <MixCover mix={mix} size={220} />
        <div className="album-hero-text">
          <h1>{mix.title}</h1>
          <p className="muted">{mix.subtitle}</p>
          <p className="muted small">
            {mix.trackIds.length} songs · {REFRESH[mix.refresh]}
          </p>
          <div className="action-row">
            <button className="pill primary" onClick={() => playMix(mix)}>
              <Play size={14} fill="currentColor" aria-hidden /> Play
            </button>
            <button className="pill" onClick={() => playMix(mix, true)}>
              <Shuffle size={14} aria-hidden /> Shuffle
            </button>
          </div>
        </div>
      </div>
      {tracks && <CompactTracks tracks={tracks} showPlays />}
    </section>
  );
}
