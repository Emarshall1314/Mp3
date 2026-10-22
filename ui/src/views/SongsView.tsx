import { useEffect, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { ChevronDown, ChevronUp, Shuffle } from 'lucide-react';
import { api } from '../api';
import { menuHandler, trackMenu } from '../actions';
import { duration, trackSpec } from '../format';
import { attempt, useStore } from '../store';
import type { SongSort, TrackRow } from '../types';
import { EmptyLibrary } from './EmptyLibrary';
import { updateLayout, useLook } from '../hooks/useAppearance';
import { COLUMN_SPECS, type Column, type ColumnId, columnTemplate, moveTo } from '../theme/layout';
import { DENSITY } from '../theme/look';
import { QualityBadge } from '../components/QualityBadge';

const PAGE = 200;

/// One cell's content for a column.
function cell(id: ColumnId, t: TrackRow) {
  switch (id) {
    case 'title':
      return t.title;
    case 'artist':
      return t.artist;
    case 'album':
      return t.album;
    case 'albumartist':
      return t.albumartist;
    case 'track':
      return t.trackNo ?? '';
    case 'year':
      return t.year ?? '';
    case 'genre':
      return t.genre;
    case 'format':
      return t.codec.toUpperCase();
    case 'quality':
      return <QualityBadge quality={t.quality} detail={trackSpec(t)} />;
    case 'plays':
      return t.playCount || '';
    case 'time':
      return duration(t.durationMs);
  }
}

/// A column header: sorts on click, reorders by dragging, resizes from its right edge.
function ColumnHeader({
  column,
  index,
  sort,
  desc,
  onSort,
}: {
  column: Column;
  index: number;
  sort: SongSort;
  desc: boolean;
  onSort: (s: SongSort) => void;
}) {
  const spec = COLUMN_SPECS[column.id];
  const resize = (e: React.PointerEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const target = e.currentTarget as HTMLElement;
    target.setPointerCapture(e.pointerId);
    const startX = e.clientX;
    const startWidth = column.width;
    const move = (m: PointerEvent) => {
      const columns = useLook.getState().look.layout.columns.map((c) =>
        c.id === column.id
          ? {
              ...c,
              width: Math.round(Math.min(600, Math.max(40, startWidth + m.clientX - startX))),
            }
          : c,
      );
      updateLayout({ columns });
    };
    const up = () => {
      target.removeEventListener('pointermove', move);
      target.removeEventListener('pointerup', up);
    };
    target.addEventListener('pointermove', move);
    target.addEventListener('pointerup', up);
  };
  return (
    <div
      role="columnheader"
      className={`c-${column.id}`}
      aria-sort={spec.sort && spec.sort === sort ? (desc ? 'descending' : 'ascending') : undefined}
      draggable
      onDragStart={(e) => e.dataTransfer.setData('text/x-cadenza-column', String(index))}
      onDragOver={(e) => {
        if (e.dataTransfer.types.includes('text/x-cadenza-column')) e.preventDefault();
      }}
      onDrop={(e) => {
        const from = Number(e.dataTransfer.getData('text/x-cadenza-column'));
        const columns = useLook.getState().look.layout.columns;
        if (Number.isInteger(from)) updateLayout({ columns: moveTo(columns, from, index) });
      }}
    >
      {spec.sort ? (
        <button className="column-button" onClick={() => spec.sort && onSort(spec.sort)}>
          {spec.label}
          {spec.sort === sort && (desc ? <ChevronDown size={12} /> : <ChevronUp size={12} />)}
        </button>
      ) : (
        <span className="column-label">{spec.label}</span>
      )}
      {column.id !== 'title' && (
        <span
          className="column-resize"
          aria-hidden
          onPointerDown={resize}
          onClick={(e) => e.stopPropagation()}
        />
      )}
    </div>
  );
}

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
  const columns = useLook((s) => s.look.layout.columns);
  const ROW = DENSITY[useLook((s) => s.look.density)].row;

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

  // One row is in the Tab order (the selected one, else the first visible); arrows move.
  const focusSelected = useRef(false);
  useEffect(() => {
    if (!focusSelected.current || selected === null) return;
    const el = scroller.current?.querySelector<HTMLElement>(`[aria-rowindex="${selected + 1}"]`);
    if (el) {
      el.focus({ preventScroll: true });
      focusSelected.current = false;
    }
  });
  const moveTo = (i: number) => {
    const to = Math.max(0, Math.min((count ?? 1) - 1, i));
    setSelected(to);
    v.scrollToIndex(to, { align: 'auto' });
    focusSelected.current = true;
  };
  const onRowKey = (e: React.KeyboardEvent, i: number) => {
    const page = Math.max(1, Math.floor((scroller.current?.clientHeight ?? 0) / ROW) - 1);
    const to: Record<string, number> = {
      ArrowDown: i + 1,
      ArrowUp: i - 1,
      PageDown: i + page,
      PageUp: i - page,
      Home: 0,
      End: (count ?? 1) - 1,
    };
    if (e.key === 'Enter') play(i);
    else if (e.key in to && !e.shiftKey && !e.ctrlKey && !e.altKey) moveTo(to[e.key] ?? i);
    else return;
    e.preventDefault();
  };
  const tabRow = selected !== null && selected >= first && selected <= last ? selected : first;

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
      <div
        className="song-table"
        role="grid"
        aria-rowcount={count ?? 0}
        style={{ '--song-cols': columnTemplate(columns) } as React.CSSProperties}
      >
        <div className="song-head" role="row">
          {columns.map((c, i) => (
            <ColumnHeader
              key={c.id}
              column={c}
              index={i}
              sort={sort}
              desc={desc}
              onSort={(by) => {
                if (by === sort) setDesc(!desc);
                else {
                  setSort(by);
                  setDesc(false);
                }
              }}
            />
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
                  tabIndex={it.index === tabRow ? 0 : -1}
                  className={`song-row ${it.index % 2 ? 'odd' : ''} ${t && t.id === playingId ? 'playing' : ''} ${selected === it.index ? 'selected' : ''}`}
                  style={{ transform: `translateY(${it.start}px)`, height: ROW }}
                  onClick={() => setSelected(it.index)}
                  onDoubleClick={() => play(it.index)}
                  onKeyDown={(e) => onRowKey(e, it.index)}
                  onContextMenu={t ? menuHandler(() => trackMenu(t)) : undefined}
                >
                  {t ? (
                    columns.map((c) => (
                      <div key={c.id} role="gridcell" className={`c-${c.id}`}>
                        {cell(c.id, t)}
                      </div>
                    ))
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
