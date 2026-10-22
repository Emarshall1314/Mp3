import { useEffect, useState } from 'react';
import { X } from 'lucide-react';
import { api } from '../api';
import { trackMenu } from '../actions';
import { duration } from '../format';
import { attempt, useStore } from '../store';
import type { TrackRow } from '../types';
import { Artwork } from './Artwork';

/// Entries shown around the current one; a queue can hold a whole 200 000-song library.
const HISTORY = 10;
const AHEAD = 200;

/// The play queue: what is playing, what comes next; drag to reorder.
export function QueuePanel() {
  const player = useStore((s) => s.player);
  const openMenu = useStore((s) => s.openMenu);
  const [rows, setRows] = useState<TrackRow[]>([]);
  const [dragFrom, setDragFrom] = useState<number | null>(null);
  const current = player?.index ?? -1;
  const first = Math.max(0, current - HISTORY);
  const version = player?.queueVersion ?? 0;

  useEffect(() => {
    let live = true;
    void api.queueWindow(first, current - first + 1 + AHEAD).then((r) => live && setRows(r));
    return () => {
      live = false;
    };
  }, [first, current, version]);

  const total = player?.queueLength ?? 0;
  const upcoming = total - current - 1;
  const hidden = Math.max(0, total - (first + rows.length));

  return (
    <aside className="queue-panel" aria-label="Queue">
      <header>
        <h2>Playing Next</h2>
        {upcoming > 0 && (
          <button className="text-button" onClick={() => void attempt(api.clearUpcoming())}>
            Clear
          </button>
        )}
      </header>
      {rows.length === 0 && (
        <p className="empty">
          The queue is empty. Play something, or use “Play Next” and “Play Later”.
        </p>
      )}
      <ol className="queue-list">
        {rows.map((t, offset) => {
          const i = first + offset;
          return (
            <li
              key={`${t.id}-${i}`}
              className={`queue-row ${i === current ? 'current' : ''} ${i < current ? 'history' : ''}`}
              draggable
              onDragStart={() => setDragFrom(i)}
              onDragOver={(e) => e.preventDefault()}
              onDrop={() => {
                if (dragFrom !== null && dragFrom !== i) void attempt(api.moveInQueue(dragFrom, i));
                setDragFrom(null);
              }}
              onDoubleClick={() => void attempt(api.playIndex(i))}
              onContextMenu={(e) => {
                e.preventDefault();
                openMenu(e.clientX, e.clientY, trackMenu(t));
              }}
            >
              <Artwork hash={t.artHash} size={36} alt="" />
              <div className="queue-text">
                <span className="title">{t.title}</span>
                <span className="subtitle">{t.artist}</span>
              </div>
              <span className="time">{duration(t.durationMs)}</span>
              <button
                className="icon-button small"
                aria-label={`Remove ${t.title} from the queue`}
                onClick={() => void attempt(api.removeFromQueue(i))}
              >
                <X size={14} />
              </button>
            </li>
          );
        })}
      </ol>
      {hidden > 0 && <p className="queue-more">and {hidden.toLocaleString()} more</p>}
    </aside>
  );
}
