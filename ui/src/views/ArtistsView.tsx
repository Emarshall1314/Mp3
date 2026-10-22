import { useEffect, useState } from 'react';
import { Play, Shuffle } from 'lucide-react';
import { api } from '../api';
import { attempt, useStore } from '../store';
import type { ArtistPage, ArtistRow, ReleaseSection } from '../types';
import { Artwork } from '../components/Artwork';
import { CompactTracks } from '../components/CompactTracks';
import { AlbumCard } from './AlbumGrid';
import { EmptyLibrary } from './EmptyLibrary';
import { ViewToggle } from '../components/ViewToggle';
import { updateLayout, useLook } from '../hooks/useAppearance';
import type { GridOrList } from '../theme/layout';

/// Artists: list on the left, the selected artist's albums on the right.
export function ArtistsView({ selected }: { selected?: string }) {
  const libraryVersion = useStore((s) => s.libraryVersion);
  const navigate = useStore((s) => s.navigate);
  const [artists, setArtists] = useState<ArtistRow[] | null>(null);

  useEffect(() => {
    void api.artists().then(setArtists);
  }, [libraryVersion]);

  const view = useLook((s) => s.look.layout.views.artists);
  const setView = (artists: GridOrList) =>
    updateLayout({ views: { ...useLook.getState().look.layout.views, artists } });

  if (artists && artists.length === 0) return <EmptyLibrary />;

  if (view === 'grid') {
    if (selected !== undefined)
      return (
        <section className="view artists-view">
          <ArtistDetail name={selected} />
        </section>
      );
    return (
      <section className="view">
        <header className="view-header">
          <h1>Artists</h1>
          <div className="view-controls">
            <ViewToggle value={view} onChange={setView} />
          </div>
        </header>
        <ul className="artist-grid scroll" aria-label="Artists">
          {artists?.map((a) => (
            <li key={a.name}>
              <button
                className="artist-card"
                onClick={() => navigate({ name: 'artist', artist: a.name })}
              >
                <Artwork hash={a.artHash} size={140} alt="" className="round" />
                <span>{a.name || 'Unknown Artist'}</span>
                <span className="subtitle">
                  {a.albumCount} {a.albumCount === 1 ? 'album' : 'albums'}
                </span>
              </button>
            </li>
          ))}
        </ul>
      </section>
    );
  }

  const current = selected ?? artists?.[0]?.name;
  return (
    <section className="view artists-view">
      <ul className="artist-list scroll" aria-label="Artists">
        <li className="artist-list-controls">
          <ViewToggle value={view} onChange={setView} />
        </li>
        {artists?.map((a) => (
          <li key={a.name}>
            <button
              className={`artist-item ${a.name === current ? 'active' : ''}`}
              onClick={() => navigate({ name: 'artist', artist: a.name })}
            >
              <Artwork hash={a.artHash} size={36} alt="" className="round" />
              <span>{a.name || 'Unknown Artist'}</span>
            </button>
          </li>
        ))}
      </ul>
      {current !== undefined && <ArtistDetail name={current} />}
    </section>
  );
}

const SECTION_TITLES: Record<ReleaseSection['kind'], string> = {
  albums: 'Albums',
  eps: 'EPs',
  singles: 'Singles',
  live: 'Live',
  compilations: 'Compilations',
  soundtracks: 'Soundtracks',
  other: 'Other Releases',
};

/// An artist's page: hero, Play/Shuffle, top songs by play count, releases by type, Appears On.
function ArtistDetail({ name }: { name: string }) {
  const libraryVersion = useStore((s) => s.libraryVersion);
  const [page, setPage] = useState<ArtistPage | null>(null);

  useEffect(() => {
    let live = true;
    void api.artistPage(name).then((p) => live && setPage(p));
    return () => {
      live = false;
    };
  }, [name, libraryVersion]);

  const own = page?.sections.flatMap((s) => s.albums) ?? [];
  const playAll = async (shuffle: boolean) => {
    const details = await Promise.all(own.map((a) => api.album(a.id)));
    const ids = details.flatMap((d) => d.tracks.map((t) => t.id));
    await api.playTracks(ids, 0, shuffle);
  };

  return (
    <div className="artist-detail scroll" aria-busy={!page}>
      <header className="artist-hero">
        <Artwork hash={page?.artHash ?? null} size={160} alt="" className="round hero-art" />
        <div>
          <h1>{name || 'Unknown Artist'}</h1>
          <p className="muted">
            {own.length} {own.length === 1 ? 'release' : 'releases'}
            {page && page.appearsOn.length > 0 && `, appears on ${page.appearsOn.length} more`}
          </p>
          <div className="hero-buttons">
            <button
              className="pill primary"
              disabled={!own.length}
              onClick={() => void attempt(playAll(false))}
            >
              <Play size={14} fill="currentColor" aria-hidden /> Play
            </button>
            <button
              className="pill"
              disabled={!own.length}
              onClick={() => void attempt(playAll(true))}
            >
              <Shuffle size={14} aria-hidden /> Shuffle
            </button>
          </div>
        </div>
      </header>
      {page && page.topSongs.length > 0 && (
        <section aria-label="Top Songs">
          <h2 className="section-title">Top Songs</h2>
          <CompactTracks tracks={page.topSongs} showPlays />
        </section>
      )}
      {page?.sections.map((s) => (
        <section key={s.kind} aria-label={SECTION_TITLES[s.kind]}>
          <h2 className="section-title">{SECTION_TITLES[s.kind]}</h2>
          <div className="card-grid">
            {s.albums.map((a) => (
              <AlbumCard key={a.id} album={a} size={170} />
            ))}
          </div>
        </section>
      ))}
      {page && page.appearsOn.length > 0 && (
        <section aria-label="Appears On">
          <h2 className="section-title">Appears On</h2>
          <div className="card-row">
            {page.appearsOn.map((a) => (
              <AlbumCard key={a.id} album={a} size={150} />
            ))}
          </div>
        </section>
      )}
    </div>
  );
}
