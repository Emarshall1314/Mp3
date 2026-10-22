import { useCallback, useEffect, useState } from 'react';
import { api } from '../api';
import { useStore } from '../store';
import type { AlbumSort } from '../types';
import { AlbumGrid } from './AlbumGrid';
import { EmptyLibrary } from './EmptyLibrary';
import { AlbumList } from './AlbumList';
import { ViewToggle } from '../components/ViewToggle';
import { updateLayout, useLook } from '../hooks/useAppearance';

function stored<T extends string>(key: string, fallback: T): T {
  try {
    return (localStorage.getItem(key) as T | null) ?? fallback;
  } catch {
    return fallback;
  }
}

function remember(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Private mode or blocked storage: the setting just isn't remembered.
  }
}

/// Albums (or Recently Added): grid with a size slider and sort order.
export function AlbumsView({ recent = false }: { recent?: boolean }) {
  const libraryVersion = useStore((s) => s.libraryVersion);
  const [count, setCount] = useState<number | null>(null);
  const [sort, setSort] = useState<AlbumSort>(() =>
    recent ? 'added' : stored('albums.sort', 'artist'),
  );
  const [size, setSize] = useState(() => Number(stored('albums.size', '180')) || 180);
  const section = recent ? 'recent' : 'albums';
  const view = useLook((s) => s.look.layout.views[section]);

  useEffect(() => {
    void api.stats().then((s) => setCount(s.albums));
  }, [libraryVersion]);

  const load = useCallback(
    (offset: number, limit: number) => api.albums(recent ? 'added' : sort, offset, limit),
    [sort, recent],
  );

  if (count === 0) return <EmptyLibrary />;
  return (
    <section className="view">
      <header className="view-header">
        <h1>{recent ? 'Recently Added' : 'Albums'}</h1>
        <div className="view-controls">
          {!recent && (
            <label>
              Sort by{' '}
              <select
                value={sort}
                onChange={(e) => {
                  setSort(e.target.value as AlbumSort);
                  remember('albums.sort', e.target.value);
                }}
              >
                <option value="artist">Artist</option>
                <option value="title">Title</option>
                <option value="year">Year</option>
                <option value="added">Date Added</option>
              </select>
            </label>
          )}
          <ViewToggle
            value={view}
            onChange={(v) =>
              updateLayout({ views: { ...useLook.getState().look.layout.views, [section]: v } })
            }
          />
          {view === 'grid' && (
            <input
              type="range"
              aria-label="Artwork size"
              min={120}
              max={300}
              step={10}
              value={size}
              onChange={(e) => {
                setSize(Number(e.target.value));
                remember('albums.size', e.target.value);
              }}
            />
          )}
        </div>
      </header>
      {count !== null && view === 'list' && (
        <AlbumList
          count={recent ? Math.min(count, 300) : count}
          load={load}
          resetKey={`${sort}-${recent}-${libraryVersion}`}
        />
      )}
      {count !== null && view === 'grid' && (
        <AlbumGrid
          count={recent ? Math.min(count, 300) : count}
          load={load}
          size={size}
          resetKey={`${sort}-${recent}-${libraryVersion}`}
        />
      )}
    </section>
  );
}
