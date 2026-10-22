import { useEffect, useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { Play } from 'lucide-react';
import { api } from '../api';
import { albumMenu, menuHandler } from '../actions';
import { attempt, useStore } from '../store';
import type { AlbumRow } from '../types';
import { Artwork } from '../components/Artwork';

const GAP = 24;
const PAGE = 120;

/// A virtualised grid of albums. `load` fetches a page; `count` is the total.
export function AlbumGrid({
  count,
  load,
  size,
  resetKey,
}: {
  count: number;
  load: (offset: number, limit: number) => Promise<AlbumRow[]>;
  size: number;
  resetKey: string;
}) {
  const scroller = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(800);
  const [pages, setPages] = useState<Map<number, AlbumRow[]>>(new Map());
  const pending = useRef(new Set<number>());

  useEffect(() => {
    setPages(new Map());
    pending.current.clear();
  }, [resetKey]);

  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setWidth(el.clientWidth));
    ro.observe(el);
    setWidth(el.clientWidth);
    return () => ro.disconnect();
  }, []);

  const cols = Math.max(1, Math.floor((width - 2 * GAP + GAP) / (size + GAP)));
  const rows = Math.ceil(count / cols);
  const rowHeight = size + 56;
  const v = useVirtualizer({
    count: rows,
    getScrollElement: () => scroller.current,
    estimateSize: () => rowHeight,
    overscan: 3,
  });
  useEffect(() => v.measure(), [rowHeight, cols, v]);

  const items = v.getVirtualItems();
  const first = (items[0]?.index ?? 0) * cols;
  const last = ((items.at(-1)?.index ?? 0) + 1) * cols;
  useEffect(() => {
    for (let p = Math.floor(first / PAGE); p <= Math.floor(last / PAGE); p++) {
      if (pages.has(p) || pending.current.has(p) || p * PAGE >= count) continue;
      pending.current.add(p);
      void load(p * PAGE, PAGE).then((rows) => {
        pending.current.delete(p);
        setPages((prev) => new Map(prev).set(p, rows));
      });
    }
  }, [first, last, pages, load, count]);

  // Arrow keys move focus between albums, a row at a time up and down.
  // Focuses the album now if its row is rendered, else after scrolling renders it.
  const focusIndex = useRef<number | null>(null);
  const focusPending = () => {
    if (focusIndex.current === null) return;
    const el = scroller.current?.querySelector<HTMLElement>(
      `.album-art-button[data-index="${focusIndex.current}"]`,
    );
    if (el) {
      el.focus({ preventScroll: true });
      focusIndex.current = null;
    }
  };
  useEffect(focusPending);
  const onKeyDown = (e: React.KeyboardEvent) => {
    const at = (e.target as HTMLElement).dataset.index;
    if (at === undefined || e.shiftKey || e.ctrlKey || e.altKey || e.metaKey) return;
    const i = Number(at);
    const delta: Record<string, number> = {
      ArrowRight: 1,
      ArrowLeft: -1,
      ArrowDown: cols,
      ArrowUp: -cols,
    };
    const d = delta[e.key];
    if (d === undefined) return;
    e.preventDefault();
    const to = i + d;
    if (to < 0 || to >= count) return;
    focusIndex.current = to;
    v.scrollToIndex(Math.floor(to / cols), { align: 'auto' });
    focusPending();
  };

  const album = (i: number): AlbumRow | undefined => pages.get(Math.floor(i / PAGE))?.[i % PAGE];

  return (
    <div className="scroll" ref={scroller} onKeyDown={onKeyDown}>
      <div style={{ height: v.getTotalSize(), position: 'relative' }}>
        {items.map((row) => (
          <div
            key={row.key}
            className="album-grid-row"
            style={{
              transform: `translateY(${row.start}px)`,
              gap: GAP,
              padding: `0 ${GAP}px`,
              gridTemplateColumns: `repeat(${cols}, ${size}px)`,
            }}
          >
            {Array.from({ length: cols }, (_, c) => row.index * cols + c)
              .filter((i) => i < count)
              .map((i) => {
                const a = album(i);
                return a ? (
                  <AlbumCard key={a.id} album={a} size={size} index={i} />
                ) : (
                  <div key={`p${i}`} className="album-card placeholder" style={{ width: size }} />
                );
              })}
          </div>
        ))}
      </div>
    </div>
  );
}

/// `index` marks the card's place in a grid for arrow-key movement.
export function AlbumCard({
  album,
  size,
  index,
}: {
  album: AlbumRow;
  size: number;
  index?: number;
}) {
  const navigate = useStore((s) => s.navigate);
  const items = useMemo(() => () => albumMenu(album), [album]);
  return (
    <div className="album-card" style={{ width: size }} onContextMenu={menuHandler(items)}>
      <button
        className="album-art-button"
        data-index={index}
        onClick={() => navigate({ name: 'album', id: album.id })}
        aria-label={`${album.title} by ${album.albumartist}`}
      >
        <Artwork hash={album.artHash} size={size} alt="" />
      </button>
      <button
        className="play-overlay"
        aria-label={`Play ${album.title}`}
        onClick={(e) => {
          e.stopPropagation();
          void attempt(api.playAlbum(album.id, 0));
        }}
      >
        <Play size={18} fill="currentColor" />
      </button>
      <button
        className="link title"
        tabIndex={-1}
        onClick={() => navigate({ name: 'album', id: album.id })}
      >
        {album.title || 'Unknown Album'}
      </button>
      <button
        className="link subtitle"
        tabIndex={-1}
        onClick={() => navigate({ name: 'artist', artist: album.albumartist })}
      >
        {album.albumartist || 'Unknown Artist'}
      </button>
    </div>
  );
}
