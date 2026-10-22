import { useEffect, useState } from 'react';
import { api } from '../api';
import { albumMenu, menuHandler } from '../actions';
import { useStore } from '../store';
import type { AlbumDetail, Browse, BrowseItem } from '../types';
import { CompactTracks } from '../components/CompactTracks';

/// One column: an "All" row, then the items. Arrow keys move within it.
function Column<T>({
  label,
  allLabel,
  items,
  selected,
  keyOf,
  nameOf,
  countOf,
  onSelect,
  onContextMenu,
}: {
  label: string;
  allLabel: string | null;
  items: T[];
  selected: string | null;
  keyOf: (item: T) => string;
  nameOf: (item: T) => string;
  countOf?: (item: T) => number;
  onSelect: (key: string | null) => void;
  onContextMenu?: (item: T) => (e: React.MouseEvent) => void;
}) {
  const keys: (string | null)[] = [...(allLabel === null ? [] : [null]), ...items.map(keyOf)];
  const move = (e: React.KeyboardEvent, i: number) => {
    const to = e.key === 'ArrowDown' ? i + 1 : e.key === 'ArrowUp' ? i - 1 : -2;
    if (to === -2) return;
    e.preventDefault();
    const k = keys[Math.max(0, Math.min(keys.length - 1, to))];
    if (k === undefined) return;
    onSelect(k);
    const list = e.currentTarget.parentElement?.parentElement;
    setTimeout(() =>
      list?.querySelector<HTMLElement>('[aria-selected="true"]')?.focus({ preventScroll: false }),
    );
  };
  const row = (key: string | null, name: string, count: number | null, i: number, item?: T) => {
    const on = key === selected;
    return (
      <li key={key ?? '\u0000all'}>
        <button
          role="option"
          aria-selected={on}
          tabIndex={on || (selected === null && i === 0) ? 0 : -1}
          className={on ? 'on' : ''}
          onClick={() => onSelect(key)}
          onKeyDown={(e) => move(e, i)}
          onContextMenu={item && onContextMenu ? onContextMenu(item) : undefined}
        >
          <span>{name}</span>
          {count !== null && <span className="muted">{count}</span>}
        </button>
      </li>
    );
  };
  return (
    <ul className="browser-column scroll" role="listbox" aria-label={label}>
      {allLabel !== null && row(null, allLabel, null, 0)}
      {items.map((item, i) =>
        row(
          keyOf(item),
          nameOf(item),
          countOf ? countOf(item) : null,
          i + (allLabel === null ? 0 : 1),
          item,
        ),
      )}
    </ul>
  );
}

const genreName = (g: BrowseItem) => g.name || 'Unknown Genre';

/// Albums as a column browser: Genre → Artist → Album, with the album's songs below.
export function ColumnBrowser() {
  const libraryVersion = useStore((s) => s.libraryVersion);
  const [genre, setGenre] = useState<string | null>(null);
  const [artist, setArtist] = useState<string | null>(null);
  const [albumId, setAlbumId] = useState<number | null>(null);
  const [data, setData] = useState<Browse | null>(null);
  const [album, setAlbum] = useState<AlbumDetail | null>(null);

  useEffect(() => {
    let live = true;
    void api.browse(genre, artist).then((b) => live && setData(b));
    return () => {
      live = false;
    };
  }, [genre, artist, libraryVersion]);

  useEffect(() => {
    if (albumId === null) return;
    let live = true;
    api.album(albumId).then(
      (a) => live && setAlbum(a),
      () => live && setAlbum(null),
    );
    return () => {
      live = false;
    };
  }, [albumId, libraryVersion]);

  const shown = album && album.album.id === albumId ? album : null;
  return (
    <div className="column-browser">
      <div className="browser-columns">
        <Column
          label="Genres"
          allLabel="All Genres"
          items={data?.genres ?? []}
          selected={genre}
          keyOf={(g) => g.name}
          nameOf={genreName}
          countOf={(g) => g.albumCount}
          onSelect={(g) => {
            setGenre(g);
            setArtist(null);
            setAlbumId(null);
          }}
        />
        <Column
          label="Artists"
          allLabel="All Artists"
          items={data?.artists ?? []}
          selected={artist}
          keyOf={(a) => a.name}
          nameOf={(a) => a.name || 'Unknown Artist'}
          countOf={(a) => a.albumCount}
          onSelect={(a) => {
            setArtist(a);
            setAlbumId(null);
          }}
        />
        <Column
          label="Albums"
          allLabel={null}
          items={data?.albums ?? []}
          selected={albumId === null ? null : String(albumId)}
          keyOf={(a) => String(a.id)}
          nameOf={(a) => [a.title || 'Unknown Album', a.year].filter(Boolean).join(' · ')}
          onSelect={(id) => setAlbumId(id === null ? null : Number(id))}
          onContextMenu={(a) => menuHandler(() => albumMenu(a))}
        />
      </div>
      <div className="browser-songs scroll">
        {shown ? (
          <CompactTracks tracks={shown.tracks} />
        ) : (
          <p className="muted">Choose an album to see its songs.</p>
        )}
      </div>
    </div>
  );
}
