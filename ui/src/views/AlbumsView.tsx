import { useCallback, useEffect, useState } from 'react';
import { api } from '../api';
import { useStore } from '../store';
import type { AlbumSort } from '../types';
import { AlbumGrid, DESKTOP_SIZES, PHONE_COLUMNS } from './AlbumGrid';
import { EmptyLibrary } from './EmptyLibrary';
import { GridZoom } from '../components/GridZoom';
import { usePhone } from '../hooks/usePhone';
import { Select } from '../components/Select';

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

/// Albums (or Recently Added): a grid with a sort order and a card size (− and +, or a pinch).
export function AlbumsView({ recent = false }: { recent?: boolean }) {
  const libraryVersion = useStore((s) => s.libraryVersion);
  const [count, setCount] = useState<number | null>(null);
  const [sort, setSort] = useState<AlbumSort>(() =>
    recent ? 'added' : stored('albums.sort', 'artist'),
  );
  const phone = usePhone();
  const levels = phone ? PHONE_COLUMNS.length : DESKTOP_SIZES.length;
  const key = phone ? 'albums.zoom.phone' : 'albums.zoom';
  const [zooms, setZooms] = useState(() => ({
    phone: Number(stored('albums.zoom.phone', '2')),
    desktop: Number(stored('albums.zoom', '2')),
  }));
  const raw = phone ? zooms.phone : zooms.desktop;
  const zoom = Number.isFinite(raw) ? Math.min(levels - 1, Math.max(0, raw)) : 2;
  const setZoom = (z: number) => {
    const to = Math.min(levels - 1, Math.max(0, z));
    setZooms((was) => (phone ? { ...was, phone: to } : { ...was, desktop: to }));
    remember(key, String(to));
  };

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
              <Select<AlbumSort>
                label="Sort by"
                value={sort}
                options={[
                  { value: 'artist', label: 'Artist' },
                  { value: 'title', label: 'Title' },
                  { value: 'year', label: 'Year' },
                  { value: 'added', label: 'Date Added' },
                ]}
                onChange={(v) => {
                  setSort(v);
                  remember('albums.sort', v);
                }}
              />
            </label>
          )}
          <GridZoom value={zoom} levels={levels} onChange={setZoom} />
        </div>
      </header>
      {count !== null && (
        <AlbumGrid
          count={recent ? Math.min(count, 300) : count}
          load={load}
          zoom={zoom}
          onZoom={(step) => setZoom(zoom + step)}
          resetKey={`${sort}-${recent}-${libraryVersion}`}
        />
      )}
    </section>
  );
}
