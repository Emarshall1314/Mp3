import { useEffect, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { ChevronDown, ChevronUp, Shuffle } from 'lucide-react';
import { api } from '../api';
import { menuHandler, trackMenu } from '../actions';
import { duration, trackSpec } from '../format';
import { attempt, useStore } from '../store';
import type { SongSort, TrackRow } from '../types';
import { EmptyLibrary } from './EmptyLibrary';
import { QualityBadge } from '../components/QualityBadge';

const ROW = 34;
const PAGE = 200;

const COLUMNS: { sort: SongSort | null; label: string; className: string }[] = [
  { sort: 'title', label: 'Title', className: 'c-title' },
  { sort: 'artist', label: 'Artist', className: 'c-artist' },
  { sort: 'album', label: 'Album', className: 'c-album' },
  { sort: 'year', label: 'Year', className: 'c-year' },
  { sort: 'genre', label: 'Genre', className: 'c-genre' },
  { sort: null, label: 'Quality', className: 'c-quality' },
  { sort: 'plays', label: 'Plays', className: 'c-plays' },
  { sort: 'duration', label: 'Time', className: 'c-time' },
];

/// Every song, virtualised: only the visible rows exist in the DOM, fetched in pages.
export function SongsView() {
  const libraryVersion = useStore((s) => s.libraryVersion);
  const playingId = useStore((s) => s.player?.trackId);
  const [count, setCount] = useState<number | null>(null);
  const [sort, setSort] = useState<SongSort>('artist');
  const [desc, setDesc] = useState(false);
  const [pages, setPages] = useState<Map<number, TrackRow[]>>(new Map());
  const [selected, setSelected] = useState<number | null>(null);
  const pending = useRef(new Set<number>());
  const scroller = useRef<HTMLDivElement>(null);

  useEffect(() => {
    void api.stats().then((s) => setCount(s.tracks));
  }, [libraryVersion]);

  useEffect(() => {
    setPages(new Map());
    pending.current.clear();
  }, [sort, desc, libraryVersion]);

  const v = useVirtualizer({
    count: count ?? 0,
    getScrollElement: () => scroller.current,
    estimateSize: () => ROW,
    overscan: 20,
  });
  const items = v.getVirtualItems();
  const first = items[0]?.index ?? 0;
  const last = items.at(-1)?.index ?? 0;

  useEffect(() => {
    if (!count) return;
    for (let p = Math.floor(first / PAGE); p <= Math.floor(last / PAGE); p++) {
      if (pages.has(p) || pending.current.has(p)) continue;
      pending.current.add(p);
      const key = `${sort}-${desc}`;
      void api.songs(sort, desc, p * PAGE, PAGE).then((rows) => {
        pending.current.delete(p);
        if (key === `${sort}-${desc}`) setPages((prev) => new Map(prev).set(p, rows));
      });
    }
  }, [first, last, pages, sort, desc, count]);

  if (count === 0) return <EmptyLibrary />;
  const row = (i: number) => pages.get(Math.floor(i / PAGE))?.[i % PAGE];
  const play = (i: number) => void attempt(api.playSongs(sort, desc, i));

  return (
    <section className="view">
      <header className="view-header">
        <h1>Songs</h1>
        <div className="view-controls">
          <span className="muted">{count?.toLocaleString()} songs</span>
          <button
            className="pill"
            onClick={() => void attempt(api.playSongs(sort, desc, 0, true))}
            disabled={!count}
          >
            <Shuffle size={14} aria-hidden /> Shuffle All
          </button>
        </div>
      </header>
      <div className="song-table" role="grid" aria-rowcount={count ?? 0}>
        <div className="song-head" role="row">
          {COLUMNS.map((c) => (
            <div
              key={c.label}
              role="columnheader"
              className={c.className}
              aria-sort={c.sort === sort ? (desc ? 'descending' : 'ascending') : undefined}
            >
              {c.sort ? (
                <button
                  className="column-button"
                  onClick={() => {
                    if (c.sort === sort) setDesc(!desc);
                    else {
                      setSort(c.sort as SongSort);
                      setDesc(false);
                    }
                  }}
                >
                  {c.label}
                  {c.sort === sort && (desc ? <ChevronDown size={12} /> : <ChevronUp size={12} />)}
                </button>
              ) : (
                c.label
              )}
            </div>
          ))}
        </div>
        <div className="scroll" ref={scroller}>
          <div style={{ height: v.getTotalSize(), position: 'relative' }}>
            {items.map((it) => {
              const t = row(it.index);
              return (
                <div
                  key={it.key}
                  role="row"
                  aria-rowindex={it.index + 1}
                  aria-selected={selected === it.index}
                  tabIndex={0}
                  className={`song-row ${it.index % 2 ? 'odd' : ''} ${t && t.id === playingId ? 'playing' : ''} ${selected === it.index ? 'selected' : ''}`}
                  style={{ transform: `translateY(${it.start}px)`, height: ROW }}
                  onClick={() => setSelected(it.index)}
                  onDoubleClick={() => play(it.index)}
                  onKeyDown={(e) => e.key === 'Enter' && play(it.index)}
                  onContextMenu={t ? menuHandler(() => trackMenu(t)) : undefined}
                >
                  {t ? (
                    <>
                      <div role="gridcell" className="c-title">
                        {t.title}
                      </div>
                      <div role="gridcell" className="c-artist">
                        {t.artist}
                      </div>
                      <div role="gridcell" className="c-album">
                        {t.album}
                      </div>
                      <div role="gridcell" className="c-year">
                        {t.year ?? ''}
                      </div>
                      <div role="gridcell" className="c-genre">
                        {t.genre}
                      </div>
                      <div role="gridcell" className="c-quality">
                        <QualityBadge quality={t.quality} detail={trackSpec(t)} />
                      </div>
                      <div role="gridcell" className="c-plays">
                        {t.playCount || ''}
                      </div>
                      <div role="gridcell" className="c-time">
                        {duration(t.durationMs)}
                      </div>
                    </>
                  ) : (
                    <div role="gridcell" className="loading-cell" />
                  )}
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </section>
  );
}
