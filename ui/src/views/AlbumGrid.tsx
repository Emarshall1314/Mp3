import { useEffect, useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { Play } from 'lucide-react';
import { api } from '../api';
import { albumMenu, menuHandler } from '../actions';
import { attempt, useStore } from '../store';
import type { AlbumRow } from '../types';
import { Artwork } from '../components/Artwork';
import { usePhone } from '../hooks/usePhone';

const PAGE = 120;

/// Grid sizes, smallest cards first. A phone steps through columns (four down to one); the
/// desktop through card sizes, with as many columns as fit.
export const PHONE_COLUMNS = [4, 3, 2, 1];
export const DESKTOP_SIZES = [120, 150, 180, 220, 260, 300];

/// A virtualised grid of albums. `load` fetches a page; `count` is the total.
export function AlbumGrid({
  count,
  load,
  zoom,
  onZoom,
  jump,
  resetKey,
}: {
  count: number;
  load: (offset: number, limit: number) => Promise<AlbumRow[]>;
  /// Index into PHONE_COLUMNS or DESKTOP_SIZES.
  zoom: number;
  /// A pinch on the grid: +1 for bigger cards, −1 for smaller.
  onZoom: (step: 1 | -1) => void;
  /// Set to a function that scrolls the grid to an album's position (the A–Z index).
  jump?: { current: ((index: number) => void) | null };
  resetKey: string;
}) {
  const scroller = useRef<HTMLDivElement>(null);
  // Two fingers on the grid pinch its size.
  const fingers = useRef(new Map<number, [number, number]>());
  const pinchFrom = useRef<number | null>(null);
  const spread = () => {
    const [a, b] = [...fingers.current.values()];
    return fingers.current.size === 2 && a && b ? Math.hypot(a[0] - b[0], a[1] - b[1]) : null;
  };
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

  // As many columns of about `size` as fit, then the cards grow to
  // fill the width so the grid has even margins rather than a ragged gap on the right.
  const phone = usePhone();
  const pad = phone ? 16 : 24;
  const gap = phone ? 14 : 24;
  const size = DESKTOP_SIZES[Math.min(zoom, DESKTOP_SIZES.length - 1)] ?? 180;
  const cols = phone
    ? (PHONE_COLUMNS[Math.min(zoom, PHONE_COLUMNS.length - 1)] ?? 2)
    : Math.max(1, Math.floor((width - 2 * pad + gap) / (size + gap)));
  const card = Math.max(60, Math.floor((width - 2 * pad - (cols - 1) * gap) / cols));
  const rows = Math.ceil(count / cols);
  // Artwork, then the title and artist lines, then the space before the next row.
  const rowHeight = card + (phone ? 70 : 64);
  const v = useVirtualizer({
    count: rows,
    getScrollElement: () => scroller.current,
    estimateSize: () => rowHeight,
    overscan: 3,
  });
  useEffect(() => v.measure(), [rowHeight, cols, v]);
  useEffect(() => {
    if (!jump) return;
    jump.current = (index) => v.scrollToIndex(Math.floor(index / cols), { align: 'start' });
    return () => {
      jump.current = null;
    };
  }, [jump, v, cols]);

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
    <div
      className="scroll album-grid-scroll"
      ref={scroller}
      onKeyDown={onKeyDown}
      onPointerDown={(e) => {
        if (e.pointerType !== 'touch') return;
        fingers.current.set(e.pointerId, [e.clientX, e.clientY]);
        pinchFrom.current = spread();
      }}
      onPointerMove={(e) => {
        if (!fingers.current.has(e.pointerId)) return;
        fingers.current.set(e.pointerId, [e.clientX, e.clientY]);
        const now = spread();
        const from = pinchFrom.current;
        if (now === null || from === null) return;
        // Each 30 % of spread is one step; the next step measures from here.
        if (now / from > 1.3) {
          onZoom(1);
          pinchFrom.current = now;
        } else if (now / from < 0.77) {
          onZoom(-1);
          pinchFrom.current = now;
        }
      }}
      onPointerUp={(e) => {
        fingers.current.delete(e.pointerId);
        pinchFrom.current = spread();
      }}
      onPointerCancel={(e) => {
        fingers.current.delete(e.pointerId);
        pinchFrom.current = spread();
      }}
    >
      {/* Re-keyed when the columns change, so the new arrangement settles in. */}
      <div
        key={cols}
        className="album-grid-body"
        style={{ height: v.getTotalSize(), position: 'relative' }}
      >
        {items.map((row) => (
          <div
            key={row.key}
            className="album-grid-row"
            style={{
              transform: `translateY(${row.start}px)`,
              gap,
              padding: `0 ${pad}px`,
              gridTemplateColumns: `repeat(${cols}, ${card}px)`,
            }}
          >
            {Array.from({ length: cols }, (_, c) => row.index * cols + c)
              .filter((i) => i < count)
              .map((i) => {
                const a = album(i);
                return a ? (
                  <AlbumCard key={a.id} album={a} size={card} index={i} />
                ) : (
                  <div key={`p${i}`} className="album-card placeholder" style={{ width: card }} />
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
