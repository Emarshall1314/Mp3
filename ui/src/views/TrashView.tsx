import { useEffect, useState } from 'react';
import { api } from '../api';
import { attempt, useStore } from '../store';
import type { TrashItem } from '../types';

const DAYS = 30;

/// Songs moved to the Trash: restore them, or delete them now instead of after 30 days.
export function TrashView() {
  const libraryVersion = useStore((s) => s.libraryVersion);
  const [items, setItems] = useState<TrashItem[]>([]);
  // When the list was read, for the days left.
  const [now, setNow] = useState(0);
  const reload = () =>
    void api.trashItems().then((list) => {
      setNow(Date.now() / 1000);
      setItems(list);
    });
  useEffect(reload, [libraryVersion]);

  const done = () => {
    reload();
    useStore.getState().libraryChanged();
  };

  return (
    <section className="view scroll trash-view">
      <header className="view-header">
        <h1>Trash</h1>
        <div className="view-controls">
          <button
            className="pill"
            disabled={items.length === 0}
            onClick={() => void attempt(api.emptyTrash()).then(done)}
          >
            Delete All Now
          </button>
        </div>
      </header>
      <p className="muted">
        Songs moved to the Trash keep their files here for {DAYS} days, then are deleted. With sync
        on, moving a song to the Trash or restoring it does the same on your other devices; deleting
        now only affects this device.
      </p>
      {items.length === 0 ? (
        <p className="muted">The Trash is empty.</p>
      ) : (
        <ul className="history-list">
          {items.map((t) => {
            const left = Math.max(0, Math.ceil(DAYS - (now - t.trashedAt) / 86400));
            return (
              <li key={t.id}>
                <div>
                  <span>
                    {t.title}
                    {t.artist && ` — ${t.artist}`}
                  </span>
                  <span className="muted small">
                    {t.album && `${t.album} · `}deleted in {left} {left === 1 ? 'day' : 'days'} ·{' '}
                    <span className="path">{t.originalPath}</span>
                  </span>
                </div>
                <div className="row-actions">
                  <button
                    className="pill"
                    onClick={() => void attempt(api.restore(t.id)).then(done)}
                  >
                    Restore
                  </button>
                  <button className="pill" onClick={() => void attempt(api.purge(t.id)).then(done)}>
                    Delete Now
                  </button>
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
