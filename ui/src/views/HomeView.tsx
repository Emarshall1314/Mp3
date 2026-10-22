import { useEffect, useState } from 'react';
import { api } from '../api';
import { useStore } from '../store';
import type { AlbumRow, Home } from '../types';
import { AlbumCard } from './AlbumGrid';
import { EmptyLibrary } from './EmptyLibrary';

/// Home: shelves of what you've been listening to and what's new. Shelves without albums are
/// left out rather than shown empty.
export function HomeView() {
  const libraryVersion = useStore((s) => s.libraryVersion);
  // Plays change the shelves too; refetch when the track changes.
  const trackId = useStore((s) => s.player?.trackId);
  const [home, setHome] = useState<Home | null>(null);

  useEffect(() => {
    void api.home().then(setHome);
  }, [libraryVersion, trackId]);

  if (!home) return <section className="view" aria-busy="true" />;
  if (home.recentlyAdded.length === 0) return <EmptyLibrary />;

  return (
    <section className="view scroll home-view">
      <header className="view-header">
        <h1>Home</h1>
      </header>
      <Shelf title="Recently Played" albums={home.recentlyPlayed} />
      <Shelf title="Recently Added" albums={home.recentlyAdded} more={{ name: 'recent' }} />
      <Shelf title="Most Played" albums={home.mostPlayed} />
      {home.recentlyPlayed.length === 0 && (
        <p className="muted home-hint">
          Play something and your recently and most played albums appear here.
        </p>
      )}
    </section>
  );
}

export function Shelf({
  title,
  albums,
  more,
}: {
  title: string;
  albums: AlbumRow[];
  more?: { name: 'recent' };
}) {
  const navigate = useStore((s) => s.navigate);
  if (albums.length === 0) return null;
  return (
    <section className="shelf" aria-label={title}>
      <div className="shelf-header">
        <h2 className="section-title">{title}</h2>
        {more && (
          <button className="link see-all" onClick={() => navigate(more)}>
            See All
          </button>
        )}
      </div>
      <div className="card-row">
        {albums.map((a) => (
          <AlbumCard key={a.id} album={a} size={170} />
        ))}
      </div>
    </section>
  );
}
