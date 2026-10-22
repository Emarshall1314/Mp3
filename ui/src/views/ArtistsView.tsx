import { useEffect, useState } from 'react';
import { Play, Shuffle } from 'lucide-react';
import { api } from '../api';
import { attempt, useStore } from '../store';
import type { AlbumRow, ArtistRow } from '../types';
import { Artwork } from '../components/Artwork';
import { AlbumCard } from './AlbumGrid';
import { EmptyLibrary } from './EmptyLibrary';

/// Artists: list on the left, the selected artist's albums on the right.
export function ArtistsView({ selected }: { selected?: string }) {
  const libraryVersion = useStore((s) => s.libraryVersion);
  const navigate = useStore((s) => s.navigate);
  const [artists, setArtists] = useState<ArtistRow[] | null>(null);

  useEffect(() => {
    void api.artists().then(setArtists);
  }, [libraryVersion]);

  if (artists && artists.length === 0) return <EmptyLibrary />;
  const current = selected ?? artists?.[0]?.name;

  return (
    <section className="view artists-view">
      <ul className="artist-list scroll" aria-label="Artists">
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

function ArtistDetail({ name }: { name: string }) {
  const libraryVersion = useStore((s) => s.libraryVersion);
  const [albums, setAlbums] = useState<AlbumRow[]>([]);

  useEffect(() => {
    void api.artistAlbums(name).then(setAlbums);
  }, [name, libraryVersion]);

  const playAll = async (shuffle: boolean) => {
    const details = await Promise.all(albums.map((a) => api.album(a.id)));
    const ids = details.flatMap((d) => d.tracks.map((t) => t.id));
    await api.playTracks(ids, 0, shuffle);
  };

  return (
    <div className="artist-detail scroll">
      <header className="view-header">
        <h1>{name || 'Unknown Artist'}</h1>
        <div className="view-controls">
          <button
            className="pill primary"
            disabled={!albums.length}
            onClick={() => void attempt(playAll(false))}
          >
            <Play size={14} fill="currentColor" aria-hidden /> Play
          </button>
          <button
            className="pill"
            disabled={!albums.length}
            onClick={() => void attempt(playAll(true))}
          >
            <Shuffle size={14} aria-hidden /> Shuffle
          </button>
        </div>
      </header>
      <h2 className="section-title">Albums</h2>
      <div className="card-grid">
        {albums.map((a) => (
          <AlbumCard key={a.id} album={a} size={180} />
        ))}
      </div>
    </div>
  );
}
