import { useEffect, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { albumMenu, menuHandler } from '../actions';
import { longDuration } from '../format';
import { useStore } from '../store';
import type { AlbumRow } from '../types';
import { Artwork } from '../components/Artwork';
import { QualityBadge } from '../components/QualityBadge';

const ROW = 56;
const PAGE = 200;

/// Albums as a list (artwork, title, artist, year, genre, length, quality), virtualised. One
/// Tab stop; arrow keys move, Enter opens.
export function AlbumList({
  count,
  load,
  resetKey,
}: {
  count: number;
  load: (offset: number, limit: number) => Promise<AlbumRow[]>;
  resetKey: string;
}) {
  const navigate = useStore((s) => s.navigate);
  const scroller = useRef<HTMLDivElement>(null);
  const [pages, setPages] = useState<Map<number, AlbumRow[]>>(new Map());
  const [focused, setFocused] = useState(0);
  const pending = useRef(new Set<number>());

  useEffect(() => {
    setPages(new Map());
    pending.current.clear();
  }, [resetKey]);

  const v = useVirtualizer({
    count,
    getScrollElement: () => scroller.current,
    estimateSize: () => ROW,
    overscan: 10,
  });
  const items = v.getVirtualItems();
  const first = items[0]?.index ?? 0;
  const last = items.at(-1)?.index ?? 0;
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

  const focusRow = (i: number) => {
    const to = Math.max(0, Math.min(count - 1, i));
    setFocused(to);
    v.scrollToIndex(to, { align: 'auto' });
    setTimeout(() =>
      scroller.current
        ?.querySelector<HTMLElement>(`[data-index="${to}"]`)
        ?.focus({ preventScroll: true }),
    );
  };

  return (
    <div className="scroll album-list" ref={scroller} role="list">
      <div style={{ height: v.getTotalSize(), position: 'relative' }}>
        {items.map((it) => {
          const a = pages.get(Math.floor(it.index / PAGE))?.[it.index % PAGE];
          return (
            <div
              key={it.key}
              role="listitem"
              className="album-list-row"
              style={{ transform: `translateY(${it.start}px)`, height: ROW }}
            >
              {a ? (
                <button
                  data-index={it.index}
                  tabIndex={it.index === focused ? 0 : -1}
                  onClick={() => navigate({ name: 'album', id: a.id })}
                  onContextMenu={menuHandler(() => albumMenu(a))}
                  onKeyDown={(e) => {
                    const to: Record<string, number> = {
                      ArrowDown: it.index + 1,
                      ArrowUp: it.index - 1,
                      Home: 0,
                      End: count - 1,
                    };
                    if (!(e.key in to)) return;
                    e.preventDefault();
                    focusRow(to[e.key] ?? it.index);
                  }}
                >
                  <Artwork hash={a.artHash} size={40} alt="" />
                  <span className="album-list-title">
                    <span>{a.title || 'Unknown Album'}</span>
                    <span className="subtitle">{a.albumartist || 'Unknown Artist'}</span>
                  </span>
                  <span className="muted">{a.year ?? ''}</span>
                  <span className="muted">{a.genre ?? ''}</span>
                  <span className="muted">
                    {a.trackCount} {a.trackCount === 1 ? 'song' : 'songs'},{' '}
                    {longDuration(a.durationMs)}
                  </span>
                  <span>{a.quality && <QualityBadge quality={a.quality} />}</span>
                </button>
              ) : (
                <div className="loading-cell" />
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
